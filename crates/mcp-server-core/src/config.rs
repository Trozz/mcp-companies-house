use crate::oauth::OAuthSettings;
use crate::observability::{LogFormat, ObservabilityConfig};
use crate::tls::TlsMode;
use secrecy::SecretString;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;

pub struct EnvReader<F: Fn(&str) -> Option<String>> {
    get: F,
    pub errors: Vec<String>,
}

impl EnvReader<fn(&str) -> Option<String>> {
    pub fn from_process() -> Self {
        EnvReader::new(|k: &str| std::env::var(k).ok())
    }
}

impl<F: Fn(&str) -> Option<String>> EnvReader<F> {
    pub fn new(get: F) -> Self {
        Self {
            get,
            errors: Vec::new(),
        }
    }

    fn raw(&self, name: &str) -> Option<String> {
        (self.get)(name).filter(|v| !v.is_empty())
    }

    pub fn optional(&mut self, name: &str) -> Option<String> {
        if let Some(v) = self.raw(name) {
            return Some(v);
        }
        let file_var = format!("{name}_FILE");
        let path = self.raw(&file_var)?;
        match std::fs::read_to_string(&path) {
            Ok(s) => Some(s.trim_end_matches(['\r', '\n']).to_string()).filter(|v| !v.is_empty()),
            Err(e) => {
                self.errors
                    .push(format!("{file_var}: cannot read {path}: {e}"));
                None
            }
        }
    }

    pub fn required(&mut self, name: &str) -> Option<String> {
        let errors_before = self.errors.len();
        let value = self.optional(name);
        if value.is_none() && self.errors.len() == errors_before {
            self.errors.push(format!("{name} is required"));
        }
        value
    }

    pub fn secret_optional(&mut self, name: &str) -> Option<SecretString> {
        self.optional(name).map(SecretString::from)
    }

    pub fn secret_required(&mut self, name: &str) -> Option<SecretString> {
        self.required(name).map(SecretString::from)
    }

    pub fn bool(&mut self, name: &str, default: bool) -> bool {
        let Some(v) = self.optional(name) else {
            return default;
        };
        match v.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => {
                self.errors
                    .push(format!("{name} must be true or false, got \"{v}\""));
                default
            }
        }
    }

    pub fn parse<T: FromStr>(&mut self, name: &str, default: T) -> T {
        self.parse_optional(name).unwrap_or(default)
    }

    pub fn parse_optional<T: FromStr>(&mut self, name: &str) -> Option<T> {
        let v = self.optional(name)?;
        match v.parse() {
            Ok(t) => Some(t),
            Err(_) => {
                self.errors.push(format!("{name} has an invalid value"));
                None
            }
        }
    }

    pub fn list(&mut self, name: &str) -> Vec<String> {
        self.optional(name)
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }
}

pub struct CoreConfig {
    pub public_url: String,
    pub public_host: String,
    pub oauth: OAuthSettings,
    pub allowed_origins: Vec<String>,
    pub bind_addr: SocketAddr,
    pub tls: TlsMode,
    pub observability: ObservabilityConfig,
}

fn parse_public_url(raw: &str) -> Result<(String, String), String> {
    let url = url::Url::parse(raw).map_err(|_| "PUBLIC_URL must be an https URL".to_string())?;
    let host = url
        .host_str()
        .ok_or_else(|| "PUBLIC_URL must be an https URL".to_string())?
        .to_string();
    let loopback = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]");
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("PUBLIC_URL must be an https URL (http is only allowed for localhost)".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("PUBLIC_URL must not contain a query or fragment".into());
    }
    if url.path() != "/" {
        return Err("PUBLIC_URL must not contain a path".into());
    }
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    Ok((url.as_str().trim_end_matches('/').to_string(), host))
}

