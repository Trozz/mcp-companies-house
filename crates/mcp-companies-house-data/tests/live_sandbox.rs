//! Smoke tests against the Companies House sandbox. Ignored by default; run with a sandbox REST
//! API key:
//!
//! ```bash
//! COMPANIES_HOUSE_API_KEY=... cargo test -p mcp-companies-house-data --test live_sandbox -- --ignored --nocapture
//! ```
//!
//! The round trip creates one test company with the sandbox test data generator and always
//! deletes it again. These tests refuse to run against the live API.
mod support;
use mcp_companies_house_data::api::{ApiSettings, ChClient};
use mcp_companies_house_data::config::SANDBOX_API_URL;
use mcp_companies_house_data::tools::CompaniesServer;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::sync::Arc;
use support::tools::{error_text, json, params};

const TEST_DATA_URL: &str = "https://test-data-sandbox.company-information.service.gov.uk";

fn key() -> SecretString {
    SecretString::from(
        std::env::var("COMPANIES_HOUSE_API_KEY")
            .expect("set COMPANIES_HOUSE_API_KEY to a sandbox REST API key"),
    )
}

fn api_url() -> url::Url {
    let raw = std::env::var("COMPANIES_HOUSE_API_URL").unwrap_or_else(|_| SANDBOX_API_URL.into());
    let url = url::Url::parse(&raw).expect("COMPANIES_HOUSE_API_URL is a URL");
    assert!(
        url.host_str().is_some_and(|h| h.contains("sandbox")),
        "live tests only run against the sandbox, not {raw}"
    );
    url
}

fn server() -> CompaniesServer {
    CompaniesServer::new(Arc::new(
        ChClient::new(ApiSettings::new(api_url(), key())).unwrap(),
    ))
}

fn http() -> reqwest::Client {
    mcp_server_core::install_crypto_provider();
    reqwest::Client::new()
}

/// Creates a test company; returns its number and authentication code.
async fn create_company() -> (String, String) {
    let res = http()
        .post(format!("{TEST_DATA_URL}/test-data/company"))
        .basic_auth(key().expose_secret(), Some(""))
        .header("content-type", "application/json")
        .body(json!({"jurisdiction": "england-wales"}).to_string())
        .send()
        .await
        .expect("test data generator reachable");
    let status = res.status();
    let body: Value = serde_json::from_str(&res.text().await.unwrap()).expect("JSON response");
    assert!(
        status.is_success(),
        "creating a test company: {status} {body}"
    );
    (
        body["company_number"].as_str().unwrap().to_string(),
        body["auth_code"].as_str().unwrap().to_string(),
    )
}

async fn delete_company(number: &str, auth_code: &str) {
    let res = http()
        .delete(format!("{TEST_DATA_URL}/test-data/company/{number}"))
        .basic_auth(key().expose_secret(), Some(""))
        .header("content-type", "application/json")
        .body(json!({"auth_code": auth_code}).to_string())
        .send()
        .await
        .expect("test data generator reachable");
    assert!(
        res.status().is_success(),
        "deleting test company {number}: {}",
        res.status()
    );
}

#[tokio::test]
#[ignore = "needs a sandbox COMPANIES_HOUSE_API_KEY and network access"]
async fn get_company_round_trip_with_a_generated_company() {
    // server() checks the URL is the sandbox: do that before creating anything.
    let s = server();
    let (number, auth_code) = create_company().await;
    println!("created sandbox company {number}");
    let found = s
        .get_company(params(json!({"company_number": number})))
        .await
        .unwrap();
    // Short form with the leading zeros dropped, when the number has any.
    let short = number.trim_start_matches('0').to_string();
    let found_short = s
        .get_company(params(json!({"company_number": short})))
        .await
        .unwrap();
    delete_company(&number, &auth_code).await;
    println!("deleted sandbox company {number}");

    let v = json(&found);
    assert_eq!(v["company_number"], number.as_str());
    assert_eq!(v["company_status"], "active");
    assert!(v["company_name"].as_str().unwrap().contains(&number), "{v}");
    assert!(v.get("etag").is_none(), "{v}");
    assert!(v["registered_office_address"].is_object(), "{v}");
    assert_eq!(json(&found_short)["company_number"], number.as_str());

    let gone = s
        .get_company(params(json!({"company_number": number})))
        .await
        .unwrap();
    assert!(error_text(&gone).starts_with("not_found: "));
}

#[tokio::test]
#[ignore = "needs a sandbox COMPANIES_HOUSE_API_KEY and network access"]
async fn company_report_on_a_generated_company() {
    // server() checks the URL is the sandbox: do that before creating anything.
    let s = server();
    let (number, auth_code) = create_company().await;
    println!("created sandbox company {number}");
    let report = s
        .company_report(params(json!({"company_number": number})))
        .await
        .unwrap();
    let officers = s
        .list_officers(params(json!({"company_number": number})))
        .await
        .unwrap();
    let filings = s
        .list_filing_history(params(json!({"company_number": number})))
        .await
        .unwrap();
    delete_company(&number, &auth_code).await;
    println!("deleted sandbox company {number}");

    // The sandbox serves profiles reliably but often fails officers and filing history (HTTP
    // 500/502): the report must still answer, listing what it could not read.
    let v = json(&report);
    println!("company_report: {v}");
    assert_eq!(v["company"]["company_number"], number.as_str());
    assert!(v["company"]["nature_of_business"].is_array(), "{v}");
    for section in [
        "officers",
        "persons_with_significant_control",
        "recent_filings",
    ] {
        assert!(
            v.get(section).is_some() || v["unavailable"].get(section).is_some(),
            "{section} neither present nor listed as unavailable: {v}"
        );
    }
    println!("list_officers: {}", support::tools::text(&officers));
    println!("list_filing_history: {}", support::tools::text(&filings));
}

#[tokio::test]
#[ignore = "needs a sandbox COMPANIES_HOUSE_API_KEY and network access"]
async fn searches_answer_in_the_expected_shape() {
    let s = server();
    let v = json(
        &s.search_companies(params(json!({"query": "limited", "limit": 5})))
            .await
            .unwrap(),
    );
    println!("search_companies: {v}");
    assert!(v["total"].is_u64(), "{v}");
    assert!(v["items"].is_array(), "{v}");

    let v = json(
        &s.filter_companies(params(
            json!({"status": ["active"], "location": "London", "limit": 5}),
        ))
        .await
        .unwrap(),
    );
    println!("filter_companies: {v}");
    assert!(v["total"].is_u64(), "{v}");
    assert!(v["items"].is_array(), "{v}");
}

#[tokio::test]
#[ignore = "needs network access"]
async fn a_wrong_key_is_reported_as_auth() {
    let s = CompaniesServer::new(Arc::new(
        ChClient::new(ApiSettings::new(
            api_url(),
            SecretString::from("00000000-0000-0000-0000-000000000000"),
        ))
        .unwrap(),
    ));
    let r = s
        .get_company(params(json!({"company_number": "00000001"})))
        .await
        .unwrap();
    assert!(error_text(&r).starts_with("auth: "), "{}", error_text(&r));
}
