//! Thin client for the Companies House REST API: Basic auth with the API key as the username
//! (blank password), no redirects, timeouts, one retry for transport errors and 5xx, status
//! mapping, rate-limit tracking and metrics.

use crate::error::{ChError, Result};
use mcp_server_core::observability::content_logging_enabled;
use reqwest::StatusCode;
use reqwest::header::{HeaderMap, RETRY_AFTER};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const RETRY_DELAY: Duration = Duration::from_millis(250);
/// Largest response body read; Companies House responses are far smaller.
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_ERROR_DETAIL: usize = 300;

pub struct ApiSettings {
    /// `COMPANIES_HOUSE_API_URL`: every request goes to this origin and below this path.
    pub base: Url,
    pub api_key: SecretString,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub retry_delay: Duration,
    /// Always log response bodies at debug level. Without it, bodies are logged only while
    /// `DEBUG_UNSAFE_LOG_CONTENT` is on (read per request, because the service is built before
    /// logging is initialised).
    pub log_bodies: bool,
}

impl ApiSettings {
    pub fn new(base: Url, api_key: SecretString) -> Self {
        ApiSettings {
            base,
            api_key,
            connect_timeout: CONNECT_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
            retry_delay: RETRY_DELAY,
            log_bodies: false,
        }
    }
}

/// The rate-limit window Companies House reported on the latest response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub limit: Option<u64>,
    pub remaining: u64,
    /// Unix time (seconds) when the window resets.
    pub reset_unix: u64,
}

impl RateLimit {
    fn from_headers(headers: &HeaderMap) -> Option<Self> {
        let num = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
        };
        Some(RateLimit {
            limit: num("x-ratelimit-limit"),
            remaining: num("x-ratelimit-remain")?,
            reset_unix: num("x-ratelimit-reset")?,
        })
    }
}

pub struct ChClient {
    http: reqwest::Client,
    settings: ApiSettings,
    rate: Mutex<Option<RateLimit>>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn status_class(status: Option<StatusCode>) -> &'static str {
    match status.map(|s| s.as_u16()) {
        Some(200..=299) => "2xx",
        Some(300..=399) => "3xx",
        Some(400..=499) => "4xx",
        Some(500..=599) => "5xx",
        _ => "error",
    }
}

/// A short, single-line message from a Companies House error body, if it has one. Bodies come
/// as `{"errors":[{"error":"..."}]}`, `{"error":"..."}` or `{"message":"..."}`.
pub fn error_detail(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let mut parts: Vec<String> = Vec::new();
    if let Some(errors) = v.get("errors").and_then(Value::as_array) {
        for e in errors {
            if let Some(text) = e.get("error").and_then(Value::as_str) {
                let mut text = text.to_string();
                if let Some(values) = e.get("error_values").and_then(Value::as_object) {
                    for (k, val) in values {
                        if let Some(val) = val.as_str() {
                            text = text.replace(&format!("{{{k}}}"), val);
                        }
                    }
                }
                parts.push(text);
            }
        }
    }
    if parts.is_empty() {
        for key in ["error", "message"] {
            if let Some(text) = v.get(key).and_then(Value::as_str) {
                parts.push(text.to_string());
                break;
            }
        }
    }
    let joined = parts.join("; ");
    let joined = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        return None;
    }
    if joined.chars().count() > MAX_ERROR_DETAIL {
        let cut: String = joined.chars().take(MAX_ERROR_DETAIL).collect();
        return Some(format!("{cut}..."));
    }
    Some(joined)
}

fn map_status(path: &str, status: StatusCode, body: &str, reset_in_secs: Option<u64>) -> ChError {
    let detail = error_detail(body);
    let with_detail = |prefix: String| match &detail {
        Some(d) => format!("{prefix}: {d}"),
        None => prefix,
    };
    match status.as_u16() {
        401 => ChError::Auth,
        403 => ChError::Upstream(with_detail(format!(
            "GET {path} was refused (HTTP 403); check the API key's IP or domain restrictions in the Developer Hub"
        ))),
        404 => ChError::NotFound(path.to_string()),
        429 => ChError::RateLimited { reset_in_secs },
        400 | 416 | 422 => ChError::Validation(with_detail(format!(
            "Companies House rejected the request (HTTP {})",
            status.as_u16()
        ))),
        300..=399 => ChError::Upstream(format!(
            "GET {path} returned an unexpected redirect (HTTP {})",
            status.as_u16()
        )),
        code => ChError::Upstream(with_detail(format!("GET {path} returned HTTP {code}"))),
    }
}

