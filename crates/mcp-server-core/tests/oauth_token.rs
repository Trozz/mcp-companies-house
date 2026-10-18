mod oauth_support;
use axum::http::StatusCode;
use mcp_server_core::jwt::{Payload, TokenKind};
use oauth_support::*;
use std::time::Duration;

const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const REDIRECT: &str = "https://app.example.com/cb";

fn client(st: &mcp_server_core::oauth::AuthState) -> String {
    st.jwt
        .issue(
            TokenKind::Client,
            None,
            Payload {
                redirect_uris: Some(vec![REDIRECT.into()]),
                ..Default::default()
            },
        )
        .unwrap()
}

fn code(st: &mcp_server_core::oauth::AuthState, client_id: &str) -> String {
    st.jwt
        .issue(
            TokenKind::Code,
            Some(Duration::from_secs(60)),
            Payload {
                client_id: Some(client_id.into()),
                redirect_uri: Some(REDIRECT.into()),
                code_challenge: Some(CHALLENGE.into()),
                scope: Some("mail".into()),
                resource: Some("https://mail.example.com/mail".into()),
                ..Default::default()
            },
        )
        .unwrap()
}

fn exchange(
    code: &str,
    client_id: &str,
    verifier: &str,
    redirect: &str,
) -> axum::http::Request<axum::body::Body> {
    form_post(
        "/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client_id),
            ("code_verifier", verifier),
            ("redirect_uri", redirect),
        ],
    )
}

#[tokio::test]
async fn code_exchange_returns_tokens() {
    let st = state();
    let cid = client(&st);
    let res = send(
        &app(st.clone()),
        exchange(&code(&st, &cid), &cid, VERIFIER, REDIRECT),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["cache-control"], "no-store");
    let v = body_json(res).await;
    assert_eq!(v["token_type"], "Bearer");
    assert_eq!(v["expires_in"], 3600);
    assert_eq!(v["scope"], "mail");
    let access = st
        .jwt
        .verify_for(
            v["access_token"].as_str().unwrap(),
            TokenKind::Access,
            "https://mail.example.com/mail",
        )
        .unwrap();
    assert_eq!(access.aud, "https://mail.example.com/mail");
    let refresh = st
        .jwt
        .verify(v["refresh_token"].as_str().unwrap(), TokenKind::Refresh)
        .unwrap();
    assert_eq!(
        refresh.payload.resource.as_deref(),
        Some("https://mail.example.com/mail")
    );
}

#[tokio::test]
async fn code_cannot_be_reused() {
    let st = state();
    let cid = client(&st);
    let c = code(&st, &cid);
    let app = app(st.clone());
    assert_eq!(
        send(&app, exchange(&c, &cid, VERIFIER, REDIRECT))
            .await
            .status(),
        StatusCode::OK
    );
    let res = send(&app, exchange(&c, &cid, VERIFIER, REDIRECT)).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(res).await["error"], "invalid_grant");
}

#[tokio::test]
async fn wrong_verifier_fails() {
    let st = state();
    let cid = client(&st);
    let res = send(
        &app(st.clone()),
        exchange(
            &code(&st, &cid),
            &cid,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            REDIRECT,
        ),
    )
    .await;
    assert_eq!(body_json(res).await["error"], "invalid_grant");
}

#[tokio::test]
async fn mismatched_redirect_or_client_fails() {
    let st = state();
    let cid = client(&st);
    let app = app(st.clone());
    let res = send(
        &app,
        exchange(
            &code(&st, &cid),
            &cid,
            VERIFIER,
            "https://app.example.com/other",
        ),
    )
    .await;
    assert_eq!(body_json(res).await["error"], "invalid_grant");
    let other = client(&st);
    let res = send(&app, exchange(&code(&st, &cid), &other, VERIFIER, REDIRECT)).await;
    assert_eq!(body_json(res).await["error"], "invalid_grant");
}

