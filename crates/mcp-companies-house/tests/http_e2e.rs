use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use mcp_companies_house::{SERVICES, app, load};
use mcp_server_core::jwt::{Jwt, Payload, TokenKind};
use mcp_server_core::service::{HttpSettings, ServiceContext, ServiceEnv};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const TOKEN: &str = "static-test-token-0123456789";
const HOST: &str = "ch.example.com";
const SIGNING_KEY: &str = "0123456789abcdef0123456789abcdef";

/// Builds the application exactly as `main` does, from environment variables only.
fn server(extra: &[(&str, &str)]) -> axum::Router {
    let mut vars: HashMap<String, String> = [
        ("PUBLIC_URL", "https://ch.example.com"),
        ("OAUTH_SIGNING_KEY", SIGNING_KEY),
        ("OAUTH_ADMIN_PASSWORD", "a long password"),
        ("MCP_AUTH_TOKEN", TOKEN),
        ("INSECURE_HTTP", "true"),
        ("COMPANIES_HOUSE_API_KEY", "test-api-key"),
        // Keep the service off the network: nothing here calls Companies House.
        ("COMPANIES_HOUSE_API_URL", "http://127.0.0.1:9"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    for (k, v) in extra {
        vars.insert(k.to_string(), v.to_string());
    }
    let mut env = ServiceEnv::new(Box::new(move |k: &str| vars.get(k).cloned()));
    let loaded = load(&mut env, SERVICES).unwrap_or_else(|| panic!("{:?}", env.errors));
    let ctx = ServiceContext {
        http: HttpSettings::from_core(&loaded.core),
        shutdown: CancellationToken::new(),
    };
    app(loaded.core.oauth, &loaded.services, &ctx)
}

fn rpc(path: &str, body: Value, token: Option<&str>, session: Option<&str>) -> Request<Body> {
    let mut b = Request::post(path)
        .header("host", HOST)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    if let Some(s) = session {
        b = b
            .header("mcp-session-id", s)
            .header("mcp-protocol-version", "2025-06-18");
    }
    b.body(Body::from(body.to_string())).unwrap()
}

/// Reads an SSE (or plain JSON) response until the first complete JSON-RPC message.
async fn rpc_message(res: Response<Body>) -> Value {
    let mut body = res.into_body();
    let mut buf = String::new();
    loop {
        if let Ok(v) = serde_json::from_str::<Value>(buf.trim()) {
            return v;
        }
        for line in buf.lines() {
            if let Some(data) = line.strip_prefix("data:")
                && let Ok(v) = serde_json::from_str::<Value>(data.trim())
                && v.get("jsonrpc").is_some()
            {
                return v;
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

fn initialize() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "e2e", "version": "1"}
    }})
}

#[tokio::test]
async fn healthz_needs_no_auth() {
    let res = server(&[])
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn companies_is_served_at_its_path_with_the_static_token() {
    let app = server(&[]);
    let res = app
        .clone()
        .oneshot(rpc("/companies", initialize(), Some(TOKEN), None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let session = res.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_string();
    let init = rpc_message(res).await;
    assert_eq!(
        init["result"]["serverInfo"]["name"],
        "mcp-companies-house-data"
    );
    let res = app
        .clone()
        .oneshot(rpc(
            "/companies",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            Some(TOKEN),
            Some(&session),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let res = app
        .oneshot(rpc(
            "/companies",
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
            Some(TOKEN),
            Some(&session),
        ))
        .await
        .unwrap();
    let list = rpc_message(res).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 18, "{tools:?}");
}

#[tokio::test]
async fn companies_without_a_token_points_to_its_resource_metadata() {
    let app = server(&[]);
    let res = app
        .clone()
        .oneshot(rpc("/companies", initialize(), None, None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        res.headers()["www-authenticate"].to_str().unwrap(),
        "Bearer resource_metadata=\"https://ch.example.com/.well-known/oauth-protected-resource/companies\""
    );
    let res = app
        .oneshot(
            Request::get("/.well-known/oauth-protected-resource/companies")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["resource"], "https://ch.example.com/companies");
    assert_eq!(v["scopes_supported"], json!(["companies"]));
}

#[tokio::test]
async fn companies_sub_paths_are_guarded_too() {
    let app = server(&[]);
    let metadata = "Bearer resource_metadata=\"https://ch.example.com/.well-known/oauth-protected-resource/companies\"";
    for (path, token) in [
        ("/companies/unknown", None),
        ("/companies/x/y", None),
        ("/companies/unknown", Some("not-a-valid-token")),
        ("/companies", Some("not-a-valid-token")),
    ] {
        let res = app
            .clone()
            .oneshot(rpc(path, initialize(), token, None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{path} {token:?}");
        assert!(
            res.headers()["www-authenticate"]
                .to_str()
                .unwrap()
                .starts_with(metadata),
            "{path} {token:?}"
        );
    }
}

#[tokio::test]
async fn the_generic_mcp_path_is_not_served() {
    let res = server(&[])
        .oneshot(rpc("/mcp", initialize(), Some(TOKEN), None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invalid_service_selection_fails_to_load() {
    let mut env = ServiceEnv::new(Box::new(|k: &str| match k {
        "COMPANIES_HOUSE_SERVICES" => Some("nope".to_string()),
        _ => None,
    }));
    assert!(load(&mut env, SERVICES).is_none());
    assert!(
        env.errors
            .iter()
            .any(|e| e.starts_with("COMPANIES_HOUSE_SERVICES contains an unknown service: nope")),
        "{:?}",
        env.errors
    );
}

/// An OAuth access token as `/token` issues it: bound to one service's resource.
fn access_token_for(service: &str) -> String {
    let jwt = Jwt::new(
        SIGNING_KEY.as_bytes(),
        "https://ch.example.com",
        "https://ch.example.com",
    );
    let resource = format!("https://ch.example.com/{service}");
    jwt.issue_for(
        TokenKind::Access,
        Some(Duration::from_secs(3600)),
        Payload {
            client_id: Some("e2e".into()),
            scope: Some(service.into()),
            resource: Some(resource.clone()),
            ..Payload::default()
        },
        &resource,
    )
    .unwrap()
}

#[tokio::test]
async fn oauth_tokens_are_accepted_only_for_their_service() {
    let app = server(&[]);
    let token = access_token_for("companies");
    let res = app
        .clone()
        .oneshot(rpc("/companies", initialize(), Some(&token), None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let other = access_token_for("filing");
    let res = app
        .oneshot(rpc("/companies", initialize(), Some(&other), None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_service_sends_its_icons() {
    let res = server(&[])
        .oneshot(rpc("/companies", initialize(), Some(TOKEN), None))
        .await
        .unwrap();
    let init = rpc_message(res).await;
    let icons = init["result"]["serverInfo"]["icons"].as_array().unwrap();
    assert_eq!(icons.len(), 2);
    assert!(
        icons[0]["src"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    assert_eq!(icons[0]["mimeType"], "image/png");
    assert!(
        icons[1]["src"]
            .as_str()
            .unwrap()
            .starts_with("data:image/svg+xml;base64,")
    );
}

#[tokio::test]
async fn favicon_needs_no_auth() {
    let res = server(&[])
        .oneshot(Request::get("/favicon.ico").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "image/png");
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert!(body.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[tokio::test]
async fn oauth_metadata_lists_the_companies_resource() {
    let res = server(&[])
        .oneshot(
            Request::get("/.well-known/oauth-authorization-server")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["issuer"], "https://ch.example.com");
}
