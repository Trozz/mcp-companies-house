mod support;
use serde_json::json;
use support::fake_ch::{FakeCh, FakeState};
use support::tools::{auth, error_text, json, params, server};

async fn fake() -> FakeCh {
    FakeCh::start(FakeState::register()).await
}

#[tokio::test]
async fn filing_history_lists_filings_with_document_ids() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_filing_history(params(json!({"company_number": "1", "limit": 2})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 4);
    assert_eq!(v["count"], 2);
    assert_eq!(v["next_start_index"], 2);
    assert_eq!(
        v["items"][0],
        json!({
            "date": "2024-12-20", "type": "AA", "category": "accounts",
            "description": "accounts-with-accounts-type-micro-entity",
            "description_values": {"made_up_date": "2024-03-31"},
            "pages": 3, "transaction_id": "tx4", "document_id": "doc-tx4"
        })
    );
    assert_eq!(fake.requests()[0].path, "/company/00000001/filing-history");
}

#[tokio::test]
async fn filing_history_filters_by_category() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_filing_history(
                params(json!({
                    "company_number": "1", "categories": ["Accounts", "officers"]
                })),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 2);
    let types: Vec<&str> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, vec!["AA", "TM01"]);
    assert_eq!(
        fake.requests()[0].param("category"),
        Some("accounts,officers")
    );
}

