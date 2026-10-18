//! Several services' MCP handlers served as one MCP server: the combined `/mcp` endpoint.
//!
//! Each service's tools appear as `<service>_<tool>`; a call is routed by that prefix to the
//! service's handler with the original tool name. Prompts are combined the same way
//! (`<service>_<prompt>`), and the tool names inside a prompt's text are given their prefix so
//! the model calls the tools by the names it sees here. [`DynHandler`] erases a handler's type so
//! handlers of different services fit in one list. Calls keep their request context, so tools
//! read the caller's credential from the request extensions exactly as on `/<service>`.
//!
//! Kept in step with mcp-icloud's `mcp-server-core::combined`; prompts are an addition here.

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ContentBlock, GetPromptRequestParams,
    GetPromptResponse, Implementation, ListPromptsResult, ListToolsResult, PaginatedRequestParams,
    Prompt, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler};
use std::future::Future;
use std::pin::Pin;

/// Path segment, OAuth resource suffix and scope of the combined endpoint; never a service name.
pub const COMBINED: &str = "mcp";

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The parts of an MCP handler the combined endpoint uses, in a form that can be boxed.
pub trait ToolHandler: Send + Sync + 'static {
    fn info(&self) -> ServerConfig;
    fn tool(&self, name: &str) -> Option<Tool>;
    fn list(
        &self,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<Vec<Tool>, McpError>>;
    fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<CallToolResponse, McpError>>;
    fn prompts(
        &self,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<Vec<Prompt>, McpError>>;
    fn prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<GetPromptResponse, McpError>>;
}

impl<S: ServerHandler> ToolHandler for S {
    fn info(&self) -> ServerConfig {
        self.get_info()
    }

    fn tool(&self, name: &str) -> Option<Tool> {
        self.get_tool(name)
    }

    /// Services list every tool in one page.
    fn list(
        &self,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<Vec<Tool>, McpError>> {
        Box::pin(async move { Ok(self.list_tools(None, context).await?.tools) })
    }

    fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<CallToolResponse, McpError>> {
        Box::pin(self.call_tool(request, context))
    }

    /// Services list every prompt in one page; one without prompts lists none.
    fn prompts(
        &self,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<Vec<Prompt>, McpError>> {
        Box::pin(async move { Ok(self.list_prompts(None, context).await?.prompts) })
    }

    fn prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> BoxFuture<'_, Result<GetPromptResponse, McpError>> {
        Box::pin(self.get_prompt(request, context))
    }
}

/// A boxed service handler that is itself an MCP handler (tools only).
pub struct DynHandler(Box<dyn ToolHandler>);

impl DynHandler {
    pub fn new(handler: impl ToolHandler) -> Self {
        Self(Box::new(handler))
    }
}

impl ServerHandler for DynHandler {
    fn get_info(&self) -> ServerConfig {
        self.0.info()
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.0.tool(name)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.0.list(context).await?))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        self.0.call(request, context).await
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(
            self.0.prompts(context).await?,
        ))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        self.0.prompt(request, context).await
    }
}

/// `<service>_<tool>`.
pub fn prefixed_name(service: &str, tool: &str) -> String {
    format!("{service}_{tool}")
}

fn prefixed(service: &str, mut tool: Tool) -> Tool {
    tool.name = prefixed_name(service, &tool.name).into();
    tool
}

