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

fn observations(v: &serde_json::Value) -> Vec<(String, String, String)> {
    v["observations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["check"].as_str().unwrap().to_string(),
                o["level"].as_str().unwrap().to_string(),
                o["detail"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[tokio::test]
async fn due_diligence_on_a_quiet_company_observes_nothing_but_never_clears_it() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .due_diligence_check(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["company"]["company_name"], "ACME WIDGETS LIMITED");
    assert_eq!(observations(&v), vec![]);
    let run: Vec<&str> = v["checks_run"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    for check in [
        "company_status",
        "accounts",
        "confirmation_statement",
        "registered_office",
        "company_age",
        "insolvency",
        "charges",
        "officers",
        "ownership",
    ] {
        assert!(run.contains(&check), "{check} missing from {run:?}");
    }
    let not_run = v["checks_not_run"].to_string();
    for check in [
        "sanctions",
        "politically_exposed_persons",
        "identity_verification",
    ] {
        assert!(not_run.contains(check), "{not_run}");
    }
    assert!(
        v["disclaimer"]
            .as_str()
            .unwrap()
            .contains("does not mean the company is sound"),
        "{v}"
    );
    assert_eq!(
        paths(&fake),
        vec![
            "/company/00000001",
            "/company/00000001/officers",
            "/company/00000001/persons-with-significant-control",
            "/company/00000001/persons-with-significant-control-statements",
        ]
    );
}

#[tokio::test]
async fn due_diligence_flags_liquidation_insolvency_and_no_directors() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .due_diligence_check(params(json!({"company_number": "6"})), auth())
            .await
            .unwrap(),
    );
    let found = observations(&v);
    let has = |check: &str, level: &str, text: &str| {
        found
            .iter()
            .any(|(c, l, d)| c == check && l == level && d.contains(text))
    };
    assert!(
        has("company_status", "flag", "status is liquidation"),
        "{found:?}"
    );
    assert!(
        has(
            "insolvency",
            "flag",
            "creditors-voluntary-liquidation (wound-up-on 2025-02-01)"
        ),
        "{found:?}"
    );
    assert!(
        has(
            "charges",
            "note",
            "charges not satisfied: 1, held by Big Bank PLC"
        ),
        "{found:?}"
    );
    assert!(has("officers", "flag", "no current directors"), "{found:?}");
    assert!(
        has("ownership", "note", "exempt from the PSC rules"),
        "{found:?}"
    );
}

#[tokio::test]
async fn due_diligence_flags_a_new_company_with_overdue_filings_and_no_owner() {
    let fake = fake().await;
    let created = chrono::Utc::now().date_naive() - chrono::Duration::days(90);
    {
        let mut st = fake.state();
        let mut p = st.companies["00000002"].clone();
        p["company_number"] = json!("00000009");
        p["date_of_creation"] = json!(created.to_string());
        p["accounts"]["next_accounts"]["overdue"] = json!(true);
        p["confirmation_statement"]["overdue"] = json!(true);
        p["registered_office_is_in_dispute"] = json!(true);
        p["company_status_detail"] = json!("active-proposal-to-strike-off");
        st.companies.insert("00000009".into(), p);
    }
    let v = json(
        &server(&fake)
            .due_diligence_check(params(json!({"company_number": "9"})), auth())
            .await
            .unwrap(),
    );
    let flags: Vec<String> = observations(&v)
        .into_iter()
        .filter(|(_, l, _)| l == "flag")
        .map(|(c, _, _)| c)
        .collect();
    assert_eq!(
        flags,
        vec![
            "company_status",
            "accounts",
            "confirmation_statement",
            "registered_office",
            "company_age",
            "officers",
            "ownership",
        ],
        "{v}"
    );
    let text = v["observations"].to_string();
    assert!(text.contains("active-proposal-to-strike-off"), "{text}");
    assert!(text.contains("may be a gap"), "{text}");
}

#[tokio::test]
async fn due_diligence_reports_checks_it_could_not_read() {
    let fake = fake().await;
    fake.state()
        .fail
        .push("/company/00000001/officers".to_string());
    let v = json(
        &server(&fake)
            .due_diligence_check(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    let run = v["checks_run"].as_array().unwrap();
    assert!(!run.iter().any(|c| c == "officers"), "{v}");
    assert!(run.iter().any(|c| c == "ownership"), "{v}");
    let failed = &v["checks_not_run"][0];
    assert_eq!(failed["check"], "officers");
    assert!(
        failed["reason"]
            .as_str()
            .unwrap()
            .starts_with("could not be read: upstream: "),
        "{failed}"
    );
}

#[tokio::test]
async fn due_diligence_needs_a_company() {
    let fake = fake().await;
    let e = error_text(
        &server(&fake)
            .due_diligence_check(params(json!({"company_number": "99999999"})), auth())
            .await
            .unwrap(),
    );
    assert!(
        e.starts_with("not_found: no company with number 99999999"),
        "{e}"
    );
}

#[tokio::test]
async fn due_diligence_flags_an_owner_the_register_marks_as_sanctioned() {
    let fake = fake().await;
    fake.state()
        .resources
        .get_mut("/company/00000001/persons-with-significant-control")
        .unwrap()["items"][0]["is_sanctioned"] = json!(true);
    let v = json(
        &server(&fake)
            .due_diligence_check(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(
        observations(&v),
        vec![(
            "ownership".to_string(),
            "flag".to_string(),
            "the register marks these owners as sanctioned: Mrs Jane Anne Smith".to_string()
        )]
    );
}
