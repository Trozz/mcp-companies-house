use mcp_server_core::config::EnvReader;
use url::Url;

/// MCP `serverInfo.name` of the company data service.
pub const SERVICE_NAME: &str = "mcp-companies-house-data";
pub const DEFAULT_API_URL: &str = "https://api.company-information.service.gov.uk";
pub const SANDBOX_API_URL: &str = "https://api-sandbox.company-information.service.gov.uk";

/// Company data settings only: the server binary owns the core configuration (URL, OAuth, TLS,
/// logging). There is no API key: each person signs in with their own.
pub struct DataConfig {
    pub api_url: Url,
    /// `COMPANIES_HOUSE_DOCUMENT_API_URL`, when set. Unset, the live Document API is used with
    /// the live API, and no Document API with any other (the sandbox has none).
    pub document_api_url: Option<Url>,
}

impl DataConfig {
    pub fn from_env<F: Fn(&str) -> Option<String>>(env: &mut EnvReader<F>) -> Option<Self> {
        let errors_before = env.errors.len();
        let raw_url = env
            .optional("COMPANIES_HOUSE_API_URL")
            .unwrap_or_else(|| DEFAULT_API_URL.to_string());
        let api_url = match parse_url(API_URL_VAR, &raw_url) {
            Ok(u) => Some(u),
            Err(msg) => {
                env.errors.push(msg);
                None
            }
        };
        let document_api_url = match env.optional(DOCUMENT_API_URL_VAR) {
            Some(raw) => match parse_url(DOCUMENT_API_URL_VAR, &raw) {
                Ok(u) => Some(u),
                Err(msg) => {
                    env.errors.push(msg);
                    None
                }
            },
            None => None,
        };
        if env.errors.len() != errors_before {
            return None;
        }
        Some(DataConfig {
            api_url: api_url?,
            document_api_url,
        })
    }
}

const API_URL_VAR: &str = "COMPANIES_HOUSE_API_URL";
const DOCUMENT_API_URL_VAR: &str = "COMPANIES_HOUSE_DOCUMENT_API_URL";

#[cfg(test)]
fn parse_api_url(raw: &str) -> Result<Url, String> {
    parse_url(API_URL_VAR, raw)
}

/// An API origin from variable `name`: https, or http to a loopback address; no credentials,
/// query or fragment.
fn parse_url(name: &str, raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|e| format!("{name} is not a valid URL: {e}"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!("{name} must not contain credentials"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(format!("{name} must not contain a query or fragment"));
    }
    if url.scheme() == "https" && url.host_str().is_some() {
        return Ok(url);
    }
    if url.scheme() == "http" && is_loopback(&url) {
        return Ok(url);
    }
    Err(format!(
        "{name} must use https (http is only allowed for loopback addresses)"
    ))
}

pub(crate) fn is_loopback(url: &Url) -> bool {
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

    fn load(pairs: &[(&str, &str)]) -> (Option<DataConfig>, Vec<String>) {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        let mut env = EnvReader::new(move |k: &str| map.get(k).cloned());
        let c = DataConfig::from_env(&mut env);
        (c, env.errors)
    }

    #[test]
    fn defaults_to_the_live_api() {
        let (c, errors) = load(&[]);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            c.unwrap().api_url.as_str(),
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
    fn the_document_api_is_optional_and_checked_like_the_api() {
        let (c, errors) = load(&[]);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(c.unwrap().document_api_url.is_none());
        let (c, errors) = load(&[(
            "COMPANIES_HOUSE_DOCUMENT_API_URL",
            "http://127.0.0.1:9999/docs/",
        )]);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            c.unwrap().document_api_url.unwrap().as_str(),
            "http://127.0.0.1:9999/docs/"
        );
        let (c, errors) = load(&[("COMPANIES_HOUSE_DOCUMENT_API_URL", "http://example.com")]);
        assert!(c.is_none());
        assert!(
            errors
                .iter()
                .any(|e| e.contains("COMPANIES_HOUSE_DOCUMENT_API_URL must use https")),
            "{errors:?}"
        );
    }

    #[test]
    fn reads_no_server_variables() {
        let read = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = read.clone();
        let mut env = EnvReader::new(move |k: &str| {
            seen.lock().unwrap().push(k.to_string());
            None
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
