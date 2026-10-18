mod support;
use serde_json::{Value, json};
use support::fake_ch::{FakeCh, FakeState, Injected};
use support::tools::{auth, error_text, json, params, server};

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
            .list_officers(params(json!({"company_number": "1"})), auth())
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
            "officer_id": "off-jane", "appointment_id": "apt-off-jane"
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
            .list_officers(
                params(json!({
                    "company_number": "00000001", "limit": 2, "order_by": "surname"
                })),
                auth(),
            )
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
            .list_officers(params(json!({"company_number": "12345678"})), auth())
            .await
            .unwrap(),
    );
    // The officers endpoint answers 200 with an empty list even for a number that does not
    // exist, so the tool checks the profile before saying "no officers".
    assert!(
        text.starts_with("not_found: no company with number 12345678"),
        "{text}"
    );
    let paths: Vec<String> = fake.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(
        paths,
        vec!["/company/12345678/officers", "/company/12345678"]
    );
}

#[tokio::test]
async fn a_company_without_officers_lists_none() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .list_officers(params(json!({"company_number": "5"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 0);
    assert_eq!(v["items"], json!([]));
}

#[tokio::test]
async fn psc_for_an_unknown_company_is_not_found() {
    let fake = fake().await;
    let text = error_text(
        &server(&fake)
            .list_persons_with_significant_control(
                params(json!({"company_number": "12345678"})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert!(
        text.starts_with("not_found: no company with number 12345678"),
        "{text}"
    );
}

#[tokio::test]
async fn unknown_disqualified_officers_are_not_found_although_the_api_answers_204() {
    let fake = fake().await;
    let text = error_text(
        &server(&fake)
            .get_disqualified_officer(
                params(json!({"officer_id": "nobody", "officer_kind": "natural"})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert!(text.starts_with("not_found: "), "{text}");
}

#[tokio::test]
async fn officer_search_pages_are_aligned() {
    let fake = fake().await;
    let s = server(&fake);
    // limit 1 is requested as a page of two, so the best match is not skipped.
    let v = json(
        &s.search_officers(params(json!({"query": "m", "limit": 1})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["items"][0]["officer_id"], "off-jane");
    assert_eq!(v["start_index"], 0);
    assert_eq!(v["next_start_index"], 1);
    let log = fake.requests();
    assert_eq!(log[0].param("items_per_page"), Some("2"));
    assert_eq!(log[0].param("start_index"), Some("0"));
    let v = json(
        &s.search_officers(
            params(json!({"query": "m", "limit": 1, "start_index": 1})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(v["items"][0]["officer_id"], "off-tom");
    assert_eq!(v["start_index"], 1);
    assert!(v.get("next_start_index").is_none(), "{v}");
}

#[tokio::test]
async fn officer_appointments_list_other_companies() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.get_officer_appointments(params(json!({"officer_id": "off-jane"})), auth())
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
        &s.get_officer_appointments(
            params(json!({"officer_id": "off-jane", "active_only": true})),
            auth(),
        )
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
        &s.get_officer_appointments(params(json!({"officer_id": "../company/1"})), auth())
            .await
            .unwrap(),
    );
    assert!(text.starts_with("validation: "), "{text}");
    let text = error_text(
        &s.get_officer_appointments(params(json!({"officer_id": "unknown-id"})), auth())
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
            .search_officers(params(json!({"query": "smith"})), auth())
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
        &s.list_persons_with_significant_control(params(json!({"company_number": "1"})), auth())
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
        &s.list_persons_with_significant_control(params(json!({"company_number": "5"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 0);
    assert_eq!(
        v["statements"],
        json!([{"statement": "no-individual-or-entity-with-signficant-control", "notified_on": "2022-02-02",
                "statement_id": "st1"}])
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
            .list_persons_with_significant_control(params(json!({"company_number": "1"})), auth())
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
            .list_persons_with_significant_control(
                params(json!({
                    "company_number": "1", "start_index": 1, "limit": 1
                })),
                auth(),
            )
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
        &s.search_disqualified_officers(params(json!({"query": "bloggs"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["officer_id"], "dq-john");
    assert_eq!(v["items"][0]["officer_kind"], "natural");

    let v = json(
        &s.get_disqualified_officer(
            params(json!({"officer_id": "dq-john", "officer_kind": "natural"})),
            auth(),
        )
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
        &s.get_disqualified_officer(params(json!({"officer_id": "dq-rogue"})), auth())
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
        &s.get_disqualified_officer(params(json!({"officer_id": "nobody"})), auth())
            .await
            .unwrap(),
    );
    assert!(text.starts_with("not_found: "), "{text}");
}

#[tokio::test]
async fn current_only_uses_the_active_filter() {
    let fake = fake().await;
    let srv = server(&fake);
    let v = json(
        &srv.list_officers(
            params(json!({"company_number": "1", "current_only": true})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(
        names(&v),
        vec!["SMITH, Jane Anne", "ACME SECRETARIES LIMITED"]
    );
    assert_eq!(v["total"], 2);
    assert_eq!(v["active_count"], 2);
    assert!(v.get("next_start_index").is_none(), "{v}");
    let sent = &fake.requests()[0];
    assert_eq!(sent.param("filter"), Some("active"));
    assert_eq!(sent.param("items_per_page"), Some("100"));

    // Pages of one walk the filtered list.
    let v = json(
        &srv.list_officers(
            params(json!({"company_number": "1", "current_only": true, "limit": 1})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(names(&v), vec!["SMITH, Jane Anne"]);
    assert_eq!(v["next_start_index"], 1);
    let v = json(
        &srv.list_officers(
            params(
                json!({"company_number": "1", "current_only": true, "limit": 1, "start_index": 1}),
            ),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(names(&v), vec!["ACME SECRETARIES LIMITED"]);
    assert!(v.get("next_start_index").is_none(), "{v}");

    // All resigned: an empty page, not an error; an unknown company is an error.
    let v = json(
        &srv.list_officers(
            params(json!({"company_number": "6", "current_only": true})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(v["count"], 0);
    let e = error_text(
        &srv.list_officers(
            params(json!({"company_number": "99999999", "current_only": true})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(e.starts_with("not_found: "), "{e}");
}

#[tokio::test]
async fn current_only_reads_the_whole_list_where_the_filter_is_refused_or_ignored() {
    let fake = fake().await;
    let srv = server(&fake);
    // Refused: the request is sent again without the filter and filtered here.
    fake.inject(Injected::json(
        400,
        json!({"errors": [{"error": "filter not supported"}]}),
    ));
    let v = json(
        &srv.list_officers(
            params(json!({"company_number": "1", "current_only": true})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(
        names(&v),
        vec!["SMITH, Jane Anne", "ACME SECRETARIES LIMITED"]
    );
    assert_eq!(v["officers_read"], 3);
    assert!(v.get("total").is_none(), "{v}");
    let sent = fake.requests();
    assert_eq!(sent[1].param("filter"), None);

    // Ignored: resigned officers come back anyway and are dropped here.
    let ignored = json!({
        "total_results": 2, "active_count": 1, "resigned_count": 1,
        "items": [
            {"name": "OLD, Director", "officer_role": "director", "resigned_on": "2020-01-01"},
            {"name": "NEW, Director", "officer_role": "director"}
        ]
    });
    fake.inject(Injected::json(200, ignored));
    let v = json(
        &srv.list_officers(
            params(json!({"company_number": "1", "current_only": true})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(names(&v), vec!["NEW, Director"]);
    assert!(v.get("total").is_none(), "{v}");
}

#[tokio::test]
async fn a_closed_company_has_no_officers_in_post() {
    let fake = fake().await;
    fake.state().resources.insert(
        "/company/SC000003/officers".into(),
        json!({
            "total_results": 1, "active_count": 0, "resigned_count": 0, "inactive_count": 1,
            "items": [{"name": "MACLEOD, Ian", "officer_role": "director", "appointed_on": "2010-01-10"}]
        }),
    );
    let v = json(
        &server(&fake)
            .list_officers(
                params(json!({"company_number": "SC000003", "current_only": true})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(v["count"], 0, "{v}");
    assert!(v["note"].as_str().unwrap().contains("closed"), "{v}");
}

#[tokio::test]
async fn role_limits_the_list_to_one_kind_of_officer() {
    let fake = fake().await;
    let srv = server(&fake);
    let v = json(
        &srv.list_officers(
            params(json!({"company_number": "1", "role": "secretaries"})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(names(&v), vec!["ACME SECRETARIES LIMITED"]);
    assert_eq!(v["officers_read"], 3);
    assert_eq!(fake.requests()[0].param("filter"), None);
    let v = json(
        &srv.list_officers(
            params(json!({"company_number": "1", "role": "directors", "current_only": true})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(names(&v), vec!["SMITH, Jane Anne"]);
    assert!(
        v.get("total").is_none(),
        "a role filter has no upstream total: {v}"
    );
}

#[tokio::test]
async fn officers_carry_appointment_ids_and_identity_verification() {
    let fake = fake().await;
    fake.state()
        .resources
        .get_mut("/company/00000001/officers")
        .unwrap()["items"][0]["identity_verification_details"] = json!({"appointment_verification_start_on": "2025-11-18", "appointment_verification_statement_due_on": "2025-12-02"});
    let v = json(
        &server(&fake)
            .list_officers(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["items"][0]["appointment_id"], "apt-off-jane");
    assert_eq!(
        v["items"][0]["identity_verification"]["appointment_verification_start_on"],
        "2025-11-18"
    );
    assert!(v["items"][1].get("identity_verification").is_none());
}

#[tokio::test]
async fn officer_network_splits_current_and_past_companies() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .officer_network(params(json!({"officer_id": "off-jane"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["name"], "Jane Anne SMITH");
    assert_eq!(v["date_of_birth"], "1980-04");
    assert_eq!(v["total_appointments"], 3);
    assert_eq!(v["appointments_read"], 3);
    assert_eq!(v["companies"], 3);
    assert_eq!(
        v["company_statuses"],
        json!({"active": 2, "liquidation": 1})
    );
    assert_eq!(v["current_count"], 2);
    assert_eq!(v["past_count"], 1);
    assert_eq!(v["past"][0]["company_number"], "00000006");
    assert!(v.get("note").is_none(), "{v}");
    assert_eq!(fake.requests()[0].param("items_per_page"), Some("100"));
}

#[tokio::test]
async fn officer_network_takes_a_name_that_matches_one_officer() {
    let fake = fake().await;
    let srv = server(&fake);
    let v = json(
        &srv.officer_network(params(json!({"officer_name": "jane anne smith"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["officer_id"], "off-jane");
    let paths: Vec<String> = fake.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(
        paths,
        vec!["/search/officers", "/officers/off-jane/appointments"]
    );

    // Not exactly anyone: the closest matches are listed.
    let e = error_text(
        &srv.officer_network(params(json!({"officer_name": "Jane"})), auth())
            .await
            .unwrap(),
    );
    assert!(
        e.starts_with("validation: no officer is named exactly Jane"),
        "{e}"
    );
    assert!(e.contains("off-jane (Jane Anne SMITH, born 1980-04"), "{e}");

    // Several people with the name: refuse and list them.
    let twin = |id: &str| {
        json!({"title": "Jane Anne SMITH", "appointment_count": 1,
               "links": {"self": format!("/officers/{id}/appointments")}})
    };
    fake.inject(Injected::json(
        200,
        json!({"items": [twin("off-jane"), twin("off-jane-2")], "total_results": 2, "start_index": 0}),
    ));
    let e = error_text(
        &srv.officer_network(params(json!({"officer_name": "SMITH, Jane Anne"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.starts_with("validation: 2 officers are named"), "{e}");
    assert!(e.contains("off-jane-2"), "{e}");

    let e = error_text(
        &srv.officer_network(params(json!({"officer_name": "Nobody Atall"})), auth())
            .await
            .unwrap(),
    );
    assert!(
        e.starts_with("not_found: no officer is named Nobody Atall"),
        "{e}"
    );
    let e = error_text(
        &srv.officer_network(params(json!({})), auth())
            .await
            .unwrap(),
    );
    assert!(e.contains("pass officer_id"), "{e}");
}

#[tokio::test]
async fn officer_network_reads_every_page_up_to_its_limit() {
    let fake = fake().await;
    let page = |start: usize, n: usize, total: usize| {
        let items: Vec<Value> = (start..start + n)
            .map(|i| {
                json!({"appointed_to": {"company_number": format!("{i:08}"), "company_status": "active"},
                       "officer_role": "director"})
            })
            .collect();
        json!({"name": "Busy PERSON", "items": items, "total_results": total, "start_index": start})
    };
    fake.inject(Injected::json(200, page(0, 100, 150)));
    fake.inject(Injected::json(200, page(100, 50, 150)));
    let v = json(
        &server(&fake)
            .officer_network(params(json!({"officer_id": "off-busy"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["appointments_read"], 150);
    assert_eq!(v["current_count"], 150);
    assert!(v.get("note").is_none(), "{v}");
    let starts: Vec<Option<String>> = fake
        .requests()
        .iter()
        .map(|r| r.param("start_index").map(str::to_string))
        .collect();
    assert_eq!(starts, vec![Some("0".into()), Some("100".into())]);

    let fake = support::fake_ch::FakeCh::start(FakeState::register()).await;
    for p in 0..10 {
        fake.inject(Injected::json(200, page(p * 100, 100, 1500)));
    }
    let v = json(
        &server(&fake)
            .officer_network(params(json!({"officer_id": "off-busy"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["appointments_read"], 1000);
    assert_eq!(fake.requests().len(), 10);
    assert!(
        v["note"].as_str().unwrap().contains("first 1000 of 1500"),
        "{v}"
    );

    let fake = support::fake_ch::FakeCh::start(FakeState::register()).await;
    let e = error_text(
        &server(&fake)
            .officer_network(params(json!({"officer_id": "off-nobody"})), auth())
            .await
            .unwrap(),
    );
    assert!(
        e.starts_with("not_found: no officer with id off-nobody"),
        "{e}"
    );
}

#[tokio::test]
async fn no_pscs_are_explained_by_an_exemption_or_called_a_gap() {
    let fake = fake().await;
    let srv = server(&fake);
    let v = json(
        &srv.list_persons_with_significant_control(params(json!({"company_number": "6"})), auth())
            .await
            .unwrap(),
    );
    let note = v["note"].as_str().unwrap();
    assert!(
        note.contains("exempt from the PSC rules: psc-exempt-as-trading-on-regulated-market"),
        "{note}"
    );
    let v = json(
        &srv.list_persons_with_significant_control(params(json!({"company_number": "2"})), auth())
            .await
            .unwrap(),
    );
    assert!(v["note"].as_str().unwrap().contains("may be a gap"), "{v}");

    // Listed PSCs need no note.
    let v = json(
        &srv.list_persons_with_significant_control(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert!(v.get("note").is_none(), "{v}");
}
