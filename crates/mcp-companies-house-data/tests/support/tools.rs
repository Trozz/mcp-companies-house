use super::fake_ch::{API_KEY, FakeCh};
use mcp_companies_house_data::api::{ApiSettings, ChClient};
use mcp_companies_house_data::tools::CompaniesServer;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use secrecy::SecretString;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;

/// Client settings for the fake: no delay between a failure and its retry.
pub fn settings(fake: &FakeCh, key: &str) -> ApiSettings {
    let mut s = ApiSettings::new(fake.url.clone(), SecretString::from(key.to_string()));
    s.retry_delay = Duration::ZERO;
    s
}

pub fn client(fake: &FakeCh) -> Arc<ChClient> {
    Arc::new(ChClient::new(settings(fake, API_KEY)).unwrap())
}

pub fn server(fake: &FakeCh) -> CompaniesServer {
    CompaniesServer::new(client(fake))
}

pub fn server_with_key(fake: &FakeCh, key: &str) -> CompaniesServer {
    CompaniesServer::new(Arc::new(ChClient::new(settings(fake, key)).unwrap()))
}

pub fn params<T: DeserializeOwned>(v: serde_json::Value) -> Parameters<T> {
    Parameters(serde_json::from_value(v).expect("valid tool arguments"))
}

pub fn is_error(r: &CallToolResult) -> bool {
    r.is_error == Some(true)
}

pub fn text(r: &CallToolResult) -> String {
    r.content[0]
        .as_text()
        .expect("first content block is text")
        .text
        .clone()
}

/// Parses the JSON text of a successful tool result, failing the test on a tool error.
pub fn json(r: &CallToolResult) -> serde_json::Value {
    assert!(!is_error(r), "tool error: {}", text(r));
    serde_json::from_str(&text(r)).expect("tool result is JSON")
}

/// The error text of a failed tool call, failing the test when the call succeeded.
pub fn error_text(r: &CallToolResult) -> String {
    assert!(is_error(r), "expected a tool error, got: {}", text(r));
    text(r)
}
