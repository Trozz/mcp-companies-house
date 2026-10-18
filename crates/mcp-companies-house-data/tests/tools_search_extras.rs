mod support;
use serde_json::json;
use support::fake_ch::{FakeCh, FakeState, Injected};
use support::tools::{auth, error_text, json, params, server};

async fn fake() -> FakeCh {
    FakeCh::start(FakeState::register()).await
}

#[tokio::test]
async fn get_company_describes_its_sic_codes() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .get_company(params(json!({"company_number": "2"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["sic_codes"], json!(["10710", "47240"]));
    assert_eq!(
        v["nature_of_business"],
        json!([
            {"code": "10710", "description": "Manufacture of bread; manufacture of fresh pastry goods and cakes"},
            {"code": "47240", "description": "Retail sale of bread, cakes, flour confectionery and sugar confectionery in specialised stores"}
        ])
    );
}

#[tokio::test]
async fn dissolved_companies_are_found_by_name_or_previous_name() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.search_dissolved_companies(params(json!({"query": "thistle"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 1);
    assert_eq!(
        v["items"][0],
        json!({
            "company_number": "SC000003", "company_name": "THISTLE SOFTWARE LTD",
            "company_status": "dissolved", "date_of_creation": "2010-01-10",
            "date_of_cessation": "2020-05-05",
            "address": "10 Buchanan Street, Glasgow, G1 3HL, Scotland",
            "previous_company_names": [{"name": "THISTLE CODE LTD", "effective_from": "2010-01-10", "ceased_on": "2012-02-02"}]
        })
    );
    let log = fake.requests();
    assert_eq!(log[0].path, "/dissolved-search/companies");
    assert_eq!(log[0].param("search_type"), Some("best-match"));
    assert_eq!(log[0].param("size"), Some("20"));

    let v = json(
        &s.search_dissolved_companies(
            params(json!({"query": "thistle code", "match_on": "previous_names"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(v["items"][0]["matched_previous_name"], "THISTLE CODE LTD");
    assert_eq!(
        fake.requests()[1].param("search_type"),
        Some("previous-name-dissolved")
    );

    let v = json(
        &s.search_dissolved_companies(params(json!({"query": "nothing like it"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 0);
}

#[tokio::test]
async fn name_availability_uses_the_legally_equivalent_search() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.check_company_name_availability(params(json!({"name": "Acme Widgets Ltd"})), auth())
            .await
            .unwrap(),
    );
    let log = fake.requests();
    assert_eq!(log[0].path, "/search/companies");
    assert_eq!(
        log[0].param("restrictions"),
        Some("active-companies legally-equivalent-company-name")
    );
    assert_eq!(v["name"], "Acme Widgets Ltd");
    assert!(v["note"].as_str().unwrap().contains("too like"));

    // The fake does no legal-equivalence matching; serve the live answer shapes directly.
    fake.inject(Injected::json(200, json!({
        "kind": "search#companies", "total_results": 0, "items": [], "items_per_page": 100, "start_index": 0
    })));
    let v = json(
        &s.check_company_name_availability(params(json!({"name": "Zebra Quantum Ltd"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["available"], true);
    assert_eq!(v["conflicting_companies"], json!([]));

    fake.inject(Injected::json(200, json!({
        "kind": "search#companies", "total_results": 1, "items_per_page": 100, "start_index": 0,
        "items": [{"title": "ACME WIDGETS LIMITED", "company_number": "00000001", "company_status": "active",
                   "company_type": "ltd", "address_snippet": "Leeds", "date_of_creation": "2015-03-01"}]
    })));
    let v = json(
        &s.check_company_name_availability(params(json!({"name": "Acme Widgets"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["available"], false);
    assert_eq!(v["conflicting_companies"][0]["company_number"], "00000001");
}

#[tokio::test]
async fn browse_lists_companies_in_name_order() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .browse_company_names(params(json!({"name": "acme h", "limit": 3})), auth())
            .await
            .unwrap(),
    );
    let names: Vec<&str> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["company_name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "ACME HOLDINGS PLC",
            "ACME WIDGETS LIMITED",
            "GREEN FUTURES CIC"
        ]
    );
    assert_eq!(v["count"], 3);
    assert_eq!(fake.requests()[0].param("size"), Some("3"));
}

#[tokio::test]
async fn sic_lookup_finds_and_describes_codes_without_calling_companies_house() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.lookup_sic_codes(params(json!({"query": "bakery"})))
            .await
            .unwrap(),
    );
    let codes: Vec<&str> = v["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["code"].as_str().unwrap())
        .collect();
    assert!(
        codes.contains(&"10710") && codes.contains(&"47240"),
        "{codes:?}"
    );

    let v = json(
        &s.lookup_sic_codes(params(json!({"codes": ["62012", "99999", "12345"]})))
            .await
            .unwrap(),
    );
    assert_eq!(
        v["codes"][0]["description"],
        "Business and domestic software development"
    );
    assert_eq!(v["codes"][1]["description"], "Dormant Company");
    assert!(v["codes"][2]["description"].is_null());

    let text = error_text(&s.lookup_sic_codes(params(json!({}))).await.unwrap());
    assert!(text.contains("give a query or codes"), "{text}");
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn search_all_mixes_companies_officers_and_disqualified_officers() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .search_all(params(json!({"query": "o"})), auth())
            .await
            .unwrap(),
    );
    let kinds: std::collections::BTreeSet<&str> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["company", "disqualified_officer", "officer"]
            .into_iter()
            .collect()
    );
    let officer = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "officer")
        .unwrap();
    assert!(officer["officer_id"].is_string(), "{officer}");
    let disqualified = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "disqualified_officer")
        .unwrap();
    assert!(disqualified["officer_kind"].is_string(), "{disqualified}");
    assert_eq!(fake.requests()[0].path, "/search");
}