#[tokio::test]
async fn access_token_is_not_accepted_as_code() {
    let st = state();
    let cid = client(&st);
    let access = st
        .jwt
        .issue(
            TokenKind::Access,
            Some(Duration::from_secs(60)),
            Payload {
                client_id: Some(cid.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    let res = send(
        &app(st.clone()),
        exchange(&access, &cid, VERIFIER, REDIRECT),
    )
    .await;
    assert_eq!(body_json(res).await["error"], "invalid_grant");
}

#[tokio::test]
async fn refresh_grant_returns_new_pair() {
    let st = state();
    let cid = client(&st);
    let refresh = st
        .jwt
        .issue(
            TokenKind::Refresh,
            Some(Duration::from_secs(60)),
            Payload {
                client_id: Some(cid.clone()),
                scope: Some("mail".into()),
                resource: Some("https://mail.example.com/mail".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let res = send(
        &app(st.clone()),
        form_post(
            "/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh),
                ("client_id", &cid),
            ],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let v = body_json(res).await;
    st.jwt
        .verify_for(
            v["access_token"].as_str().unwrap(),
            TokenKind::Access,
            "https://mail.example.com/mail",
        )
        .unwrap();
    st.jwt
        .verify(v["refresh_token"].as_str().unwrap(), TokenKind::Refresh)
        .unwrap();
}

#[tokio::test]
async fn access_token_is_not_accepted_as_refresh() {
    let st = state();
    let cid = client(&st);
    let access = st
        .jwt
        .issue(
            TokenKind::Access,
            Some(Duration::from_secs(60)),
            Payload {
                client_id: Some(cid.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    let res = send(
        &app(st.clone()),
        form_post(
            "/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &access),
                ("client_id", &cid),
            ],
        ),
    )
    .await;
    assert_eq!(body_json(res).await["error"], "invalid_grant");
}

#[tokio::test]
async fn refresh_grant_requires_client_id() {
    let st = state();
    let cid = client(&st);
    let refresh = st
        .jwt
        .issue(
            TokenKind::Refresh,
            Some(Duration::from_secs(60)),
            Payload {
                client_id: Some(cid),
                scope: Some("mail".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let res = send(
        &app(st.clone()),
        form_post(
            "/token",
            &[("grant_type", "refresh_token"), ("refresh_token", &refresh)],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(res).await["error"], "invalid_request");
}

#[tokio::test]
async fn unknown_grant_type() {
    let res = send(
        &app(state()),
        form_post("/token", &[("grant_type", "password")]),
    )
    .await;
    assert_eq!(body_json(res).await["error"], "unsupported_grant_type");
}

fn refresh_token(
    st: &mcp_server_core::oauth::AuthState,
    cid: &str,
    resource: Option<&str>,
) -> String {
    st.jwt
        .issue(
            TokenKind::Refresh,
            Some(Duration::from_secs(60)),
            Payload {
                client_id: Some(cid.into()),
                scope: Some("mail".into()),
                resource: resource.map(String::from),
                ..Default::default()
            },
        )
        .unwrap()
}

fn refresh_request(
    token: &str,
    cid: &str,
    resource: Option<&str>,
) -> axum::http::Request<axum::body::Body> {
    let mut pairs = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", token),
        ("client_id", cid),
    ];
    if let Some(r) = resource {
        pairs.push(("resource", r));
    }
    form_post("/token", &pairs)
}

#[tokio::test]
async fn refresh_without_resource_is_rejected() {
    let st = state();
    let cid = client(&st);
    let res = send(
        &app(st.clone()),
        refresh_request(&refresh_token(&st, &cid, None), &cid, None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(res).await["error"], "invalid_grant");
}

#[tokio::test]
async fn refresh_minted_for_the_old_mcp_audience_is_rejected() {
    let st = state();
    let cid = client(&st);
    let old = mcp_server_core::jwt::Jwt::new(
        b"0123456789abcdef0123456789abcdef",
        "https://mail.example.com",
        "https://mail.example.com/mcp",
    )
    .issue(
        TokenKind::Refresh,
        Some(Duration::from_secs(60)),
        Payload {
            client_id: Some(cid.clone()),
            scope: Some("mail".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let res = send(&app(st.clone()), refresh_request(&old, &cid, None)).await;
    assert_eq!(body_json(res).await["error"], "invalid_grant");
}

#[tokio::test]
async fn refresh_for_a_disabled_service_is_rejected() {
    let st = state();
    let cid = client(&st);
    let token = refresh_token(&st, &cid, Some("https://mail.example.com/calendar"));
    let res = send(&app(st.clone()), refresh_request(&token, &cid, None)).await;
    assert_eq!(body_json(res).await["error"], "invalid_target");
}

#[tokio::test]
async fn token_request_resource_must_match_the_grant() {
    let st = state_two_services(None);
    let cid = client(&st);
    let token = refresh_token(&st, &cid, Some("https://mail.example.com/mail"));
    let app = app(st.clone());
    let res = send(
        &app,
        refresh_request(&token, &cid, Some("https://mail.example.com/calendar")),
    )
    .await;
    assert_eq!(body_json(res).await["error"], "invalid_target");
    let res = send(
        &app,
        refresh_request(&token, &cid, Some("https://mail.example.com/mail")),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    let mut req_pairs = vec![
        ("grant_type", "authorization_code"),
        ("client_id", cid.as_str()),
        ("code_verifier", VERIFIER),
        ("redirect_uri", REDIRECT),
        ("resource", "https://mail.example.com/calendar"),
    ];
    let c = code(&st, &cid);
    req_pairs.push(("code", c.as_str()));
    let res = send(&app, form_post("/token", &req_pairs)).await;
    assert_eq!(body_json(res).await["error"], "invalid_target");
}
