mod oauth_support;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use mcp_server_core::credential::{CheckFuture, CredentialCheck, CredentialVerifier};
use mcp_server_core::server::build_app;
use oauth_support::*;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::ServiceExt;

const GOOD: &str = "good-key-0123456789";

/// Accepts only `GOOD`, after a delay, counting its checks.
struct Slow {
    checks: AtomicUsize,
}

impl CredentialVerifier for Slow {
    fn label(&self) -> &'static str {
        "API key"
    }
    fn help(&self) -> &'static str {
        ""
    }
    fn check<'a>(&'a self, credential: &'a str) -> CheckFuture<'a> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        let ok = credential == GOOD;
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if ok {
                CredentialCheck::Valid
            } else {
                CredentialCheck::Invalid(None)
            }
        })
    }
}

fn setup() -> (axum::Router, Arc<Slow>) {
    let v = Arc::new(Slow {
        checks: AtomicUsize::new(0),
    });
    let st = state_for(v.clone(), &["mail"]);
    let app = build_app(
        st,
        vec![(
            "mail",
            axum::Router::new().route("/", post(|| async { "ok" })),
        )],
    );
    (app, v)
}

fn bearer(scheme_and_token: &str, peer: [u8; 4]) -> Request<Body> {
    Request::post("/mail")
        .header("authorization", scheme_and_token)
        .extension(ConnectInfo(SocketAddr::from((peer, 4000))))
        .body(Body::empty())
        .unwrap()
}

async fn concurrently(app: &axum::Router, reqs: Vec<Request<Body>>) -> Vec<StatusCode> {
    let tasks: Vec<_> = reqs
        .into_iter()
        .map(|r| {
            let app = app.clone();
            tokio::spawn(async move { app.oneshot(r).await.unwrap().status() })
        })
        .collect();
    let mut out = Vec::new();
    for t in tasks {
        out.push(t.await.unwrap());
    }
    out
}

#[tokio::test]
async fn concurrent_bad_keys_from_one_client_stay_within_its_limit() {
    let (app, v) = setup();
    let reqs = (0..50)
        .map(|i| bearer(&format!("Bearer bad-key-{i:04}"), [10, 0, 0, 1]))
        .collect();
    let statuses = concurrently(&app, reqs).await;
    let checked = v.checks.load(Ordering::SeqCst);
    assert!(checked <= 5, "{checked} upstream checks");
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::UNAUTHORIZED)
            .count(),
        checked
    );
    assert!(
        statuses
            .iter()
            .all(|s| *s == StatusCode::UNAUTHORIZED || *s == StatusCode::TOO_MANY_REQUESTS)
    );
}

#[tokio::test]
async fn concurrent_requests_with_one_new_key_share_one_check() {
    let (app, v) = setup();
    let reqs = (0..20)
        .map(|_| bearer(&format!("Bearer {GOOD}"), [10, 0, 0, 2]))
        .collect();
    let statuses = concurrently(&app, reqs).await;
    assert!(
        statuses.iter().all(|s| *s == StatusCode::OK),
        "{statuses:?}"
    );
    assert_eq!(v.checks.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn other_peoples_failures_never_block_a_clean_client() {
    let (app, _) = setup();
    for ip in 1..=6u8 {
        for i in 0..5 {
            let res = app
                .clone()
                .oneshot(bearer(&format!("Bearer bad-{ip}-{i}-key"), [10, 0, 1, ip]))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }
    }
    let res = app
        .clone()
        .oneshot(bearer(&format!("Bearer {GOOD}"), [192, 0, 2, 9]))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "a fresh client signs in");
    // A client that failed itself is blocked, even with a working key.
    let (app, _) = setup();
    for ip in 1..=6u8 {
        for i in 0..5 {
            app.clone()
                .oneshot(bearer(&format!("Bearer bad-{ip}-{i}-key"), [10, 0, 1, ip]))
                .await
                .unwrap();
        }
    }
    let res = app
        .oneshot(bearer(&format!("Bearer {GOOD}"), [10, 0, 1, 1]))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn the_bearer_scheme_is_case_insensitive() {
    let (app, _) = setup();
    for header in [format!("bearer {GOOD}"), format!("BEARER {GOOD}")] {
        let res = app
            .clone()
            .oneshot(bearer(&header, [10, 0, 0, 3]))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{header}");
    }
}
