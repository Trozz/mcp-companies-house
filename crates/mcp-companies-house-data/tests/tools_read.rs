mod support;
use serde_json::{Value, json};
use support::fake_ch::{FakeCh, FakeState, Injected, now_unix};
use support::tools::{auth, auth_with, error_text, json, no_auth, params, server};

async fn fake() -> FakeCh {
    FakeCh::start(FakeState::register()).await
}

fn numbers(v: &Value) -> Vec<&str> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["company_number"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn search_companies_returns_compact_matches() {
    let fake = fake().await;
    let r = server(&fake)
        .search_companies(params(json!({"query": "acme"})), auth())
        .await
        .unwrap();
    let v = json(&r);
    assert_eq!(v["total"], 3);
    assert_eq!(v["count"], 3);
    assert_eq!(v["start_index"], 0);
    assert!(v.get("next_start_index").is_none(), "{v}");
    assert_eq!(
        v["items"][0],
        json!({
            "company_number": "00000001",
            "company_name": "ACME WIDGETS LIMITED",
            "company_status": "active",
            "company_type": "ltd",
            "date_of_creation": "2015-03-01",
            "address": "1, Park Row, Leeds, LS1 5AB, England"
        })
    );
    let log = fake.requests();
    assert_eq!(log[0].path, "/search/companies");
    assert_eq!(log[0].param("q"), Some("acme"));
    assert_eq!(log[0].param("items_per_page"), Some("20"));
    assert_eq!(log[0].param("start_index"), Some("0"));
    assert_eq!(log[0].param("restrictions"), None);
}

#[tokio::test]
async fn search_companies_pages_and_filters_to_active() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.search_companies(params(json!({"query": "acme", "limit": 2})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(numbers(&v), vec!["00000001", "00000002"]);
    assert_eq!(v["next_start_index"], 2);
    let v = json(
        &s.search_companies(
            params(json!({"query": "acme", "limit": 2, "start_index": 2})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(numbers(&v), vec!["00000006"]);
    assert!(v.get("next_start_index").is_none(), "{v}");

    let v = json(
        &s.search_companies(
            params(json!({"query": "acme", "active_only": true})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(numbers(&v), vec!["00000001", "00000002"]);
    let last = fake.requests().pop().unwrap();
    assert_eq!(last.param("restrictions"), Some("active-companies"));
}

#[tokio::test]
async fn search_companies_shows_cessation_dates() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .search_companies(params(json!({"query": "thistle"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["items"][0]["company_status"], "dissolved");
    assert_eq!(v["items"][0]["date_of_cessation"], "2020-05-05");
}

#[tokio::test]
async fn search_companies_validates_before_calling() {
    let fake = fake().await;
    let s = server(&fake);
    for (args, msg) in [
        (json!({"query": "   "}), "query must not be empty"),
        (
            json!({"query": "acme", "limit": 0}),
            "limit must be between 1 and 100",
        ),
        (
            json!({"query": "acme", "limit": 500}),
            "limit must be between 1 and 100",
        ),
        (
            json!({"query": "x".repeat(201)}),
            "query is limited to 200 characters",
        ),
    ] {
        let r = s
            .search_companies(params(args.clone()), auth())
            .await
            .unwrap();
        let text = error_text(&r);
        assert!(text.starts_with("validation: "), "{args}: {text}");
        assert!(text.contains(msg), "{args}: {text}");
    }
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn filter_companies_combines_filters() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.filter_companies(
            params(json!({
                "location": "leeds",
                "status": ["active"],
                "company_type": ["ltd"],
            })),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(v["total"], 3);
    assert_eq!(numbers(&v), vec!["00000001", "00000002", "00000005"]);
    assert_eq!(
        v["items"][1],
        json!({
            "company_number": "00000002",
            "company_name": "ACME BAKERY LIMITED",
            "company_status": "active",
            "company_type": "ltd",
            "date_of_creation": "2021-06-15",
            "address": "Unit 4, Kirkstall Road, Leeds, LS3 1LH, England",
            "sic_codes": ["10710", "47240"]
        })
    );
    let log = fake.requests();
    assert_eq!(log[0].path, "/advanced-search/companies");
    assert_eq!(log[0].param("location"), Some("leeds"));
    assert_eq!(log[0].param("company_status"), Some("active"));
    assert_eq!(log[0].param("company_type"), Some("ltd"));
    assert_eq!(log[0].param("size"), Some("20"));
    assert_eq!(log[0].param("start_index"), Some("0"));
}

#[tokio::test]
async fn filter_companies_by_sic_code_subtype_and_dates() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.filter_companies(params(json!({"sic_codes": ["62012", "62020"]})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(numbers(&v), vec!["00000001", "SC000003"]);
    assert_eq!(
        fake.requests().pop().unwrap().param("sic_codes"),
        Some("62012,62020")
    );

    let v = json(
        &s.filter_companies(
            params(json!({"subtype": ["community-interest-company"]})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(numbers(&v), vec!["00000005"]);
    assert_eq!(
        v["items"][0]["company_subtype"],
        "community-interest-company"
    );

    let v = json(
        &s.filter_companies(
            params(json!({
                "incorporated_from": "2018-01-01",
                "incorporated_to": "2021-12-31"
            })),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(numbers(&v), vec!["00000002", "OC000004"]);

    let v = json(
        &s.filter_companies(
            params(json!({
                "status": ["dissolved"],
                "dissolved_from": "2020-01-01",
                "dissolved_to": "2020-12-31"
            })),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(numbers(&v), vec!["SC000003"]);
    assert_eq!(v["items"][0]["date_of_cessation"], "2020-05-05");
}

#[tokio::test]
async fn filter_companies_counts_with_limit_one_and_pages() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.filter_companies(params(json!({"name_includes": "acme", "limit": 1})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["total"], 3);
    assert_eq!(v["count"], 1);
    assert_eq!(v["next_start_index"], 1);
    let v = json(
        &s.filter_companies(
            params(json!({
                "name_includes": "acme", "name_excludes": "bakery", "limit": 1, "start_index": 1
            })),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(v["total"], 2);
    assert_eq!(numbers(&v), vec!["00000006"]);
    assert!(v.get("next_start_index").is_none(), "{v}");
}

#[tokio::test]
async fn filter_companies_with_no_matches_is_an_empty_page() {
    let fake = fake().await;
    let r = server(&fake)
        .filter_companies(params(json!({"location": "Aberystwyth"})), auth())
        .await
        .unwrap();
    assert_eq!(
        json(&r),
        json!({"total": 0, "start_index": 0, "count": 0, "items": []})
    );
}

#[tokio::test]
async fn filter_companies_rejects_bad_arguments_before_calling() {
    let fake = fake().await;
    let s = server(&fake);
    for (args, msg) in [
        (json!({}), "at least one filter"),
        (json!({"incorporated_from": "2024-13-01"}), "YYYY-MM-DD"),
        (json!({"sic_codes": ["software"]}), "SIC 2007"),
        (json!({"location": "Leeds", "limit": 101}), "limit"),
    ] {
        let r = s
            .filter_companies(params(args.clone()), auth())
            .await
            .unwrap();
        let text = error_text(&r);
        assert!(text.starts_with("validation: "), "{args}: {text}");
        assert!(text.contains(msg), "{args}: {text}");
    }
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn get_company_returns_the_profile_without_noise() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .get_company(params(json!({"company_number": "6"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(fake.requests()[0].path, "/company/00000006");
    assert_eq!(v["company_name"], "ACME HOLDINGS PLC");
    assert_eq!(v["company_status"], "liquidation");
    assert_eq!(v["type"], "plc");
    assert_eq!(v["registered_office_address"]["postal_code"], "EC2R 8EJ");
    assert!(
        v["registered_office_address"].get("po_box").is_none(),
        "{v}"
    );
    assert_eq!(v["accounts"]["next_due"], "2027-12-31");
    assert_eq!(
        v["records_available"],
        json!(["charges", "filing_history", "insolvency", "officers"])
    );
    for gone in ["etag", "links", "has_charges", "has_insolvency_history"] {
        assert!(v.get(gone).is_none(), "{gone} still present: {v}");
    }
}

#[tokio::test]
async fn get_company_keeps_previous_names() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .get_company(params(json!({"company_number": "sc 000003"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["company_number"], "SC000003");
    assert_eq!(v["jurisdiction"], "scotland");
    assert_eq!(v["previous_company_names"][0]["name"], "THISTLE CODE LTD");
}

#[tokio::test]
async fn get_company_reports_unknown_and_invalid_numbers() {
    let fake = fake().await;
    let s = server(&fake);
    let text = error_text(
        &s.get_company(params(json!({"company_number": "12345678"})), auth())
            .await
            .unwrap(),
    );
    assert!(text.starts_with("not_found: "), "{text}");
    assert!(text.contains("12345678"), "{text}");
    assert!(text.contains("search_companies"), "{text}");
    let text = error_text(
        &s.get_company(params(json!({"company_number": "../search"})), auth())
            .await
            .unwrap(),
    );
    assert!(text.starts_with("validation: "), "{text}");
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn upstream_failures_are_tool_errors_with_a_kind() {
    let fake = fake().await;
    let text = error_text(
        &server(&fake)
            .search_companies(params(json!({"query": "acme"})), auth_with("wrong-key"))
            .await
            .unwrap(),
    );
    assert!(text.starts_with("auth: "), "{text}");
    assert!(text.contains("Sign in again"), "{text}");

    fake.inject(Injected::json(429, json!({})).header("retry-after", "60"));
    let s = server(&fake);
    let text = error_text(
        &s.get_company(params(json!({"company_number": "00000001"})), auth())
            .await
            .unwrap(),
    );
    assert!(text.starts_with("rate_limited: "), "{text}");
    assert!(text.contains("600 requests per 5 minutes"), "{text}");

    let fake = FakeCh::start(FakeState::register()).await;
    fake.inject(Injected::status(500));
    fake.inject(Injected::status(500));
    let text = error_text(
        &server(&fake)
            .filter_companies(params(json!({"location": "leeds"})), auth())
            .await
            .unwrap(),
    );
    assert!(text.starts_with("upstream: "), "{text}");
}

#[tokio::test]
async fn a_used_up_rate_limit_stops_calls_until_the_reset() {
    let fake = fake().await;
    {
        let mut st = fake.state();
        st.rate_remaining = 1;
        st.rate_reset_unix = now_unix() + 60;
    }
    let s = server(&fake);
    // This call uses the last request of the window (remaining becomes 0).
    json(
        &s.get_company(params(json!({"company_number": "00000001"})), auth())
            .await
            .unwrap(),
    );
    let text = error_text(
        &s.search_companies(params(json!({"query": "acme"})), auth())
            .await
            .unwrap(),
    );
    assert!(text.starts_with("rate_limited: "), "{text}");
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn search_with_limit_one_returns_the_best_match_first() {
    let fake = fake().await;
    let s = server(&fake);
    let v = json(
        &s.search_companies(params(json!({"query": "acme", "limit": 1})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(numbers(&v), vec!["00000001"]);
    assert_eq!(v["start_index"], 0);
    assert_eq!(v["next_start_index"], 1);
    assert_eq!(fake.requests()[0].param("items_per_page"), Some("2"));
    let v = json(
        &s.search_companies(
            params(json!({"query": "acme", "limit": 1, "start_index": 1})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(numbers(&v), vec!["00000002"]);
    assert_eq!(v["next_start_index"], 2);
}

#[tokio::test]
async fn an_unaligned_start_returns_the_rest_of_its_page() {
    let fake = fake().await;
    // Companies House serves pages aligned to items_per_page: asking for 2 from index 1 reads
    // the page [0, 2) and returns its second item, labelled with the real offset.
    let v = json(
        &server(&fake)
            .search_companies(
                params(json!({"query": "acme", "limit": 2, "start_index": 1})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(numbers(&v), vec!["00000002"]);
    assert_eq!(v["start_index"], 1);
    assert_eq!(v["next_start_index"], 2);
    let log = fake.requests();
    assert_eq!(log[0].param("start_index"), Some("0"));
    assert_eq!(log[0].param("items_per_page"), Some("2"));
}

#[tokio::test]
async fn search_stops_at_the_companies_house_window() {
    let fake = fake().await;
    let s = server(&fake);
    let text = error_text(
        &s.search_companies(
            params(json!({"query": "acme", "start_index": 1000})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert!(text.starts_with("validation: "), "{text}");
    assert!(text.contains("first 1000 matches"), "{text}");
    assert!(fake.requests().is_empty());

    // A page ending at result 1000 offers no next page, and says why.
    let items: Vec<Value> = (0..20)
        .map(|i| json!({"title": format!("CO {i}"), "company_number": format!("{i:08}")}))
        .collect();
    fake.inject(Injected::json(
        200,
        json!({"kind": "search#companies", "total_results": 5000, "start_index": 980, "items_per_page": 20, "items": items}),
    ));
    let v = json(
        &s.search_companies(params(json!({"query": "co", "start_index": 980})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["count"], 20);
    assert_eq!(v["total"], 5000);
    assert!(v.get("next_start_index").is_none(), "{v}");
    assert!(v["note"].as_str().unwrap().contains("first 1000 matches"));

    // Near the window pages stay aligned to the limit, and are shortened only when the aligned
    // page would pass result 1000 (which Companies House refuses).
    for (start, limit, sent_start, sent_size) in [
        (990, 20, "980", "20"),
        (985, 20, "980", "20"),
        (993, 10, "990", "10"),
        (901, 100, "900", "100"),
        (995, 30, "995", "5"),
        (999, 1, "998", "2"),
    ] {
        let v = json(
            &s.search_companies(
                params(json!({"query": "acme", "start_index": start, "limit": limit})),
                auth(),
            )
            .await
            .unwrap(),
        );
        let last = fake.requests().pop().unwrap();
        assert_eq!(
            (last.param("start_index"), last.param("items_per_page")),
            (Some(sent_start), Some(sent_size)),
            "start {start}, limit {limit}: {v}"
        );
    }
}

#[tokio::test]
async fn pages_near_result_1000_are_full() {
    let fake = fake().await;
    let s = server(&fake);
    let items = |from: usize, n: usize| -> Vec<Value> {
        (from..from + n)
            .map(|i| json!({"title": format!("CO {i}"), "company_number": format!("{i:08}")}))
            .collect()
    };
    // Companies House serves the page aligned to the request; the tool returns the rest of it.
    fake.inject(Injected::json(
        200,
        json!({"kind": "search#companies", "total_results": 5000, "start_index": 980, "items_per_page": 20, "items": items(980, 20)}),
    ));
    let v = json(
        &s.search_companies(
            params(json!({"query": "co", "start_index": 985, "limit": 20})),
            auth(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(v["start_index"], 985);
    assert_eq!(v["count"], 15, "{v}");
    assert_eq!(v["items"][0]["company_number"], "00000985");
}

#[tokio::test]
async fn each_call_uses_the_callers_own_key() {
    let fake = fake().await;
    let s = server(&fake);
    json(
        &s.get_company(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    let text = error_text(
        &s.get_company(
            params(json!({"company_number": "1"})),
            auth_with("someone-elses-key"),
        )
        .await
        .unwrap(),
    );
    assert!(text.starts_with("auth: "), "{text}");
    let log = fake.requests();
    assert_ne!(log[0].authorization, log[1].authorization);
}

#[tokio::test]
async fn a_call_without_a_key_is_refused_before_calling() {
    let fake = fake().await;
    let text = error_text(
        &server(&fake)
            .get_company(params(json!({"company_number": "1"})), no_auth())
            .await
            .unwrap(),
    );
    assert!(
        text.starts_with("auth: this request carried no Companies House API key"),
        "{text}"
    );
    assert!(fake.requests().is_empty());
}

#[tokio::test]
async fn one_callers_used_up_rate_limit_does_not_block_another() {
    let fake = fake().await;
    let s = server(&fake);
    fake.inject(Injected::json(429, json!({})).header("retry-after", "60"));
    let text = error_text(
        &s.get_company(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert!(text.starts_with("rate_limited: "), "{text}");
    // The same caller is now held back locally; a different caller's key still works upstream
    // (the fake rejects it, which proves the request was sent).
    let text = error_text(
        &s.get_company(params(json!({"company_number": "1"})), auth())
            .await
            .unwrap(),
    );
    assert!(text.starts_with("rate_limited: "), "{text}");
    let before = fake.requests().len();
    let text = error_text(
        &s.get_company(
            params(json!({"company_number": "1"})),
            auth_with("other-key"),
        )
        .await
        .unwrap(),
    );
    assert!(text.starts_with("auth: "), "{text}");
    assert_eq!(fake.requests().len(), before + 1);
}
