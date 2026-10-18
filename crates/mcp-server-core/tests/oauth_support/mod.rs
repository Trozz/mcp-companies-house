#![allow(dead_code)]
use axum::body::Body;
use axum::http::{Request, Response};
use http_body_util::BodyExt;
use mcp_server_core::credential::{CredentialVerifier, TestVerifier};
use mcp_server_core::oauth::{AuthState, OAuthSettings, oauth_router};
use std::net::IpAddr;
use std::sync::Arc;
use tower::ServiceExt;

pub const PUBLIC_URL: &str = "https://mail.example.com";
/// A credential the test verifier accepts.
pub const KEY: &str = "good-api-key-0123";
/// A credential the test verifier cannot check ("upstream down").
pub const DOWN_KEY: &str = "down-api-key";

pub fn verifier() -> Arc<TestVerifier> {
    TestVerifier::new(&[KEY], Some(DOWN_KEY))
}

/// One service, `mail`.
pub fn state() -> Arc<AuthState> {
    state_with(verifier(), Vec::new(), &["mail"])
}

/// Two services, `mail` and `calendar`.
pub fn state_two_services() -> Arc<AuthState> {
    state_with(verifier(), Vec::new(), &["mail", "calendar"])
}

/// `mail` and the combined endpoint, `mcp`.
pub fn state_with_combined() -> Arc<AuthState> {
    state_with(verifier(), Vec::new(), &["mail", "mcp"])
}

pub fn state_with_trusted_proxies(trusted_proxies: Vec<IpAddr>) -> Arc<AuthState> {
    state_with(verifier(), trusted_proxies, &["mail"])
}

/// One service, `mail`, checking credentials with `verifier`.
pub fn state_with_verifier(verifier: Arc<TestVerifier>) -> Arc<AuthState> {
    state_with(verifier, Vec::new(), &["mail"])
}

/// The given services, checking credentials with any verifier.
pub fn state_for(verifier: Arc<dyn CredentialVerifier>, services: &[&str]) -> Arc<AuthState> {
    state_with(verifier, Vec::new(), services)
}

fn state_with(
    verifier: Arc<dyn CredentialVerifier>,
    trusted_proxies: Vec<IpAddr>,
    services: &[&str],
) -> Arc<AuthState> {
    AuthState::new(
        OAuthSettings {
            public_url: PUBLIC_URL.into(),
            signing_key: "0123456789abcdef0123456789abcdef".to_string().into(),
            trusted_proxies,
        },
        services,
        verifier,
    )
}

/// `KEY` sealed for tokens of `st`.
pub fn sealed(st: &AuthState) -> Option<String> {
    Some(st.sealer.seal(KEY).unwrap())
}

pub fn app(state: Arc<AuthState>) -> axum::Router {
    oauth_router(state)
}

pub async fn send(app: &axum::Router, req: Request<Body>) -> Response<Body> {
    app.clone().oneshot(req).await.unwrap()
}

pub async fn body_bytes(res: Response<Body>) -> Vec<u8> {
    res.into_body().collect().await.unwrap().to_bytes().to_vec()
}

pub async fn body_json(res: Response<Body>) -> serde_json::Value {
    serde_json::from_slice(&body_bytes(res).await).unwrap()
}

pub async fn body_text(res: Response<Body>) -> String {
    String::from_utf8(body_bytes(res).await).unwrap()
}

pub fn json_post(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::post(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub fn form_post(uri: &str, pairs: &[(&str, &str)]) -> Request<Body> {
    form_post_from(uri, pairs, [10, 0, 0, 1], None)
}

pub fn form_post_from(
    uri: &str,
    pairs: &[(&str, &str)],
    peer: [u8; 4],
    forwarded_for: Option<&str>,
) -> Request<Body> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish();
    let mut b = Request::post(uri)
        .header("content-type", "application/x-www-form-urlencoded")
        .extension(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            peer, 5555,
        ))));
    if let Some(xff) = forwarded_for {
        b = b.header("x-forwarded-for", xff);
    }
    b.body(Body::from(body)).unwrap()
}

pub fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}