impl ChClient {
    pub fn new(settings: ApiSettings) -> Result<Self> {
        mcp_server_core::install_crypto_provider();
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(settings.connect_timeout)
            .timeout(settings.request_timeout)
            .user_agent(format!("mcp-companies-house/{}", crate::VERSION))
            .build()
            .map_err(|e| ChError::Internal(format!("building the HTTP client failed: {e}")))?;
        Ok(ChClient {
            http,
            settings,
            rate: Mutex::new(None),
        })
    }

    pub fn base(&self) -> &Url {
        &self.settings.base
    }

    /// The rate-limit window from the latest response, if Companies House sent one.
    pub fn rate_limit(&self) -> Option<RateLimit> {
        *self.rate.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn log_bodies(&self) -> bool {
        self.settings.log_bodies || content_logging_enabled()
    }

    /// `base` with `segments` appended (each percent-encoded) and `query` added.
    pub fn url(&self, segments: &[&str], query: &[(&str, String)]) -> Url {
        let mut url = self.settings.base.clone();
        url.path_segments_mut()
            .expect("an http(s) base URL has path segments")
            .pop_if_empty()
            .extend(segments);
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        url
    }

    /// Seconds until the window resets when the latest response said none are left.
    fn exhausted_for(&self) -> Option<u64> {
        let rate = self.rate_limit()?;
        let now = now_unix();
        (rate.remaining == 0 && rate.reset_unix > now).then(|| rate.reset_unix - now)
    }

    fn record_rate(&self, headers: &HeaderMap, status: StatusCode) -> Option<u64> {
        let mut rate = RateLimit::from_headers(headers);
        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = headers
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            let reset_unix = rate
                .map(|r| r.reset_unix)
                .or_else(|| retry_after.map(|s| now_unix() + s));
            rate = reset_unix.map(|reset_unix| RateLimit {
                limit: rate.and_then(|r| r.limit),
                remaining: 0,
                reset_unix,
            });
        }
        let rate = rate?;
        metrics::gauge!("companies_house_ratelimit_remaining").set(rate.remaining as f64);
        *self.rate.lock().unwrap_or_else(|e| e.into_inner()) = Some(rate);
        let now = now_unix();
        Some(rate.reset_unix.saturating_sub(now))
    }

