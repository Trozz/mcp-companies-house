use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

#[derive(Debug, Clone, Copy)]
pub enum LogFormat {
    Json,
    Pretty,
}

impl FromStr for LogFormat {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Ok(LogFormat::Json),
            "pretty" => Ok(LogFormat::Pretty),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    pub service_name: &'static str,
    pub crate_target: &'static str,
    pub log_level: String,
    pub log_format: LogFormat,
    pub debug: bool,
    pub unsafe_content: bool,
    pub otlp_enabled: bool,
    pub metrics_addr: Option<SocketAddr>,
}

static DEBUG: AtomicBool = AtomicBool::new(false);
static CONTENT: AtomicBool = AtomicBool::new(false);

pub fn debug_enabled() -> bool {
    DEBUG.load(Ordering::Relaxed)
}

pub fn content_logging_enabled() -> bool {
    CONTENT.load(Ordering::Relaxed)
}

pub fn build_filter(level: &str, debug: bool, crate_target: &str) -> String {
    if !debug {
        return level.to_string();
    }
    format!("{level},{crate_target}=debug,mcp_server_core=debug")
}

pub struct ObservabilityGuard {
    provider: Option<SdkTracerProvider>,
}

impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        if let Some(p) = self.provider.take() {
            let _ = p.shutdown();
        }
    }
}

const DURATION_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

pub fn init(cfg: &ObservabilityConfig) -> anyhow::Result<ObservabilityGuard> {
    let debug = cfg.debug || cfg.unsafe_content;
    DEBUG.store(debug, Ordering::Relaxed);
    CONTENT.store(cfg.unsafe_content, Ordering::Relaxed);

    let filter = EnvFilter::try_new(build_filter(&cfg.log_level, debug, cfg.crate_target))?;
    let fmt_layer = match cfg.log_format {
        LogFormat::Json => tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(true)
            .flatten_event(true)
            .boxed(),
        LogFormat::Pretty => tracing_subscriber::fmt::layer().pretty().boxed(),
    };
    let provider = if cfg.otlp_enabled {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .build()?;
        Some(
            SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(
                    Resource::builder()
                        .with_service_name(cfg.service_name)
                        .build(),
                )
                .build(),
        )
    } else {
        None
    };
    let otel_layer = provider
        .as_ref()
        .map(|p| tracing_opentelemetry::layer().with_tracer(p.tracer(cfg.service_name)));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(otel_layer)
        .try_init()?;
    if let Some(p) = &provider {
        opentelemetry::global::set_tracer_provider(p.clone());
    }

    if let Some(addr) = cfg.metrics_addr {
        metrics_exporter_prometheus::PrometheusBuilder::new()
            .with_http_listener(addr)
            .set_buckets(DURATION_BUCKETS)?
            .install()?;
        tracing::info!(%addr, "prometheus metrics listener started");
    }
    if cfg.unsafe_content {
        tracing::warn!(
            "DEBUG_UNSAFE_LOG_CONTENT is enabled: message content WILL be written to logs"
        );
    }
    Ok(ObservabilityGuard { provider })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_format_parses() {
        assert!(matches!("json".parse::<LogFormat>(), Ok(LogFormat::Json)));
        assert!(matches!(
            "Pretty".parse::<LogFormat>(),
            Ok(LogFormat::Pretty)
        ));
        assert!("xml".parse::<LogFormat>().is_err());
    }

    #[test]
    fn filter_without_debug_is_level_as_given() {
        assert_eq!(build_filter("info", false, "mcp_icloud_mail"), "info");
        assert_eq!(
            build_filter("warn,mcp_icloud_mail::imap=trace", false, "mcp_icloud_mail"),
            "warn,mcp_icloud_mail::imap=trace"
        );
    }

    #[test]
    fn debug_raises_own_crates() {
        assert_eq!(
            build_filter("info", true, "mcp_icloud_mail"),
            "info,mcp_icloud_mail=debug,mcp_server_core=debug"
        );
    }
}
