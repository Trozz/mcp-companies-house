mod support;
use mcp_companies_house_data::api::{ChClient, MAX_BODY_BYTES};
use mcp_companies_house_data::error::ChError;
use serde_json::json;
use std::time::Duration;
use support::fake_ch::{AUTH, FakeCh, FakeState, Injected, RATE_LIMIT, now_unix};
use support::tools::{client, settings};

async fn fake() -> FakeCh {
    FakeCh::start(FakeState::register()).await
}

#[tokio::test]
async fn sends_the_key_as_basic_auth_username_with_a_blank_password() {
    let fake = fake().await;
    let v = client(&fake)
        .get("company_profile", &["company", "00000001"], &[])
        .await
        .unwrap();
    assert_eq!(v["company_name"], "ACME WIDGETS LIMITED");
    let log = fake.requests();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].authorization.as_deref(), Some(AUTH));
    assert_eq!(log[0].accept.as_deref(), Some("application/json"));
}

#[tokio::test]
async fn a_wrong_key_is_an_auth_error_and_is_not_retried() {
    let fake = fake().await;
    let c = ChClient::new(settings(&fake, "wrong-key")).unwrap();
    let e = c
        .get("company_profile", &["company", "00000001"], &[])
        .await
        .unwrap_err();
    assert!(matches!(e, ChError::Auth), "{e:?}");
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn missing_resources_are_not_found() {
    let fake = fake().await;
    let e = client(&fake)
        .get("company_profile", &["company", "99999999"], &[])
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "not_found");
    assert!(e.to_string().contains("/company/99999999"), "{e}");
}

#[tokio::test]
async fn one_server_error_is_retried() {
    let fake = fake().await;
    fake.inject(Injected::status(502));
    let v = client(&fake)
        .get("company_profile", &["company", "00000001"], &[])
        .await
        .unwrap();
    assert_eq!(v["company_number"], "00000001");
    assert_eq!(fake.requests().len(), 2);
}

#[tokio::test]
async fn a_second_server_error_is_reported_with_its_detail() {
    let fake = fake().await;
    fake.inject(Injected::status(500));
    fake.inject(Injected::json(503, json!({"error": "Service Unavailable"})));
    let e = client(&fake)
        .get("company_profile", &["company", "00000001"], &[])
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "upstream");
    let msg = e.to_string();
    assert!(msg.contains("HTTP 503"), "{msg}");
    assert!(msg.contains("Service Unavailable"), "{msg}");
    assert_eq!(fake.requests().len(), 2);
}

#[tokio::test]
async fn bad_requests_are_validation_errors_with_the_upstream_reason() {
    let fake = fake().await;
    fake.inject(Injected::json(
        400,
        json!({"errors": [{"error": "Invalid value {value} for size", "error_values": {"value": "0"}, "type": "ch:validation"}]}),
    ));
    let e = client(&fake)
        .get("advanced_search", &["advanced-search", "companies"], &[])
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "validation");
    assert!(e.to_string().contains("Invalid value 0 for size"), "{e}");
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn rate_limit_headers_are_tracked() {
    let fake = fake().await;
    let c = client(&fake);
    assert_eq!(c.rate_limit(), None);
    c.get("company_profile", &["company", "00000001"], &[])
        .await
        .unwrap();
    let rate = c.rate_limit().expect("rate limit recorded");
    assert_eq!(rate.limit, Some(RATE_LIMIT));
    assert_eq!(rate.remaining, RATE_LIMIT - 1);
}

#[tokio::test]
async fn a_429_is_not_retried_and_later_calls_fail_without_a_request() {
    let fake = fake().await;
    let reset = now_unix() + 120;
    fake.inject(
        Injected::json(429, json!({}))
            .header("x-ratelimit-remain", "0")
            .header("x-ratelimit-reset", reset.to_string()),
    );
    // The fake's own rate headers come first; make them agree with the injected 429.
    {
        let mut st = fake.state();
        st.rate_remaining = 1;
        st.rate_reset_unix = reset;
    }
    let c = client(&fake);
    let e = c
        .get("company_profile", &["company", "00000001"], &[])
        .await
        .unwrap_err();
    match e {
        ChError::RateLimited { reset_in_secs } => {
            assert!(
                matches!(reset_in_secs, Some(118..=120)),
                "{reset_in_secs:?}"
            )
        }
        other => panic!("expected rate_limited, got {other:?}"),
    }
    assert_eq!(fake.requests().len(), 1);
    let e = c
        .get("company_profile", &["company", "00000002"], &[])
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "rate_limited");
    assert!(e.to_string().contains("resets in"), "{e}");
    assert_eq!(
        fake.requests().len(),
        1,
        "no request while the window is used up"
    );
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let fake = fake().await;
    fake.inject(Injected::status(302).header("location", "https://elsewhere.example.com/x"));
    let e = client(&fake)
        .get("company_profile", &["company", "00000001"], &[])
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "upstream");
    assert!(e.to_string().contains("redirect"), "{e}");
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn invalid_json_is_an_upstream_error() {
    let fake = fake().await;
    fake.inject(Injected {
        status: 200,
        body: "<html>maintenance</html>".into(),
        headers: Vec::new(),
    });
    let e = client(&fake)
        .get("company_profile", &["company", "00000001"], &[])
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "upstream");
    assert!(e.to_string().contains("invalid JSON"), "{e}");
}

#[tokio::test]
async fn oversized_bodies_are_refused() {
    let fake = fake().await;
    fake.inject(Injected {
        status: 200,
        body: format!("\"{}\"", "x".repeat(MAX_BODY_BYTES)),
        headers: Vec::new(),
    });
    let e = client(&fake)
        .get("company_profile", &["company", "00000001"], &[])
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "upstream");
    assert!(e.to_string().contains("more than"), "{e}");
}

#[tokio::test]
async fn timeouts_are_retried_once_then_reported_without_the_query() {
    let fake = fake().await;
    fake.state().delay = Duration::from_millis(500);
    let mut s = settings(&fake, support::fake_ch::API_KEY);
    s.request_timeout = Duration::from_millis(100);
    let c = ChClient::new(s).unwrap();
    let e = c
        .get(
            "search_companies",
            &["search", "companies"],
            &[("q", "private search words".into())],
        )
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "upstream");
    let msg = e.to_string();
    assert!(msg.contains("timed out"), "{msg}");
    assert!(
        !msg.contains("private"),
        "query leaked into the error: {msg}"
    );
    // Both attempts reached the server before timing out.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(fake.requests().len(), 2);
}

#[tokio::test]
async fn unreachable_servers_are_upstream_errors() {
    let mut s = settings(&fake().await, support::fake_ch::API_KEY);
    s.base = url::Url::parse("http://127.0.0.1:9/").unwrap();
    let e = ChClient::new(s)
        .unwrap()
        .get("company_profile", &["company", "00000001"], &[])
        .await
        .unwrap_err();
    assert_eq!(e.kind(), "upstream");
    assert!(e.to_string().contains("failed"), "{e}");
}
