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
        ChClient::new(ApiSettings::new(api_url())).unwrap(),
    ))
}

/// Request details of a caller signed in with the sandbox key from the environment.
fn live_auth() -> rmcp::handler::server::tool::Extension<axum::http::request::Parts> {
    support::tools::auth_with(key().expose_secret())
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
        .get_company(params(json!({"company_number": number})), live_auth())
        .await
        .unwrap();
    // Short form with the leading zeros dropped, when the number has any.
    let short = number.trim_start_matches('0').to_string();
    let found_short = s
        .get_company(params(json!({"company_number": short})), live_auth())
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
        .get_company(params(json!({"company_number": number})), live_auth())
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
        .company_report(params(json!({"company_number": number})), live_auth())
        .await
        .unwrap();
    let officers = s
        .list_officers(params(json!({"company_number": number})), live_auth())
        .await
        .unwrap();
    let filings = s
        .list_filing_history(params(json!({"company_number": number})), live_auth())
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
async fn due_diligence_and_records_on_a_generated_company() {
    let s = server();
    let (number, auth_code) = create_company().await;
    println!("created sandbox company {number}");
    let n = || params(json!({"company_number": number}));
    let check = s.due_diligence_check(n(), live_auth()).await.unwrap();
    let registers = s.get_company_registers(n(), live_auth()).await.unwrap();
    let exemptions = s.get_exemptions(n(), live_auth()).await.unwrap();
    let branches = s.get_uk_establishments(n(), live_auth()).await.unwrap();
    let current = s
        .list_officers(
            params(json!({"company_number": number, "current_only": true})),
            live_auth(),
        )
        .await
        .unwrap();
    let filings = s
        .list_filing_history(params(json!({"company_number": number})), live_auth())
        .await
        .unwrap();
    // Read what depends on the lists before the company is deleted.
    let officer_id = (!support::tools::is_error(&current))
        .then(|| {
            json(&current)["items"][0]["officer_id"]
                .as_str()
                .map(str::to_string)
        })
        .flatten();
    let network = match &officer_id {
        Some(id) => Some(
            s.officer_network(params(json!({"officer_id": id})), live_auth())
                .await
                .unwrap(),
        ),
        None => None,
    };
    let transaction = (!support::tools::is_error(&filings))
        .then(|| {
            json(&filings)["items"][0]["transaction_id"]
                .as_str()
                .map(str::to_string)
        })
        .flatten();
    let filing = match &transaction {
        Some(tx) => Some(
            s.get_filing(
                params(json!({"company_number": number, "transaction_id": tx})),
                live_auth(),
            )
            .await
            .unwrap(),
        ),
        None => None,
    };
    let document = s
        .download_filing_document(params(json!({"document_id": "abc"})), live_auth())
        .await
        .unwrap();
    delete_company(&number, &auth_code).await;
    println!("deleted sandbox company {number}");

    let v = json(&check);
    println!("due_diligence_check: {v}");
    assert_eq!(v["company"]["company_number"], number.as_str());
    assert!(v["observations"].is_array(), "{v}");
    assert!(
        v["checks_run"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "company_status"),
        "{v}"
    );
    assert!(v["disclaimer"].is_string(), "{v}");
    // The sandbox does not serve every record type: print what it answered.
    for (name, r) in [
        ("get_company_registers", &registers),
        ("get_exemptions", &exemptions),
        ("get_uk_establishments", &branches),
        ("list_officers current_only", &current),
    ] {
        println!("{name}: {}", support::tools::text(r));
    }
    if let Some(r) = &network {
        println!("officer_network: {}", support::tools::text(r));
        let v = json(r);
        assert_eq!(v["officer_id"].as_str(), officer_id.as_deref(), "{v}");
    }
    if let Some(r) = &filing {
        println!("get_filing: {}", support::tools::text(r));
        assert_eq!(json(r)["transaction_id"].as_str(), transaction.as_deref());
    }
    // There is no sandbox Document API: the tool says so instead of calling anything.
    let e = error_text(&document);
    assert!(e.contains("Document API has no sandbox"), "{e}");
}

#[tokio::test]
#[ignore = "needs a sandbox COMPANIES_HOUSE_API_KEY and network access"]
async fn searches_answer_in_the_expected_shape() {
    let s = server();
    let v = json(
        &s.search_companies(params(json!({"query": "limited", "limit": 5})), live_auth())
            .await
            .unwrap(),
    );
    println!("search_companies: {v}");
    assert!(v["total"].is_u64(), "{v}");
    assert!(v["items"].is_array(), "{v}");

    let v = json(
        &s.filter_companies(
            params(json!({"status": ["active"], "location": "London", "limit": 5})),
            live_auth(),
        )
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
    let r = server()
        .get_company(
            params(json!({"company_number": "00000001"})),
            support::tools::auth_with("00000000-0000-0000-0000-000000000000"),
        )
        .await
        .unwrap();
    assert!(error_text(&r).starts_with("auth: "), "{}", error_text(&r));
}

#[tokio::test]
#[ignore = "needs a sandbox COMPANIES_HOUSE_API_KEY and network access"]
async fn the_sign_in_check_accepts_a_real_key_and_rejects_a_wrong_one() {
    use mcp_companies_house_data::verifier::ChKeyVerifier;
    use mcp_server_core::credential::{CredentialCheck, CredentialVerifier};
    let v = ChKeyVerifier::new(Arc::new(
        ChClient::new(ApiSettings::new(api_url())).unwrap(),
    ));
    assert_eq!(v.check(key().expose_secret()).await, CredentialCheck::Valid);
    assert_eq!(
        v.check("00000000-0000-0000-0000-000000000000").await,
        CredentialCheck::Invalid(None)
    );
}
