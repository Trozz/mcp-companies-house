mod support;
use serde_json::{Value, json};
use support::fake_ch::{FakeCh, FakeState};
use support::tools::{error_text, json, params, server};

async fn fake() -> FakeCh {
    FakeCh::start(FakeState::register()).await
}

fn names(v: &Value) -> Vec<&str> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn list_officers_shows_current_and_resigned_with_ids() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_officers(params(json!({"company_number": "1"})))
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 3);
    assert_eq!(v["active_count"], 2);
    assert_eq!(v["resigned_count"], 1);
    assert_eq!(
        names(&v),
        vec!["SMITH, Jane Anne", "BROWN, Tom", "ACME SECRETARIES LIMITED"]
    );
    assert_eq!(
        v["items"][0],
        json!({
            "name": "SMITH, Jane Anne", "officer_role": "director", "appointed_on": "2015-03-01",
            "occupation": "Engineer", "nationality": "British", "country_of_residence": "England",
            "date_of_birth": "1980-04", "address": "1, Park Row, Leeds, LS1 5AB, England",
            "officer_id": "off-jane"
        })
    );
    assert_eq!(v["items"][1]["resigned_on"], "2019-12-31");
    assert_eq!(
        v["items"][2]["identification"]["registration_number"],
        "00000099"
    );
    let log = fake.requests();
    assert_eq!(log[0].path, "/company/00000001/officers");
    assert_eq!(log[0].param("items_per_page"), Some("20"));
}

#[tokio::test]
async fn list_officers_pages_and_orders() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_officers(params(json!({
                "company_number": "00000001", "limit": 2, "order_by": "surname"
            })))
            .await
            .unwrap(),
    );
    assert_eq!(v["count"], 2);
    assert_eq!(v["next_start_index"], 2);
    assert_eq!(fake.requests()[0].param("order_by"), Some("surname"));
}

#[tokio::test]
async fn list_officers_for_an_unknown_company_is_not_found() {
    let fake = fake().await;
    let text = error_text(
        &server(&fake)
            .list_officers(params(json!({"company_number": "12345678"})))
            .await
            .unwrap(),
    );
    assert!(
        text.starts_with("not_found: no officers for company 12345678"),
        "{text}"
    );
}

#[tokio::test]
async fn officer_appointments_list_other_companies() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.get_officer_appointments(params(json!({"officer_id": "off-jane"})))
            .await
            .unwrap(),
    );
    assert_eq!(v["officer_id"], "off-jane");
    assert_eq!(v["name"], "Jane Anne SMITH");
    assert_eq!(v["date_of_birth"], "1980-04");
    assert_eq!(v["is_corporate_officer"], false);
    assert_eq!(v["total"], 3);
    let companies: Vec<&str> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["company_name"].as_str().unwrap())
        .collect();
    assert_eq!(
        companies,
        vec![
            "ACME WIDGETS LIMITED",
            "ACME BAKERY LIMITED",
            "ACME HOLDINGS PLC"
        ]
    );
    assert_eq!(v["items"][2]["resigned_on"], "2010-01-01");

    let v = json(
        &s.get_officer_appointments(params(
            json!({"officer_id": "off-jane", "active_only": true}),
        ))
        .await
        .unwrap(),
    );
    assert_eq!(v["total"], 2);
    assert_eq!(fake.requests()[1].param("filter"), Some("active"));
}

#[tokio::test]
async fn officer_appointments_validate_the_id() {
    let fake = fake().await;
    let s = server(&fake);
    let text = error_text(
        &s.get_officer_appointments(params(json!({"officer_id": "../company/1"})))
            .await
            .unwrap(),
    );
    assert!(text.starts_with("validation: "), "{text}");
    let text = error_text(
        &s.get_officer_appointments(params(json!({"officer_id": "unknown-id"})))
            .await
            .unwrap(),
    );
    assert!(text.starts_with("not_found: "), "{text}");
    assert!(text.contains("search_officers"), "{text}");
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn search_officers_returns_officer_ids() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .search_officers(params(json!({"query": "smith"})))
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 1);
    assert_eq!(
        v["items"][0],
        json!({
            "name": "Jane Anne SMITH", "officer_id": "off-jane", "appointment_count": 3,
            "date_of_birth": "1980-04", "address": "1 Park Row, Leeds, LS1 5AB",
            "description": "Total number of appointments 3 - Born April 1980"
        })
    );
    assert_eq!(fake.requests()[0].param("q"), Some("smith"));
}