impl CoreConfig {
    pub fn from_env<F: Fn(&str) -> Option<String>>(
        env: &mut EnvReader<F>,
        service_name: &'static str,
        crate_target: &'static str,
    ) -> Option<Self> {
        let errors_before = env.errors.len();

        let public = env
            .required("PUBLIC_URL")
            .and_then(|raw| match parse_public_url(&raw) {
                Ok(v) => Some(v),
                Err(e) => {
                    env.errors.push(e);
                    None
                }
            });
        let signing_key = env.secret_required("OAUTH_SIGNING_KEY");
        if let Some(k) = &signing_key {
            use secrecy::ExposeSecret;
            if k.expose_secret().len() < 32 {
                env.errors
                    .push("OAUTH_SIGNING_KEY must be at least 32 bytes".into());
            }
        }
        let mut trusted_proxies = Vec::new();
        for raw in env.list("TRUSTED_PROXIES") {
            match raw.parse::<IpAddr>() {
                Ok(ip) => trusted_proxies.push(ip),
                Err(_) => env.errors.push(format!(
                    "TRUSTED_PROXIES contains an invalid IP address: {raw}"
                )),
            }
        }
        let allowed_origins = env.list("ALLOWED_ORIGINS");
        let bind_addr = env.parse("BIND_ADDR", SocketAddr::from(([0, 0, 0, 0], 8443)));
        let insecure = env.bool("INSECURE_HTTP", false);
        let cert = env.optional("TLS_CERT_PATH").map(PathBuf::from);
        let key = env.optional("TLS_KEY_PATH").map(PathBuf::from);
        let log_level = env.optional("LOG_LEVEL").unwrap_or_else(|| "info".into());
        let log_format = env.parse("LOG_FORMAT", LogFormat::Json);
        let debug = env.bool("DEBUG", false);
        let unsafe_content = env.bool("DEBUG_UNSAFE_LOG_CONTENT", false);
        let metrics_addr = env.parse_optional::<SocketAddr>("METRICS_ADDR");
        let otlp_enabled = env.optional("OTEL_EXPORTER_OTLP_ENDPOINT").is_some();

        let (public_url, public_host) = public.unwrap_or_default();
        let tls = match TlsMode::select(insecure, cert, key, &public_host) {
            Ok(t) => Some(t),
            Err(e) => {
                env.errors.push(e);
                None
            }
        };
        if env.errors.len() != errors_before {
            return None;
        }
        Some(CoreConfig {
            oauth: OAuthSettings {
                public_url: public_url.clone(),
                signing_key: signing_key?,
                trusted_proxies,
            },
            public_url,
            public_host,
            allowed_origins,
            bind_addr,
            tls: tls?,
            observability: ObservabilityConfig {
                service_name,
                crate_target,
                log_level,
                log_format,
                debug,
                unsafe_content,
                otlp_enabled,
                metrics_addr,
            },
        })
    }

    /// External URL of a service mounted at `/<service>`.
    pub fn service_url(&self, service: &str) -> String {
        format!("{}/{service}", self.public_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::collections::HashMap;

    fn reader(pairs: &[(&str, &str)]) -> EnvReader<impl Fn(&str) -> Option<String> + use<>> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        EnvReader::new(move |k: &str| map.get(k).cloned())
    }

    #[test]
    fn required_reports_missing() {
        let mut env = reader(&[]);
        assert_eq!(env.required("FOO"), None);
        assert_eq!(env.errors, vec!["FOO is required".to_string()]);
    }

    #[test]
    fn empty_values_count_as_unset() {
        let mut env = reader(&[("FOO", "")]);
        assert_eq!(env.optional("FOO"), None);
    }

    #[test]
    fn file_variant_is_read_and_trailing_newline_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");
        std::fs::write(&path, "s3cret\n").unwrap();
        let p = path.to_str().unwrap().to_string();
        let mut env = reader(&[("PASS_FILE", p.as_str())]);
        assert_eq!(
            env.secret_required("PASS").unwrap().expose_secret(),
            "s3cret"
        );
        assert!(env.errors.is_empty());
    }

    #[test]
    fn direct_value_wins_over_file() {
        let mut env = reader(&[("PASS", "direct"), ("PASS_FILE", "/nonexistent")]);
        assert_eq!(env.required("PASS").as_deref(), Some("direct"));
    }

    #[test]
    fn unreadable_file_is_an_error_without_leaking_contents() {
        let mut env = reader(&[("PASS_FILE", "/definitely/not/here")]);
        assert_eq!(env.required("PASS"), None);
        assert_eq!(env.errors.len(), 1);
        assert!(env.errors[0].starts_with("PASS_FILE: cannot read /definitely/not/here"));
    }

    #[test]
    fn bool_parsing() {
        let mut env = reader(&[("A", "true"), ("B", "0"), ("C", "yes"), ("D", "maybe")]);
        assert!(env.bool("A", false));
        assert!(!env.bool("B", true));
        assert!(env.bool("C", false));
        assert!(env.bool("MISSING", true));
        assert!(!env.bool("D", false));
        assert_eq!(
            env.errors,
            vec!["D must be true or false, got \"maybe\"".to_string()]
        );
    }

