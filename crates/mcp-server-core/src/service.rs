use crate::config::{CoreConfig, EnvReader};
use crate::server::BODY_LIMIT;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rmcp::ServerHandler;
use rmcp::model::Icon;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// The environment reader every service factory receives.
pub type ServiceEnv = EnvReader<Box<dyn Fn(&str) -> Option<String>>>;

/// Reads a service's configuration. Errors go to `env.errors`; `None` means the service cannot start.
pub type ServiceFactory = fn(&mut ServiceEnv) -> Option<Arc<dyn Service>>;

/// What the MCP HTTP layer needs from the core configuration.
#[derive(Debug, Clone)]
pub struct HttpSettings {
    /// External base URL without a trailing slash, e.g. `https://icloud.example.com`.
    pub public_url: String,
    /// Host part of `public_url`.
    pub public_host: String,
    /// Extra allowed browser origins (`ALLOWED_ORIGINS`), passed to rmcp as given.
    pub allowed_origins: Vec<String>,
}

impl HttpSettings {
    pub fn from_core(core: &CoreConfig) -> Self {
        Self {
            public_url: core.public_url.clone(),
            public_host: core.public_host.clone(),
            allowed_origins: core.allowed_origins.clone(),
        }
    }
}

pub struct ServiceContext {
    pub http: HttpSettings,
    pub shutdown: CancellationToken,
}

/// One MCP service (mail, calendar, ...) mounted by the server at `/<name>`.
pub trait Service: Send + Sync + 'static {
    /// Path segment, OAuth resource suffix and scope, e.g. "mail".
    fn name(&self) -> &'static str;
    /// Tool names, logged at startup.
    fn tool_names(&self) -> Vec<String>;
    /// Tracing targets this service needs capped at info because their trace output can leak secrets.
    fn log_caps(&self) -> &'static [&'static str] {
        &[]
    }
    /// The rmcp Streamable HTTP service for this service, mounted by core at `/<name>`.
    fn router(self: Arc<Self>, ctx: &ServiceContext) -> axum::Router;
}

/// `scheme://host:port` for `public_url`, with the scheme's default port made explicit so rmcp
/// matches exactly that origin (rmcp treats a port-less entry as "any port").
fn origin_with_port(public_url: &str) -> String {
    let (scheme, rest) = public_url
        .split_once("://")
        .unwrap_or(("https", public_url));
    let authority = rest.split('/').next().unwrap_or(rest);
    let has_port = authority
        .rsplit_once(':')
        .is_some_and(|(host, port)| !host.ends_with(':') && port.parse::<u16>().is_ok());
    if has_port {
        return format!("{scheme}://{authority}");
    }
    let port = if scheme == "http" { 80 } else { 443 };
    format!("{scheme}://{authority}:{port}")
}

/// Wraps an rmcp server factory in the Streamable HTTP service with host and origin checks, the
/// body limit and shutdown wiring. The factory runs per session: only clone `Arc`s in it.
pub fn mcp_service<S, F>(ctx: &ServiceContext, factory: F) -> axum::Router
where
    S: ServerHandler + Send + 'static,
    F: Fn() -> Result<S, std::io::Error> + Send + Sync + 'static,
{
    let http = &ctx.http;
    let mut hosts = vec![http.public_host.clone()];
    for h in ["localhost", "127.0.0.1", "::1"] {
        if !hosts.iter().any(|x| x == h) {
            hosts.push(h.to_string());
        }
    }
    let mut origins = vec![origin_with_port(&http.public_url)];
    origins.extend(http.allowed_origins.iter().cloned());
    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(hosts)
        .with_allowed_origins(origins)
        .enforce_origin_validation()
        .with_max_request_body_bytes(BODY_LIMIT)
        .with_cancellation_token(ctx.shutdown.clone());
    let service =
        StreamableHttpService::new(factory, Arc::new(LocalSessionManager::default()), config);
    axum::Router::new().fallback_service(service)
}

/// A service's `serverInfo` icons as data URIs: the 128x128 PNG first, then the SVG.
pub fn icons(png: &[u8], svg: &str) -> Vec<Icon> {
    vec![
        Icon::new(format!("data:image/png;base64,{}", STANDARD.encode(png)))
            .with_mime_type("image/png")
            .with_sizes(vec!["128x128".into()]),
        Icon::new(format!(
            "data:image/svg+xml;base64,{}",
            STANDARD.encode(svg)
        ))
        .with_mime_type("image/svg+xml")
        .with_sizes(vec!["any".into()]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_png_then_svg_data_uris() {
        let icons = icons(b"png-bytes", "<svg/>");
        assert_eq!(icons.len(), 2);
        assert_eq!(
            icons[0].src,
            format!("data:image/png;base64,{}", STANDARD.encode(b"png-bytes"))
        );
        assert_eq!(icons[0].mime_type.as_deref(), Some("image/png"));
        assert_eq!(icons[0].sizes, Some(vec!["128x128".to_string()]));
        assert_eq!(
            icons[1].src,
            format!("data:image/svg+xml;base64,{}", STANDARD.encode("<svg/>"))
        );
        assert_eq!(icons[1].mime_type.as_deref(), Some("image/svg+xml"));
        assert_eq!(icons[1].sizes, Some(vec!["any".to_string()]));
    }

    #[test]
    fn origin_gets_explicit_default_port() {
        assert_eq!(
            origin_with_port("https://mail.example.com"),
            "https://mail.example.com:443"
        );
        assert_eq!(
            origin_with_port("http://localhost:8443"),
            "http://localhost:8443"
        );
        assert_eq!(origin_with_port("http://localhost"), "http://localhost:80");
        assert_eq!(origin_with_port("https://[::1]"), "https://[::1]:443");
        assert_eq!(origin_with_port("https://[::1]:9000"), "https://[::1]:9000");
    }
}