#[tokio::test]
async fn psc_lists_owners_and_statements() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.list_persons_with_significant_control(params(json!({"company_number": "1"})))
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 2);
    assert_eq!(v["active_count"], 1);
    assert_eq!(v["ceased_count"], 1);
    assert_eq!(v["items"][0]["kind"], "individual");
    assert_eq!(
        v["items"][0]["natures_of_control"],
        json!([
            "ownership-of-shares-75-to-100-percent",
            "voting-rights-75-to-100-percent"
        ])
    );
    assert_eq!(v["items"][1]["ceased_on"], "2019-12-31");
    assert_eq!(v["statements"], json!([]));
    let paths: Vec<String> = fake.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(
        paths,
        vec![
            "/company/00000001/persons-with-significant-control",
            "/company/00000001/persons-with-significant-control-statements"
        ]
    );

    // No PSCs, only a statement.
    let v = json(
        &s.list_persons_with_significant_control(params(json!({"company_number": "5"})))
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 0);
    assert_eq!(
        v["statements"],
        json!([{"statement": "no-individual-or-entity-with-signficant-control", "notified_on": "2022-02-02"}])
    );
}

#[tokio::test]
async fn psc_are_listed_when_the_statements_fail() {
    let fake = fake().await;
    let list =
        support::fake_records::fixtures()["/company/00000001/persons-with-significant-control"]
            .clone();
    fake.inject(support::fake_ch::Injected::json(200, list));
    fake.inject(support::fake_ch::Injected::status(500));
    fake.inject(support::fake_ch::Injected::status(500));
    let v = json(
        &server(&fake)
            .list_persons_with_significant_control(params(json!({"company_number": "1"})))
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 2);
    assert!(v.get("statements").is_none(), "{v}");
    assert!(
        v["statements_unavailable"]
            .as_str()
            .unwrap()
            .starts_with("upstream: "),
        "{v}"
    );
}

#[tokio::test]
async fn later_psc_pages_skip_the_statements() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_persons_with_significant_control(params(json!({
                "company_number": "1", "start_index": 1, "limit": 1
            })))
            .await
            .unwrap(),
    );
    assert!(v.get("statements").is_none(), "{v}");
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn disqualified_officers_can_be_searched_and_read() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.search_disqualified_officers(params(json!({"query": "bloggs"})))
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["officer_id"], "dq-john");
    assert_eq!(v["items"][0]["officer_kind"], "natural");

    let v = json(
        &s.get_disqualified_officer(params(
            json!({"officer_id": "dq-john", "officer_kind": "natural"}),
        ))
        .await
        .unwrap(),
    );
    assert_eq!(v["officer_id"], "dq-john");
    assert_eq!(v["officer_kind"], "natural");
    assert_eq!(v["surname"], "BLOGGS");
    assert_eq!(
        v["disqualifications"][0]["disqualified_until"],
        "2026-01-01"
    );
    assert!(v.get("etag").is_none() && v.get("links").is_none(), "{v}");
}

#[tokio::test]
async fn disqualified_officer_kind_is_found_when_not_given() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.get_disqualified_officer(params(json!({"officer_id": "dq-rogue"})))
            .await
            .unwrap(),
    );
    assert_eq!(v["officer_kind"], "corporate");
    assert_eq!(v["name"], "ROGUE TRADING LIMITED");
    let paths: Vec<String> = fake.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(
        paths,
        vec![
            "/disqualified-officers/natural/dq-rogue",
            "/disqualified-officers/corporate/dq-rogue"
        ]
    );
    let text = error_text(
        &s.get_disqualified_officer(params(json!({"officer_id": "nobody"})))
            .await
            .unwrap(),
    );
    assert!(text.starts_with("not_found: "), "{text}");
}