    #[test]
    fn parse_with_default_and_error() {
        let mut env = reader(&[("N", "42"), ("BAD", "x")]);
        assert_eq!(env.parse("N", 1u32), 42);
        assert_eq!(env.parse("MISSING", 7u32), 7);
        assert_eq!(env.parse("BAD", 3u32), 3);
        assert_eq!(env.errors, vec!["BAD has an invalid value".to_string()]);
    }

    #[test]
    fn list_splits_and_trims() {
        let mut env = reader(&[("L", " a, b ,,c ")]);
        assert_eq!(env.list("L"), vec!["a", "b", "c"]);
    }

    fn core_env(extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = [
            ("PUBLIC_URL", "https://mail.example.com/"),
            ("OAUTH_SIGNING_KEY", "0123456789abcdef0123456789abcdef"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        v.extend(extra.iter().map(|(a, b)| (a.to_string(), b.to_string())));
        v
    }

    fn core(pairs: Vec<(String, String)>) -> (Option<CoreConfig>, Vec<String>) {
        let map: HashMap<String, String> = pairs.into_iter().collect();
        let mut env = EnvReader::new(move |k: &str| map.get(k).cloned());
        let c = CoreConfig::from_env(&mut env, "mcp-icloud-mail", "mcp_icloud_mail");
        (c, env.errors)
    }

    #[test]
    fn core_config_defaults() {
        let (c, errors) = core(core_env(&[]));
        assert!(errors.is_empty(), "{errors:?}");
        let c = c.unwrap();
        assert_eq!(c.public_url, "https://mail.example.com");
        assert_eq!(c.public_host, "mail.example.com");
        assert_eq!(c.service_url("mail"), "https://mail.example.com/mail");
        assert_eq!(c.bind_addr.to_string(), "0.0.0.0:8443");
        assert!(matches!(c.tls, crate::tls::TlsMode::SelfSigned { .. }));
        assert!(matches!(
            c.observability.log_format,
            crate::observability::LogFormat::Json
        ));
        assert!(!c.observability.otlp_enabled);
    }

    #[test]
    fn core_config_collects_all_errors() {
        let (c, errors) = core(vec![
            ("PUBLIC_URL".into(), "ftp://x".into()),
            ("OAUTH_SIGNING_KEY".into(), "short".into()),
            ("TLS_CERT_PATH".into(), "/c".into()),
            ("LOG_FORMAT".into(), "xml".into()),
        ]);
        assert!(c.is_none());
        let joined = errors.join("\n");
        assert!(
            joined.contains("PUBLIC_URL must be an https URL"),
            "{joined}"
        );
        assert!(
            joined.contains("OAUTH_SIGNING_KEY must be at least 32 bytes"),
            "{joined}"
        );
        assert!(
            joined.contains("TLS_CERT_PATH and TLS_KEY_PATH must be set together"),
            "{joined}"
        );
        assert!(
            joined.contains("LOG_FORMAT has an invalid value"),
            "{joined}"
        );
        assert!(!joined.contains("short"), "secret value leaked: {joined}");
    }

    #[test]
    fn http_public_url_allowed_only_for_localhost() {
        let (c, errors) = core(core_env(&[("PUBLIC_URL", "http://localhost:8443")]));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(c.unwrap().public_url, "http://localhost:8443");
        let (_, errors) = core(core_env(&[("PUBLIC_URL", "http://mail.example.com")]));
        assert!(!errors.is_empty());
    }

    #[test]
    fn trusted_proxies_default_empty_and_parse() {
        let (c, _) = core(core_env(&[]));
        assert!(c.unwrap().oauth.trusted_proxies.is_empty());
        let (c, errors) = core(core_env(&[("TRUSTED_PROXIES", "10.0.0.1, ::1")]));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            c.unwrap().oauth.trusted_proxies,
            vec![
                "10.0.0.1".parse::<std::net::IpAddr>().unwrap(),
                "::1".parse().unwrap()
            ]
        );
        let (c, errors) = core(core_env(&[("TRUSTED_PROXIES", "10.0.0.1,nope")]));
        assert!(c.is_none());
        assert!(errors.join("\n").contains("TRUSTED_PROXIES"), "{errors:?}");
    }

    #[test]
    fn public_url_with_path_is_rejected() {
        let (c, errors) = core(core_env(&[("PUBLIC_URL", "https://mail.example.com/sub")]));
        assert!(c.is_none());
        assert!(
            errors
                .join("\n")
                .contains("PUBLIC_URL must not contain a path"),
            "{errors:?}"
        );
    }

    #[test]
    fn otlp_enabled_when_endpoint_set() {
        let (c, _) = core(core_env(&[(
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "http://collector:4318",
        )]));
        assert!(c.unwrap().observability.otlp_enabled);
    }
}
