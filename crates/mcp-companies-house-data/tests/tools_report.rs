mod support;
use serde_json::json;
use support::fake_ch::{FakeCh, FakeState, Injected};
use support::tools::{auth, error_text, json, params, server};

async fn fake() -> FakeCh {
    FakeCh::start(FakeState::register()).await
}

fn paths(fake: &FakeCh) -> Vec<String> {
    let mut p: Vec<String> = fake.requests().into_iter().map(|r| r.path).collect();
    p.sort();
    p
}

#[tokio::test]
async fn report_gathers_every_section_for_a_company() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .company_report(params(json!({"company_number": "1", "filings": 2})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["company"]["company_name"], "ACME WIDGETS LIMITED");
    assert_eq!(v["company"]["nature_of_business"][0]["code"], "62012");
    let active: Vec<&str> = v["officers"]["active"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert_eq!(active, vec!["SMITH, Jane Anne", "ACME SECRETARIES LIMITED"]);
    assert_eq!(v["officers"]["resigned_count"], 1);
    let psc = &v["persons_with_significant_control"];
    assert_eq!(psc["active"].as_array().unwrap().len(), 1);
    assert_eq!(psc["active"][0]["name"], "Mrs Jane Anne Smith");
    assert_eq!(psc["ceased_count"], 1);
    assert_eq!(v["recent_filings"].as_array().unwrap().len(), 2);
    assert_eq!(v["recent_filings"][0]["type"], "AA");
    // The profile lists no charges or insolvency, so neither is requested.
    assert_eq!(v["charges"], json!({"total": 0}));
    assert_eq!(v["insolvency"], json!({"cases": []}));
    assert!(v.get("unavailable").is_none(), "{v}");
    assert_eq!(
        paths(&fake),
        vec![
            "/company/00000001",
            "/company/00000001/filing-history",
            "/company/00000001/officers",
            "/company/00000001/persons-with-significant-control",
            "/company/00000001/persons-with-significant-control-statements",
        ]
    );
}

#[tokio::test]
async fn report_reads_charges_and_insolvency_when_the_profile_lists_them() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .company_report(params(json!({"company_number": "6", "filings": 0})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["company"]["company_status"], "liquidation");
    assert_eq!(v["officers"]["active"], json!([]));
    assert_eq!(v["charges"]["total"], 2);
    let open = v["charges"]["not_satisfied"].as_array().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["charge_id"], "ch-1");
    assert_eq!(
        v["insolvency"]["cases"][0]["type"],
        "creditors-voluntary-liquidation"
    );
    assert!(v.get("recent_filings").is_none(), "{v}");
    let p = paths(&fake);
    assert!(
        p.contains(&"/company/00000006/charges".to_string()),
        "{p:?}"
    );
    assert!(
        p.contains(&"/company/00000006/insolvency".to_string()),
        "{p:?}"
    );
    assert!(!p.iter().any(|x| x.ends_with("filing-history")), "{p:?}");
}

#[tokio::test]
async fn report_lists_sections_that_failed() {
    let fake = fake().await;
    // The profile succeeds; the three sections read next (officers, PSCs, filings) fail on
    // their first attempt and on the retry.
    let profile = fake.state().companies["00000001"].clone();
    fake.inject(Injected::json(200, profile));
    for _ in 0..6 {
        fake.inject(Injected::status(500));
    }
    let v = json(
        &server(&fake)
            .company_report(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["company"]["company_number"], "00000001");
    let unavailable = v["unavailable"]
        .as_object()
        .expect("an unavailable section");
    let mut failed: Vec<&str> = unavailable.keys().map(String::as_str).collect();
    failed.sort();
    assert_eq!(
        failed,
        vec![
            "officers",
            "persons_with_significant_control",
            "recent_filings"
        ]
    );
    for (_, reason) in unavailable {
        assert!(
            reason.as_str().unwrap().starts_with("upstream: "),
            "{reason}"
        );
    }
    assert_eq!(v["charges"], json!({"total": 0}));
}

#[tokio::test]
async fn report_fails_when_the_company_does_not_exist() {
    let fake = fake().await;
    let s = server(&fake);
    let text = error_text(
        &s.company_report(params(json!({"company_number": "12345678"})), auth())
            .await
            .unwrap(),
    );
    assert!(
        text.starts_with("not_found: no company with number 12345678"),
        "{text}"
    );
    assert_eq!(fake.requests().len(), 1);
    let text = error_text(
        &s.company_report(
            params(json!({"company_number": "1", "filings": 51})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(text.starts_with("validation: "), "{text}");
}