/// Gives every whole identifier in `text` that is one of `tools` its service prefix, so a
/// prompt written for `/<service>` names the tools as `/mcp` lists them. Identifiers are runs of
/// ASCII letters, digits and `_`, so `get_company` inside `get_company_registers` is left alone.
fn prefix_tool_names(service: &str, tools: &[String], text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if tools.iter().any(|t| t == word) {
            out.push_str(&prefixed_name(service, word));
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// The service a prefixed tool name belongs to, and the tool's own name. Service names never
/// contain `_`, so the first `_` ends the prefix.
fn split<'a>(services: &[&str], name: &'a str) -> Option<(usize, &'a str)> {
    let (service, tool) = name.split_once('_')?;
    let index = services.iter().position(|s| *s == service)?;
    (!tool.is_empty()).then_some((index, tool))
}

/// A short guide to the prefixes, then each service's own instructions under its name.
fn instructions(services: &[(&str, Option<String>)]) -> String {
    let prefixes: Vec<String> = services.iter().map(|(s, _)| format!("{s}_")).collect();
    let mut text = format!(
        "This server combines several services. Each tool and prompt name starts with its service: {}. \
         The notes below, tool descriptions and error messages name tools without that prefix.",
        prefixes.join(", ")
    );
    for (service, own) in services {
        if let Some(own) = own.as_deref().filter(|t| !t.trim().is_empty()) {
            text.push_str(&format!("\n\n{service}:\n{}", own.trim()));
        }
    }
    text
}

/// One MCP server over several services' handlers.
pub struct Combined {
    info: ServerConfig,
    names: Vec<&'static str>,
    handlers: Vec<DynHandler>,
}

impl Combined {
    /// `services` in mount order. Service names must not contain `_` (they are lowercase path
    /// segments).
    pub fn new(server_info: Implementation, services: Vec<(&'static str, DynHandler)>) -> Self {
        let own: Vec<(&str, Option<String>)> = services
            .iter()
            .map(|(name, h)| (*name, h.get_info().instructions))
            .collect();
        let has_prompts = services
            .iter()
            .any(|(_, h)| h.get_info().capabilities.prompts.is_some());
        let capabilities = if has_prompts {
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build()
        } else {
            ServerCapabilities::builder().enable_tools().build()
        };
        let info = ServerConfig::new(capabilities)
            .with_server_info(server_info)
            .with_instructions(instructions(&own));
        let (names, handlers) = services.into_iter().unzip();
        Self {
            info,
            names,
            handlers,
        }
    }
}

impl ServerHandler for Combined {
    fn get_info(&self) -> ServerConfig {
        self.info.clone()
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        let (index, tool) = split(&self.names, name)?;
        self.handlers[index]
            .get_tool(tool)
            .map(|t| prefixed(self.names[index], t))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let mut tools = Vec::new();
        for (service, handler) in self.names.iter().zip(&self.handlers) {
            let listed = handler.list_tools(None, context.clone()).await?.tools;
            tools.extend(listed.into_iter().map(|t| prefixed(service, t)));
        }
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let Some((index, tool)) = split(&self.names, &request.name) else {
            return Err(McpError::invalid_params("tool not found", None));
        };
        request.name = tool.to_string().into();
        self.handlers[index].call_tool(request, context).await
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        let mut prompts = Vec::new();
        for (service, handler) in self.names.iter().zip(&self.handlers) {
            let listed = handler.list_prompts(None, context.clone()).await?.prompts;
            prompts.extend(listed.into_iter().map(|mut p| {
                p.name = prefixed_name(service, &p.name);
                p
            }));
        }
        Ok(ListPromptsResult::with_all_items(prompts))
    }

    async fn get_prompt(
        &self,
        mut request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        let Some((index, prompt)) = split(&self.names, &request.name) else {
            return Err(McpError::invalid_params("prompt not found", None));
        };
        request.name = prompt.to_string();
        let service = self.names[index];
        let handler = &self.handlers[index];
        let response = handler.get_prompt(request, context.clone()).await?;
        let GetPromptResponse::Complete(mut result) = response else {
            return Ok(response);
        };
        let tools: Vec<String> = handler
            .list_tools(None, context)
            .await?
            .tools
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        for message in &mut result.messages {
            if let ContentBlock::Text(t) = &mut message.content {
                t.text = prefix_tool_names(service, &tools, &t.text);
            }
        }
        Ok(GetPromptResponse::Complete(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn names_are_prefixed_with_their_service() {
        assert_eq!(
            prefixed_name("companies", "search_companies"),
            "companies_search_companies"
        );
        let tool = Tool::new("get_company", "d", Arc::new(serde_json::Map::new()));
        assert_eq!(prefixed("companies", tool).name, "companies_get_company");
    }

    #[test]
    fn a_prefixed_name_routes_to_its_service_and_original_tool() {
        let services = ["companies", "streaming"];
        assert_eq!(
            split(&services, "streaming_read_stream"),
            Some((1, "read_stream"))
        );
        assert_eq!(
            split(&services, "companies_get_company"),
            Some((0, "get_company"))
        );
        assert_eq!(split(&services, "get_company"), None);
        assert_eq!(split(&services, "filing_submit"), None);
        assert_eq!(split(&services, "companies_"), None);
        assert_eq!(split(&services, "companies"), None);
    }

    #[test]
    fn tool_names_in_prompt_text_get_their_prefix() {
        let tools = vec!["get_company".to_string(), "list_officers".to_string()];
        assert_eq!(
            prefix_tool_names(
                "companies",
                &tools,
                "Call get_company, then list_officers (not get_company_registers or my_get_company)."
            ),
            "Call companies_get_company, then companies_list_officers (not get_company_registers or my_get_company)."
        );
        assert_eq!(
            prefix_tool_names("companies", &tools, "get_company"),
            "companies_get_company"
        );
        assert_eq!(prefix_tool_names("companies", &tools, "£ – ü"), "£ – ü");
    }

    #[test]
    fn instructions_name_the_prefixes_and_keep_each_services_own() {
        let text = instructions(&[
            ("companies", Some("Find companies.\n".into())),
            ("streaming", None),
            ("filing", Some("File changes.".into())),
        ]);
        assert_eq!(
            text,
            "This server combines several services. Each tool and prompt name starts with its service: \
companies_, streaming_, filing_. The notes below, tool descriptions and error messages name \
tools without that prefix.\n\ncompanies:\nFind companies.\n\nfiling:\nFile changes."
        );
    }
}