    /// GET a JSON resource. `endpoint` is a fixed label for metrics and logs; `segments` and
    /// `query` are appended to the base URL. A 2xx response with an empty body is `Value::Null`.
    pub async fn get(
        &self,
        endpoint: &'static str,
        segments: &[&str],
        query: &[(&str, String)],
    ) -> Result<Value> {
        let url = self.url(segments, query);
        let path = url.path().to_string();
        if let Some(reset_in_secs) = self.exhausted_for() {
            metrics::counter!("companies_house_rate_limited_total", "source" => "local")
                .increment(1);
            tracing::warn!(
                endpoint,
                reset_in_secs,
                "rate limit used up; not calling Companies House"
            );
            return Err(ChError::RateLimited {
                reset_in_secs: Some(reset_in_secs),
            });
        }
        let mut retried = false;
        loop {
            let started = Instant::now();
            let result = self
                .http
                .get(url.clone())
                .basic_auth(self.settings.api_key.expose_secret(), Some(""))
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await;
            let status = result.as_ref().ok().map(|r| r.status());
            metrics::counter!("companies_house_requests_total", "endpoint" => endpoint, "status_class" => status_class(status))
                .increment(1);
            metrics::histogram!("companies_house_request_duration_seconds", "endpoint" => endpoint)
                .record(started.elapsed().as_secs_f64());
            tracing::debug!(
                endpoint,
                path = %path,
                status = status.map(|s| s.as_u16()),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "companies house request"
            );
            let response = match result {
                Ok(r) => r,
                Err(e) if !retried => {
                    tracing::debug!(endpoint, error = %describe(e), "retrying");
                    retried = true;
                    tokio::time::sleep(self.settings.retry_delay).await;
                    continue;
                }
                Err(e) => {
                    let what = if e.is_timeout() {
                        "timed out"
                    } else {
                        "failed"
                    };
                    return Err(ChError::Upstream(format!(
                        "GET {path} {what}: {}",
                        describe(e)
                    )));
                }
            };
            let status = response.status();
            if status.is_server_error() && !retried {
                tracing::debug!(endpoint, status = status.as_u16(), "retrying");
                retried = true;
                tokio::time::sleep(self.settings.retry_delay).await;
                continue;
            }
            let reset_in_secs = self.record_rate(response.headers(), status);
            let body = read_body(response, &path).await?;
            if self.log_bodies() {
                tracing::debug!(endpoint, path = %path, status = status.as_u16(), body = %body, "companies house response");
            }
            if !status.is_success() {
                if status == StatusCode::TOO_MANY_REQUESTS {
                    metrics::counter!("companies_house_rate_limited_total", "source" => "upstream")
                        .increment(1);
                }
                return Err(map_status(&path, status, &body, reset_in_secs));
            }
            if body.trim().is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&body)
                .map_err(|e| ChError::Upstream(format!("GET {path} returned invalid JSON: {e}")));
        }
    }
}

/// A reqwest error with its causes, without the URL: the URL carries the caller's search terms.
fn describe(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut msg = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        msg = format!("{msg}: {s}");
        source = s.source();
    }
    msg
}

