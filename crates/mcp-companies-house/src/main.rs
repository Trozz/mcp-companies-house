use mcp_companies_house::{Loaded, SERVICES, VERSION, app, combined_tool_names, load, log_filter};
use mcp_server_core::config::EnvReader;
use mcp_server_core::service::{HttpSettings, ServiceContext, ServiceEnv};
use mcp_server_core::{install_crypto_provider, observability, shutdown_signal, tls};
use std::net::SocketAddr;
use std::process::ExitCode;

const USAGE: &str = "usage: mcp-companies-house [--version | --healthcheck]\nAll configuration is read from environment variables; see README.md.";

#[tokio::main]
async fn main() -> ExitCode {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("--version" | "-V") => {
            println!("mcp-companies-house {VERSION}");
            return ExitCode::SUCCESS;
        }
        Some("--healthcheck") => return healthcheck().await,
        Some("--help" | "-h") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("unknown argument: {other}\n{USAGE}");
            return ExitCode::from(2);
        }
        None => {}
    }
    install_crypto_provider();
    let mut env: ServiceEnv = EnvReader::new(Box::new(|k: &str| std::env::var(k).ok()));
    let Some(loaded) = load(&mut env, SERVICES) else {
        for e in &env.errors {
            eprintln!("configuration error: {e}");
        }
        return ExitCode::FAILURE;
    };
    match run(loaded).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "fatal");
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Container HEALTHCHECK: GET /healthz on 127.0.0.1 at the BIND_ADDR port.
async fn healthcheck() -> ExitCode {
    install_crypto_provider();
    let mut env = EnvReader::from_process();
    let addr = env.parse("BIND_ADDR", SocketAddr::from(([0, 0, 0, 0], 8443)));
    let insecure = env.bool("INSECURE_HTTP", false);
    if tls::healthcheck(addr.port(), !insecure).await {
        return ExitCode::SUCCESS;
    }
    eprintln!("healthcheck failed");
    ExitCode::FAILURE
}

async fn run(loaded: Loaded) -> anyhow::Result<()> {
    let Loaded { core, services } = loaded;
    let caps: Vec<&str> = services
        .iter()
        .flat_map(|s| s.log_caps().iter().copied())
        .collect();
    let mut obs = core.observability.clone();
    obs.log_level = log_filter(&obs.log_level, &caps);
    let _guard = observability::init(&obs)?;
    tracing::info!(version = VERSION, "starting mcp-companies-house");
    for s in &services {
        tracing::info!(
            service = s.name(),
            url = core.service_url(s.name()),
            tools = ?s.tool_names(),
            "service enabled"
        );
    }
    let combined = combined_tool_names(&services);
    if !combined.is_empty() {
        tracing::info!(
            url = core.service_url(mcp_server_core::combined::COMBINED),
            tools = combined.len(),
            "combined endpoint enabled"
        );
    }

    let shutdown = shutdown_signal();
    let ctx = ServiceContext {
        http: HttpSettings::from_core(&core),
        shutdown: shutdown.clone(),
    };
    let app = app(core.oauth, &services, &ctx);
    tracing::info!(addr = %core.bind_addr, "listening");
    tls::serve(app, core.bind_addr, core.tls, shutdown).await
}
