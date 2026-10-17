use mcp_server_core::config::EnvReader;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

/// MCP `serverInfo.name` of the company data service.
pub const SERVICE_NAME: &str = "mcp-companies-house-data";
pub const DEFAULT_API_URL: &str = "https://api.company-information.service.gov.uk";
pub const SANDBOX_API_URL: &str = "https://api-sandbox.company-information.service.gov.uk";

/// Company data settings only: the server binary owns the core configuration (URL, OAuth, TLS,
/// logging).
pub struct DataConfig {
    pub api_key: SecretString,
    pub api_url: Url,
}

impl DataConfig {
    pub fn from_env<F: Fn(&str) -> Option<String>>(env: &mut EnvReader<F>) -> Option<Self> {
        let errors_before = env.errors.len();
        let api_key = env.secret_required("COMPANIES_HOUSE_API_KEY");
        if let Some(k) = &api_key {
            let k = k.expose_secret();
            if k.contains(':') || k.chars().any(char::is_whitespace) {
                env.errors.push(
                    "COMPANIES_HOUSE_API_KEY must not contain spaces or a colon; paste the key exactly as the Developer Hub shows it"
                        .into(),
                );
            }
        }
        let raw_url = env
            .optional("COMPANIES_HOUSE_API_URL")
            .unwrap_or_else(|| DEFAULT_API_URL.to_string());
        let api_url = match parse_api_url(&raw_url) {
            Ok(u) => Some(u),
            Err(msg) => {
                env.errors.push(msg);
                None
            }
        };
        if env.errors.len() != errors_before {
            return None;
        }
        Some(DataConfig {
            api_key: api_key?,
            api_url: api_url?,
        })
    }
}

fn parse_api_url(raw: &str) -> Result<Url, String> {
    let url =
        Url::parse(raw).map_err(|e| format!("COMPANIES_HOUSE_API_URL is not a valid URL: {e}"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("COMPANIES_HOUSE_API_URL must not contain credentials".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("COMPANIES_HOUSE_API_URL must not contain a query or fragment".into());
    }
    if url.scheme() == "https" && url.host_str().is_some() {
        return Ok(url);
    }
    if url.scheme() == "http" && is_loopback(&url) {
        return Ok(url);
    }
    Err(
        "COMPANIES_HOUSE_API_URL must use https (http is only allowed for loopback addresses)"
            .into(),
    )
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const KEY: &str = "test-api-key";

    fn load(extra: &[(&str, &str)]) -> (Option<DataConfig>, Vec<String>) {
        let mut map: HashMap<String, String> = [("COMPANIES_HOUSE_API_KEY", KEY)]
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        for (k, v) in extra {
            map.insert(k.to_string(), v.to_string());
        }
        let mut env = EnvReader::new(move |k: &str| map.get(k).cloned());
        let c = DataConfig::from_env(&mut env);
        (c, env.errors)
    }

    #[test]
    fn defaults_to_the_live_api() {
        let (c, errors) = load(&[]);
        assert!(errors.is_empty(), "{errors:?}");
        let c = c.unwrap();
        assert_eq!(c.api_key.expose_secret(), KEY);
        assert_eq!(
            c.api_url.as_str(),
            "https://api.company-information.service.gov.uk/"
        );
    }

    #[test]
    fn sandbox_and_loopback_urls_are_accepted() {
        let (c, errors) = load(&[("COMPANIES_HOUSE_API_URL", SANDBOX_API_URL)]);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            c.unwrap().api_url.host_str(),
            Some("api-sandbox.company-information.service.gov.uk")
        );
        for ok in [
            "http://127.0.0.1:9999",
            "http://localhost:8080/",
            "http://[::1]:8080",
            "https://proxy.example.com/companies-house/",
        ] {
            assert!(parse_api_url(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn api_url_must_be_https_unless_loopback() {
        for bad in [
            "http://api.company-information.service.gov.uk",
            "ftp://x",
            "not a url",
            "http://10.0.0.1",
            "https://user:pw@api.company-information.service.gov.uk",
            "https://api.company-information.service.gov.uk/?x=1",
        ] {
            assert!(parse_api_url(bad).is_err(), "{bad}");
        }
        let (c, errors) = load(&[("COMPANIES_HOUSE_API_URL", "http://example.com")]);
        assert!(c.is_none());
        assert!(
            errors.iter().any(|e| e.contains("COMPANIES_HOUSE_API_URL")),
            "{errors:?}"
        );
    }

    #[test]
    fn missing_key_is_reported() {
        let mut env = EnvReader::new(|_: &str| None);
        assert!(DataConfig::from_env(&mut env).is_none());
        assert_eq!(
            env.errors,
            vec!["COMPANIES_HOUSE_API_KEY is required".to_string()]
        );
    }

    #[test]
    fn malformed_key_is_rejected_without_echoing_it() {
        let (c, errors) = load(&[("COMPANIES_HOUSE_API_KEY", "abc:def secret")]);
        assert!(c.is_none());
        let joined = errors.join("\n");
        assert!(joined.contains("COMPANIES_HOUSE_API_KEY"), "{joined}");
        assert!(!joined.contains("abc:def"), "secret leaked: {joined}");
    }

    #[test]
    fn errors_are_reported_together() {
        let (c, errors) = load(&[
            ("COMPANIES_HOUSE_API_KEY", "has space"),
            ("COMPANIES_HOUSE_API_URL", "ftp://x"),
        ]);
        assert!(c.is_none());
        assert_eq!(errors.len(), 2, "{errors:?}");
    }

    #[test]
    fn reads_no_server_variables() {
        let read = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = read.clone();
        let mut env = EnvReader::new(move |k: &str| {
            seen.lock().unwrap().push(k.to_string());
            (k == "COMPANIES_HOUSE_API_KEY").then(|| KEY.to_string())
        });
        assert!(DataConfig::from_env(&mut env).is_some(), "{:?}", env.errors);
        let read = read.lock().unwrap();
        for k in read.iter() {
            assert!(
                k.starts_with("COMPANIES_HOUSE_"),
                "company data config read a server variable: {k}"
            );
        }
        assert!(
            !read.iter().any(|k| k == "COMPANIES_HOUSE_SERVICES"),
            "{read:?}"
        );
    }
}