async fn read_body(mut response: reqwest::Response, path: &str) -> Result<String> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if buf.len() + chunk.len() > MAX_BODY_BYTES {
                    return Err(ChError::Upstream(format!(
                        "GET {path} returned more than {MAX_BODY_BYTES} bytes"
                    )));
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => {
                return Err(ChError::Upstream(format!(
                    "reading GET {path} failed: {}",
                    describe(e)
                )));
            }
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    fn client(base: &str) -> ChClient {
        ChClient::new(ApiSettings::new(
            Url::parse(base).unwrap(),
            SecretString::from("test-api-key"),
        ))
        .unwrap()
    }

    #[test]
    fn urls_append_encoded_segments_and_query() {
        let c = client("https://api.company-information.service.gov.uk");
        assert_eq!(
            c.url(&["company", "00445790"], &[]).as_str(),
            "https://api.company-information.service.gov.uk/company/00445790"
        );
        assert_eq!(
            c.url(
                &["search", "companies"],
                &[
                    ("q", "marks & spencer".into()),
                    ("items_per_page", "5".into())
                ]
            )
            .as_str(),
            "https://api.company-information.service.gov.uk/search/companies?q=marks+%26+spencer&items_per_page=5"
        );
        assert_eq!(
            c.url(&["company", "../x?y"], &[]).path(),
            "/company/..%2Fx%3Fy"
        );
    }

    #[test]
    fn urls_keep_a_base_path() {
        let c = client("https://proxy.example.com/ch/");
        assert_eq!(
            c.url(&["company", "SC123456"], &[]).as_str(),
            "https://proxy.example.com/ch/company/SC123456"
        );
        let c = client("http://127.0.0.1:9000/ch");
        assert_eq!(
            c.url(&["search", "companies"], &[]).as_str(),
            "http://127.0.0.1:9000/ch/search/companies"
        );
    }

    #[test]
    fn statuses_map_to_the_spec_error_kinds() {
        let kind = |code: u16| {
            map_status(
                "/company/1",
                StatusCode::from_u16(code).unwrap(),
                "",
                Some(5),
            )
            .kind()
        };
        assert_eq!(kind(401), "auth");
        assert_eq!(kind(403), "upstream");
        assert_eq!(kind(404), "not_found");
        assert_eq!(kind(429), "rate_limited");
        assert_eq!(kind(400), "validation");
        assert_eq!(kind(416), "validation");
        assert_eq!(kind(302), "upstream");
        assert_eq!(kind(500), "upstream");
        assert_eq!(kind(503), "upstream");
    }

    #[test]
    fn validation_errors_carry_the_upstream_detail() {
        let body = r#"{"errors":[{"error":"Invalid value {value} for company_status","error_values":{"value":"bogus"},"type":"ch:validation"}]}"#;
        let e = map_status(
            "/advanced-search/companies",
            StatusCode::BAD_REQUEST,
            body,
            None,
        );
        assert_eq!(e.kind(), "validation");
        assert!(
            e.to_string()
                .contains("Invalid value bogus for company_status"),
            "{e}"
        );
    }

    #[test]
    fn error_detail_reads_every_body_shape() {
        assert_eq!(
            error_detail(r#"{"error":"Invalid Authorization","type":"ch:service"}"#).as_deref(),
            Some("Invalid Authorization")
        );
        assert_eq!(
            error_detail(r#"{"message":"404 NOT_FOUND \"x\"","request_id":"r"}"#).as_deref(),
            Some("404 NOT_FOUND \"x\"")
        );
        assert_eq!(
            error_detail(r#"{"errors":[{"error":"a"},{"error":"b"}]}"#).as_deref(),
            Some("a; b")
        );
        assert_eq!(error_detail("{}"), None);
        assert_eq!(error_detail("<html>"), None);
        let long = format!(r#"{{"error":"{}"}}"#, "x".repeat(1000));
        assert_eq!(
            error_detail(&long).unwrap().chars().count(),
            MAX_ERROR_DETAIL + 3
        );
    }

    #[test]
    fn rate_limit_headers_are_parsed() {
        let mut h = HeaderMap::new();
        h.insert("x-ratelimit-limit", HeaderValue::from_static("600"));
        h.insert("x-ratelimit-remain", HeaderValue::from_static("599"));
        h.insert("x-ratelimit-reset", HeaderValue::from_static("1792279244"));
        h.insert("x-ratelimit-window", HeaderValue::from_static("5m"));
        assert_eq!(
            RateLimit::from_headers(&h),
            Some(RateLimit {
                limit: Some(600),
                remaining: 599,
                reset_unix: 1_792_279_244
            })
        );
        assert_eq!(RateLimit::from_headers(&HeaderMap::new()), None);
    }

    #[test]
    fn a_429_without_rate_headers_uses_retry_after() {
        let c = client("https://api.company-information.service.gov.uk");
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, HeaderValue::from_static("30"));
        let reset = c.record_rate(&h, StatusCode::TOO_MANY_REQUESTS);
        assert!(matches!(reset, Some(29..=30)), "{reset:?}");
        assert_eq!(c.rate_limit().unwrap().remaining, 0);
        assert!(c.exhausted_for().is_some());
    }

    #[test]
    fn an_exhausted_window_in_the_past_does_not_block() {
        let c = client("https://api.company-information.service.gov.uk");
        *c.rate.lock().unwrap() = Some(RateLimit {
            limit: Some(600),
            remaining: 0,
            reset_unix: now_unix() - 1,
        });
        assert_eq!(c.exhausted_for(), None);
        *c.rate.lock().unwrap() = Some(RateLimit {
            limit: Some(600),
            remaining: 0,
            reset_unix: now_unix() + 100,
        });
        assert!(matches!(c.exhausted_for(), Some(99..=100)));
    }

    #[test]
    fn status_classes() {
        assert_eq!(status_class(Some(StatusCode::OK)), "2xx");
        assert_eq!(status_class(Some(StatusCode::FOUND)), "3xx");
        assert_eq!(status_class(Some(StatusCode::NOT_FOUND)), "4xx");
        assert_eq!(status_class(Some(StatusCode::BAD_GATEWAY)), "5xx");
        assert_eq!(status_class(None), "error");
    }
}
