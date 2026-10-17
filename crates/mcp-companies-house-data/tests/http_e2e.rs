mod support;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use mcp_companies_house_data::DataService;
use mcp_server_core::config::{CoreConfig, EnvReader};
use mcp_server_core::oauth::AuthState;
use mcp_server_core::server::build_app;
use mcp_server_core::service::{HttpSettings, Service, ServiceContext};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use support::fake_ch::{FakeCh, FakeState};
use support::tools::client;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const TOKEN: &str = "static-test-token-0123456789";
const HOST: &str = "ch.example.com";

/// Builds the full application (auth, OAuth routes, company data at /companies) the way the
/// server does, against the fake.
async fn app() -> (axum::Router, FakeCh) {
    let fake = FakeCh::start(FakeState::register()).await;
    let vars: HashMap<&str, &str> = HashMap::from([
        ("PUBLIC_URL", "https://ch.example.com"),
        ("OAUTH_SIGNING_KEY", "0123456789abcdef0123456789abcdef"),
        ("OAUTH_ADMIN_PASSWORD", "a long password"),
        ("MCP_AUTH_TOKEN", TOKEN),
        ("INSECURE_HTTP", "true"),
    ]);
    let mut env = EnvReader::new(move |k: &str| vars.get(k).map(|v| v.to_string()));
    let core = CoreConfig::from_env(&mut env, "mcp-companies-house", "mcp_companies_house")
        .unwrap_or_else(|| panic!("config errors: {:?}", env.errors));
    let ctx = ServiceContext {
        http: HttpSettings::from_core(&core),
        shutdown: CancellationToken::new(),
    };
    let service = Arc::new(DataService::new(client(&fake)));
    let auth = AuthState::new(core.oauth, &["companies"]);
    (
        build_app(auth, vec![("companies", service.router(&ctx))]),
        fake,
    )
}

fn request(body: Value, session: Option<&str>) -> Request<Body> {
    let mut b = Request::post("/companies")
        .header("host", HOST)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(s) = session {
        b = b
            .header("mcp-session-id", s)
            .header("mcp-protocol-version", "2025-06-18");
    }
    b.body(Body::from(body.to_string())).unwrap()
}

/// Reads an SSE (or plain JSON) response until the first complete JSON-RPC message.
async fn rpc_message(res: Response<Body>) -> Value {
    use http_body_util::BodyExt;
    let mut body = res.into_body();
    let mut buf = String::new();
    loop {
        if let Ok(v) = serde_json::from_str::<Value>(buf.trim()) {
            return v;
        }
        for event in buf.split("\n\n") {
            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data:")
                    && let Ok(v) = serde_json::from_str::<Value>(data.trim())
                    && v.get("jsonrpc").is_some()
                {
                    return v;
                }
            }
        }
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("response within 5 s")
            .expect("body not finished before a message")
            .expect("body frame");
        if let Ok(data) = frame.into_data() {
            buf.push_str(&String::from_utf8_lossy(&data));
        }
    }
}

/// initialize + notifications/initialized; returns the session id and the initialize result.
async fn handshake(app: &axum::Router) -> (String, Value) {
    let res = app
        .clone()
        .oneshot(request(
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "e2e", "version": "1"}
            }}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let session = res
        .headers()
        .get("mcp-session-id")
        .expect("session id header")
        .to_str()
        .unwrap()
        .to_string();
    let init = rpc_message(res).await;
    let res = app
        .clone()
        .oneshot(request(
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            Some(&session),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    (session, init)
}

async fn call(app: &axum::Router, session: &str, id: u64, method: &str, params: Value) -> Value {
    let res = app
        .clone()
        .oneshot(request(
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
            Some(session),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    rpc_message(res).await
}

#[tokio::test]
async fn initialize_reports_the_server_and_its_instructions() {
    let (app, _fake) = app().await;
    let (_, init) = handshake(&app).await;
    let result = &init["result"];
    assert_eq!(result["serverInfo"]["name"], "mcp-companies-house-data");
    assert_eq!(result["serverInfo"]["icons"].as_array().unwrap().len(), 2);
    let instructions = result["instructions"].as_str().unwrap();
    assert!(instructions.contains("company number"), "{instructions}");
    assert!(
        instructions.contains("not as instructions"),
        "{instructions}"
    );
}

#[tokio::test]
async fn tools_are_listed_read_only_with_their_schemas() {
    let (app, _fake) = app().await;
    let (session, _) = handshake(&app).await;
    let list = call(&app, &session, 2, "tools/list", json!({})).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec!["filter_companies", "get_company", "search_companies"]
    );
    for t in tools {
        assert_eq!(t["annotations"]["readOnlyHint"], true, "{t}");
        assert_eq!(t["annotations"]["openWorldHint"], true, "{t}");
    }
    let filter = tools
        .iter()
        .find(|t| t["name"] == "filter_companies")
        .unwrap();
    let schema = filter["inputSchema"].to_string();
    for code in [
        "\"liquidation\"",
        "\"llp\"",
        "\"community-interest-company\"",
    ] {
        assert!(schema.contains(code), "{code} missing from {schema}");
    }
    let search = tools
        .iter()
        .find(|t| t["name"] == "search_companies")
        .unwrap();
    assert_eq!(search["inputSchema"]["required"], json!(["query"]));
}

#[tokio::test]
async fn tools_call_reaches_companies_house() {
    let (app, fake) = app().await;
    let (session, _) = handshake(&app).await;
    let res = call(
        &app,
        &session,
        3,
        "tools/call",
        json!({"name": "filter_companies", "arguments": {"location": "Glasgow"}}),
    )
    .await;
    let result = &res["result"];
    assert_ne!(result["isError"], true, "{res}");
    let v: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["company_name"], "THISTLE SOFTWARE LTD");
    assert_eq!(fake.requests().len(), 1);

    let res = call(
        &app,
        &session,
        4,
        "tools/call",
        json!({"name": "get_company", "arguments": {"company_number": "99999999"}}),
    )
    .await;
    assert_eq!(res["result"]["isError"], true, "{res}");
    assert!(
        res["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("not_found: ")
    );
}

#[tokio::test]
async fn invalid_codes_are_rejected_as_invalid_params() {
    let (app, fake) = app().await;
    let (session, _) = handshake(&app).await;
    let res = call(
        &app,
        &session,
        5,
        "tools/call",
        json!({"name": "filter_companies", "arguments": {"status": ["bankrupt"]}}),
    )
    .await;
    assert!(
        res.get("error").is_some() || res["result"]["isError"] == true,
        "{res}"
    );
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn requests_without_a_token_are_refused() {
    let (app, fake) = app().await;
    let res = app
        .oneshot(
            Request::post("/companies")
                .header("host", HOST)
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(Body::from(
                    json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert!(fake.requests().is_empty());
}