#[tokio::test]
async fn filing_history_rejects_bad_categories_before_calling() {
    let fake = fake().await;
    let text = error_text(
        &server(&fake)
            .list_filing_history(
                params(json!({"company_number": "1", "categories": ["acc,ounts"]})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert!(text.starts_with("validation: "), "{text}");
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn charges_are_listed_with_counts() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_charges(params(json!({"company_number": "6"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 2);
    assert_eq!(v["satisfied_count"], 1);
    assert_eq!(v["part_satisfied_count"], 0);
    assert_eq!(
        v["items"][0],
        json!({
            "charge_id": "ch-1", "charge_code": "0000000600ch-1", "charge_number": 1,
            "status": "outstanding", "classification": "A registered charge",
            "created_on": "2019-01-02", "delivered_on": "2019-01-02",
            "persons_entitled": ["Big Bank PLC"],
            "particulars": {"description": "Land at 1 Poultry", "contains_fixed_charge": true}
        })
    );
    assert_eq!(v["items"][1]["satisfied_on"], "2005-05-05");
}

#[tokio::test]
async fn a_company_without_charges_has_none() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_charges(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(
        v,
        json!({"total": 0, "start_index": 0, "count": 0, "items": []})
    );
}

#[tokio::test]
async fn get_charge_returns_the_full_record() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.get_charge(
            params(json!({"company_number": "6", "charge_id": "ch-1"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(v["charge_id"], "ch-1");
    assert_eq!(
        v["secured_details"]["description"],
        "All monies due or to become due"
    );
    assert_eq!(
        v["transactions"][0]["filing_type"],
        "create-charge-with-deed"
    );
    assert!(
        v.get("id").is_none() && v.get("etag").is_none() && v.get("links").is_none(),
        "{v}"
    );
    assert_eq!(fake.requests()[0].path, "/company/00000006/charges/ch-1");

    let text = error_text(
        &s.get_charge(
            params(json!({"company_number": "6", "charge_id": "ch-9"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(
        text.starts_with("not_found: no charge ch-9 for company 00000006"),
        "{text}"
    );
    let text = error_text(
        &s.get_charge(
            params(json!({"company_number": "6", "charge_id": "ch/../1"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(text.starts_with("validation: "), "{text}");
}

#[tokio::test]
async fn insolvency_cases_are_shown() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .get_insolvency(params(json!({"company_number": "6"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["status"], "liquidation");
    assert_eq!(v["cases"][0]["type"], "creditors-voluntary-liquidation");
    assert_eq!(v["cases"][0]["dates"]["wound-up-on"], "2025-02-01");
    assert_eq!(
        v["cases"][0]["practitioners"][0]["address"],
        "1 Way, York, YO1 1AA"
    );
}

#[tokio::test]
async fn no_insolvency_record_is_an_empty_list() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .get_insolvency(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["cases"], json!([]));
    assert!(v["note"].as_str().unwrap().contains("no insolvency record"));
}

#[tokio::test]
async fn charges_and_insolvency_of_an_unknown_company_are_not_found() {
    let fake = fake().await;
    let s = server(&fake);
    for r in [
        s.list_charges(params(json!({"company_number": "12345678"})), auth())
            .await
            .unwrap(),
        s.get_insolvency(params(json!({"company_number": "12345678"})), auth())
            .await
            .unwrap(),
    ] {
        let text = error_text(&r);
        assert!(
            text.starts_with("not_found: no company with number 12345678"),
            "{text}"
        );
    }
}

#[tokio::test]
async fn one_filing_is_read_by_transaction_id() {
    let fake = fake().await;
    let srv = server(&fake);
    let v = json(
        &srv.get_filing(
            params(json!({"company_number": "1", "transaction_id": "tx4"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(v["type"], "AA");
    assert_eq!(v["document_id"], "doc-tx4");
    assert_eq!(
        fake.requests()[0].path,
        "/company/00000001/filing-history/tx4"
    );
    let e = error_text(
        &srv.get_filing(
            params(json!({"company_number": "1", "transaction_id": "tx-none"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(e.starts_with("not_found: no filing tx-none"), "{e}");
    let e = error_text(
        &srv.get_filing(
            params(json!({"company_number": "1", "transaction_id": "../x"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(e.starts_with("validation: "), "{e}");
}

#[tokio::test]
async fn registers_say_where_each_is_kept() {
    let fake = fake().await;
    let srv = server(&fake);
    let v = json(
        &srv.get_company_registers(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(
        v["registers"][0],
        json!({
            "register": "directors", "location": "public-register", "since": "2020-06-30",
            "history": [
                {"location": "public-register", "moved_on": "2020-06-30", "transaction_id": "txr2"},
                {"location": "single-alternative-inspection-location", "moved_on": "2017-01-01", "transaction_id": "txr1"}
            ]
        })
    );
    assert_eq!(
        v["registers"][1]["register"],
        "persons_with_significant_control"
    );

    // No register record is an answer, not an error; an unknown company is.
    let v = json(
        &srv.get_company_registers(params(json!({"company_number": "2"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["registers"], json!([]));
    assert!(v["note"].as_str().unwrap().contains("registered office"));
    let e = error_text(
        &srv.get_company_registers(params(json!({"company_number": "99999999"})), auth())
            .await
            .unwrap(),
    );
    assert!(
        e.starts_with("not_found: no company with number 99999999"),
        "{e}"
    );
}

#[tokio::test]
async fn exemptions_are_listed_or_reported_as_none() {
    let fake = fake().await;
    let srv = server(&fake);
    let v = json(
        &srv.get_exemptions(params(json!({"company_number": "6"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(
        v,
        json!({"exemptions": [{
            "exemption": "psc-exempt-as-trading-on-regulated-market", "current": true,
            "periods": [{"exempt_from": "2016-04-06"}]
        }]})
    );
    let v = json(
        &srv.get_exemptions(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["exemptions"], json!([]));
    let e = error_text(
        &srv.get_exemptions(params(json!({"company_number": "99999999"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.starts_with("not_found: "), "{e}");
}

#[tokio::test]
async fn uk_establishments_of_an_overseas_company_are_listed() {
    let fake = fake().await;
    let srv = server(&fake);
    let v = json(
        &srv.get_uk_establishments(params(json!({"company_number": "FC7"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 2);
    assert_eq!(
        v["items"][0],
        json!({"company_number": "BR000001", "company_name": "OVERSEAS WIDGETS INC (UK BRANCH)",
               "company_status": "open", "locality": "London"})
    );
    assert_eq!(
        fake.requests()[0].path,
        "/company/FC000007/uk-establishments"
    );
    let v = json(
        &srv.get_uk_establishments(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v, json!({"total": 0, "items": []}));
    let e = error_text(
        &srv.get_uk_establishments(params(json!({"company_number": "99999999"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.starts_with("not_found: "), "{e}");
}
