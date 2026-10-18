//! The combined endpoint, `/mcp`: every enabled service's tools in one MCP server, each tool name
//! prefixed with its service's name and an underscore (`companies_search_companies`). Calls go to
//! the service's own handler with the request context unchanged, so tools see the same request
//! extensions (the caller's credential) as on the service's own path.

use crate::service::{Service, ServiceContext, mcp_service};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Path segment, OAuth resource suffix and scope of the combined endpoint.
pub const COMBINED: &str = "mcp";

/// Longest tool name clients accept.
pub const MAX_TOOL_NAME: usize = 64;

/// Most pages read from one service's tool list.
const MAX_PAGES: usize = 50;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One service's tools behind a type-erased handler, so the combined server can hold services of
/// different handler types. Implemented for every [`ServerHandler`].
pub trait ToolSet: Send + Sync + 'static {
    fn info(&self) -> ServerConfig;
    fn tool(&self, name: &str) -> Option<Tool>;
    fn list(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<ListToolsResult, McpError>>;
    fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<CallToolResponse, McpError>>;
}

impl<S: ServerHandler + Sync> ToolSet for S {
    fn info(&self) -> ServerConfig {
        ServerHandler::get_info(self)
    }

    fn tool(&self, name: &str) -> Option<Tool> {
        ServerHandler::get_tool(self, name)
    }

    fn list(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<ListToolsResult, McpError>> {
        Box::pin(ServerHandler::list_tools(self, request, context))
    }

    fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<CallToolResponse, McpError>> {
        Box::pin(ServerHandler::call_tool(self, request, context))
    }
}

/// `service_tool`, the name a service's tool has on `/mcp`.
pub fn prefixed(service: &str, tool: &str) -> String {
    format!("{service}_{tool}")
}

/// The MCP server behind `/mcp`. Built once per session.
pub struct Combined {
    info: ServerConfig,
    sets: Vec<(&'static str, Box<dyn ToolSet>)>,
}

impl Combined {
    /// `sets` pairs each service name with its tools, in the order they are listed. Server
    /// instructions are put together from each service's own.
    pub fn new(
        implementation: Implementation,
        sets: Vec<(&'static str, Box<dyn ToolSet>)>,
    ) -> Self {
        let mut instructions = String::from(
            "This server combines several services. Each tool name starts with the name of the \
             service it belongs to and an underscore: the companies service's search_companies \
             is companies_search_companies here. The notes below, tool descriptions and error \
             messages come from each service and name tools without that prefix.",
        );
        for (name, set) in &sets {
            if let Some(text) = set.info().instructions {
                instructions.push_str(&format!("\n\n## {name}\n\n{text}"));
            }
        }
        let info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(implementation)
            .with_instructions(instructions);
        Combined { info, sets }
    }

    /// The service handling `name` on `/mcp`, and the tool's name in that service. When one
    /// service name extends another ("a" and "a_b"), the longer match wins.
    fn route(&self, name: &str) -> Option<(&dyn ToolSet, &'static str, String)> {
        self.sets
            .iter()
            .filter_map(|(service, set)| {
                let rest = name.strip_prefix(service)?.strip_prefix('_')?;
                (!rest.is_empty()).then(|| (set.as_ref(), *service, rest.to_string()))
            })
            .max_by_key(|(_, service, _)| service.len())
    }
}

fn unknown_tool(name: &str) -> McpError {
    McpError::invalid_params(format!("tool not found: {name}"), None)
}

impl ServerHandler for Combined {
    fn get_info(&self) -> ServerConfig {
        self.info.clone()
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        let (set, service, tool) = self.route(name)?;
        let mut t = set.tool(&tool)?;
        t.name = prefixed(service, &t.name).into();
        Some(t)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let mut tools = Vec::new();
        for (service, set) in &self.sets {
            let mut cursor = None;
            for _ in 0..MAX_PAGES {
                let request =
                    cursor.map(|c| PaginatedRequestParams::default().with_cursor(Some(c)));
                let page = set.list(request, context.clone()).await?;
                for mut t in page.tools {
                    t.name = prefixed(service, &t.name).into();
                    tools.push(t);
                }
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
        }
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let Some((set, _, tool)) = self.route(&request.name) else {
            return Err(unknown_tool(&request.name));
        };
        request.name = tool.into();
        set.call(request, context).await
    }
}

/// The `/mcp` router over every service that offers its tools there ([`Service::tool_set`]), or
/// `None` when none does.
pub fn router(
    ctx: &ServiceContext,
    services: &[Arc<dyn Service>],
    implementation: Implementation,
) -> Option<axum::Router> {
    let services: Vec<Arc<dyn Service>> = services
        .iter()
        .filter(|s| s.tool_set().is_some())
        .cloned()
        .collect();
    if services.is_empty() {
        return None;
    }
    Some(mcp_service(ctx, move || {
        let sets = services
            .iter()
            .filter_map(|s| s.tool_set().map(|t| (s.name(), t)))
            .collect();
        Ok(Combined::new(implementation.clone(), sets))
    }))
}

/// The names the combined endpoint gives `service`'s tools.
pub fn tool_names(service: &dyn Service) -> Vec<String> {
    service
        .tool_names()
        .iter()
        .map(|t| prefixed(service.name(), t))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_prefixed_with_the_service() {
        assert_eq!(
            prefixed("companies", "search_companies"),
            "companies_search_companies"
        );
    }

    #[test]
    fn calls_route_by_the_longest_matching_service() {
        #[derive(Clone)]
        struct Empty;
        impl ServerHandler for Empty {}
        let c = Combined::new(
            Implementation::new("t", "1"),
            vec![("a", Box::new(Empty)), ("a_b", Box::new(Empty))],
        );
        let (_, service, tool) = c.route("a_tool").unwrap();
        assert_eq!((service, tool.as_str()), ("a", "tool"));
        let (_, service, tool) = c.route("a_b_tool").unwrap();
        assert_eq!((service, tool.as_str()), ("a_b", "tool"));
        assert!(c.route("a_").is_none());
        assert!(c.route("atool").is_none());
        assert!(c.route("b_tool").is_none());
    }
}
