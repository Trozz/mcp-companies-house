//! Canned Companies House resources for the fake (officers, appointments, PSCs, filings, charges,
//! insolvency, disqualifications) and the searches that read them, shaped like the live API.
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn officer(
    name: &str,
    role: &str,
    appointed: &str,
    resigned: Option<&str>,
    id: &str,
    number: &str,
) -> Value {
    let mut o = json!({
        "name": name,
        "officer_role": role,
        "appointed_on": appointed,
        "occupation": "Engineer",
        "nationality": "British",
        "country_of_residence": "England",
        "date_of_birth": {"month": 4, "year": 1980},
        "address": {"premises": "1", "address_line_1": "Park Row", "locality": "Leeds", "postal_code": "LS1 5AB", "country": "England"},
        "links": {
            "self": format!("/company/{number}/appointments/apt-{id}"),
            "officer": {"appointments": format!("/officers/{id}/appointments")}
        },
        "etag": "officer-etag"
    });
    if let Some(r) = resigned {
        o["resigned_on"] = json!(r);
    }
    o
}

fn appointment(
    number: &str,
    name: &str,
    status: &str,
    appointed: &str,
    resigned: Option<&str>,
) -> Value {
    let mut a = json!({
        "appointed_to": {"company_number": number, "company_name": name, "company_status": status},
        "officer_role": "director",
        "appointed_on": appointed,
        "name": "Jane Anne SMITH",
        "occupation": "Engineer",
        "links": {"company": format!("/company/{number}")}
    });
    if let Some(r) = resigned {
        a["resigned_on"] = json!(r);
    }
    a
}

fn filing(
    date: &str,
    kind: &str,
    category: &str,
    description: &str,
    values: Value,
    id: &str,
) -> Value {
    json!({
        "date": date,
        "type": kind,
        "category": category,
        "description": description,
        "description_values": values,
        "transaction_id": id,
        "pages": 3,
        "barcode": "XBARCODE",
        "links": {
            "self": format!("/company/00000001/filing-history/{id}"),
            "document_metadata": format!("https://frontend-doc-api.company-information.service.gov.uk/document/doc-{id}")
        }
    })
}

fn charge(id: &str, status: &str, created: &str, satisfied: Option<&str>) -> Value {
    let mut c = json!({
        "id": id,
        "charge_code": format!("0000000600{id}"),
        "charge_number": 1,
        "status": status,
        "classification": {"type": "charge-description", "description": "A registered charge"},
        "created_on": created,
        "delivered_on": created,
        "persons_entitled": [{"name": "Big Bank PLC"}],
        "particulars": {"type": "brief-description", "description": "Land at 1 Poultry", "contains_fixed_charge": true},
        "secured_details": {"type": "amount-secured", "description": "All monies due or to become due"},
        "transactions": [{"filing_type": "create-charge-with-deed", "delivered_on": created}],
        "etag": "charge-etag",
        "links": {"self": format!("/company/00000006/charges/{id}")}
    });
    if let Some(s) = satisfied {
        c["satisfied_on"] = json!(s);
    }
    c
}

/// Resources by request path.
pub fn fixtures() -> BTreeMap<String, Value> {
    let mut r = BTreeMap::new();
    r.insert(
        "/company/00000001/officers".into(),
        json!({
            "kind": "officer-list", "etag": "list-etag", "items_per_page": 35, "start_index": 0,
            "total_results": 3, "active_count": 2, "resigned_count": 1,
            "links": {"self": "/company/00000001/officers"},
            "items": [
                officer("SMITH, Jane Anne", "director", "2015-03-01", None, "off-jane", "00000001"),
                officer("BROWN, Tom", "director", "2015-03-01", Some("2019-12-31"), "off-tom", "00000001"),
                {
                    "name": "ACME SECRETARIES LIMITED", "officer_role": "corporate-secretary",
                    "appointed_on": "2016-01-01",
                    "identification": {"identification_type": "uk-limited-company", "registration_number": "00000099"},
                    "address": {"address_line_1": "2 Park Row", "locality": "Leeds", "postal_code": "LS1 5AB"},
                    "links": {"self": "/company/00000001/appointments/apt-sec",
                              "officer": {"appointments": "/officers/off-acmesec/appointments"}}
                }
            ]
        }),
    );
    r.insert(
        "/company/00000002/officers".into(),
        json!({
            "kind": "officer-list", "total_results": 1, "active_count": 1, "resigned_count": 0,
            "items": [officer("SMITH, Jane Anne", "director", "2021-06-15", None, "off-jane", "00000002")]
        }),
    );
    r.insert(
        "/company/00000006/officers".into(),
        json!({
            "kind": "officer-list", "total_results": 1, "active_count": 0, "resigned_count": 1,
            "items": [officer("SMITH, Jane Anne", "director", "1999-11-11", Some("2010-01-01"), "off-jane", "00000006")]
        }),
    );
    r.insert(
        "/officers/off-jane/appointments".into(),
        json!({
            "kind": "personal-appointment", "etag": "e", "name": "Jane Anne SMITH",
            "date_of_birth": {"month": 4, "year": 1980}, "is_corporate_officer": false,
            "items_per_page": 35, "start_index": 0, "total_results": 3,
            "links": {"self": "/officers/off-jane/appointments"},
            "items": [
                appointment("00000001", "ACME WIDGETS LIMITED", "active", "2015-03-01", None),
                appointment("00000002", "ACME BAKERY LIMITED", "active", "2021-06-15", None),
                appointment("00000006", "ACME HOLDINGS PLC", "liquidation", "1999-11-11", Some("2010-01-01")),
            ]
        }),
    );
    r.insert(
        "/company/00000001/persons-with-significant-control".into(),
        json!({
            "items_per_page": 25, "start_index": 0, "total_results": 2, "active_count": 1, "ceased_count": 1,
            "links": {"self": "/company/00000001/persons-with-significant-control"},
            "items": [
                {
                    "name": "Mrs Jane Anne Smith", "kind": "individual-person-with-significant-control",
                    "notified_on": "2016-04-06", "nationality": "British", "country_of_residence": "England",
                    "date_of_birth": {"month": 4, "year": 1980},
                    "natures_of_control": ["ownership-of-shares-75-to-100-percent", "voting-rights-75-to-100-percent"],
                    "address": {"premises": "1", "address_line_1": "Park Row", "locality": "Leeds", "postal_code": "LS1 5AB"},
                    "etag": "psc-etag", "links": {"self": "/company/00000001/persons-with-significant-control/individual/psc1"}
                },
                {
                    "name": "Mr Tom Brown", "kind": "individual-person-with-significant-control",
                    "notified_on": "2016-04-06", "ceased_on": "2019-12-31",
                    "natures_of_control": ["ownership-of-shares-25-to-50-percent"],
                    "links": {"self": "/company/00000001/persons-with-significant-control/individual/psc2"}
                }
            ]
        }),
    );
    r.insert(
        "/company/00000005/persons-with-significant-control-statements".into(),
        json!({
            "items_per_page": 25, "start_index": 0, "total_results": 1, "active_count": 1, "ceased_count": 0,
            "items": [{
                "kind": "persons-with-significant-control-statement",
                "statement": "no-individual-or-entity-with-signficant-control",
                "notified_on": "2022-02-02", "etag": "s-etag",
                "links": {"self": "/company/00000005/persons-with-significant-control-statements/st1"}
            }]
        }),
    );
    r.insert(
        "/company/00000001/filing-history".into(),
        json!({
            "kind": "filing-history", "etag": "fh-etag", "filing_history_status": "filing-history-available",
            "items_per_page": 25, "start_index": 0, "total_count": 4,
            "items": [
                filing("2024-12-20", "AA", "accounts", "accounts-with-accounts-type-micro-entity", json!({"made_up_date": "2024-03-31"}), "tx4"),
                filing("2024-06-10", "CS01", "confirmation-statement", "confirmation-statement-with-no-updates", json!({"made_up_date": "2024-05-31"}), "tx3"),
                filing("2019-12-31", "TM01", "officers", "termination-director-company-with-name-termination-date", json!({"officer_name": "Tom Brown", "termination_date": "2019-12-31"}), "tx2"),
                filing("2015-03-01", "NEWINC", "incorporation", "incorporation-company", json!({}), "tx1"),
            ]
        }),
    );
    r.insert(
        "/company/00000006/charges".into(),
        json!({
            "etag": "c-etag", "total_count": 2, "unfiltered_count": 2, "satisfied_count": 1, "part_satisfied_count": 0,
            "items": [
                charge("ch-1", "outstanding", "2019-01-02", None),
                charge("ch-2", "fully-satisfied", "2001-05-05", Some("2005-05-05")),
            ]
        }),
    );
    r.insert(
        "/company/00000006/charges/ch-1".into(),
        charge("ch-1", "outstanding", "2019-01-02", None),
    );
    r.insert(
        "/company/00000006/insolvency".into(),
        json!({
            "etag": "i-etag", "status": "liquidation",
            "cases": [{
                "type": "creditors-voluntary-liquidation", "number": "1",
                "dates": [{"type": "wound-up-on", "date": "2025-02-01"}],
                "practitioners": [{
                    "name": "A Liquidator", "role": "practitioner", "appointed_on": "2025-02-01",
                    "address": {"address_line_1": "1 Way", "locality": "York", "postal_code": "YO1 1AA"}
                }],
                "links": {"charge": "/company/00000006/charges/ch-1"}
            }]
        }),
    );
    r.insert(
        "/disqualified-officers/natural/dq-john".into(),
        json!({
            "kind": "natural-disqualification", "etag": "d-etag", "forename": "John", "surname": "BLOGGS",
            "date_of_birth": "1970-03-15", "nationality": "British",
            "links": {"self": "/disqualified-officers/natural/dq-john"},
            "disqualifications": [{
                "disqualification_type": "court-order", "disqualified_from": "2020-01-01",
                "disqualified_until": "2026-01-01", "court_name": "Leeds County Court",
                "company_names": ["BLOGGS TRADING LIMITED"],
                "reason": {"description_identifier": "misconduct", "act": "company-directors-disqualification-act-1986", "article": "6"},
                "address": {"address_line_1": "3 Lane", "locality": "Hull", "postal_code": "HU1 1AA"}
            }]
        }),
    );
    r.insert(
        "/disqualified-officers/corporate/dq-rogue".into(),
        json!({
            "kind": "corporate-disqualification", "name": "ROGUE TRADING LIMITED", "company_number": "09999999",
            "links": {"self": "/disqualified-officers/corporate/dq-rogue"},
            "disqualifications": [{"disqualification_type": "undertaking", "disqualified_from": "2021-01-01",
                                   "disqualified_until": "2027-01-01", "reason": {"description_identifier": "misconduct"},
                                   "address": {"locality": "London"}}]
        }),
    );
    r
}

fn param<'a>(query: &'a [(String, String)], name: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn number(query: &[(String, String)], name: &str, default: usize) -> usize {
    param(query, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A canned resource, with `filter=active` (appointments) and `category` (filing history)
/// applied and its items paged by `items_per_page` and `start_index`.
pub fn canned(
    resources: &BTreeMap<String, Value>,
    path: &str,
    query: &[(String, String)],
) -> Option<Value> {
    let mut v = resources.get(path)?.clone();
    let Some(items) = v.get("items").and_then(Value::as_array).cloned() else {
        return Some(v);
    };
    let mut items = items;
    let mut filtered = false;
    if param(query, "filter") == Some("active") {
        items.retain(|i| i.get("resigned_on").is_none());
        filtered = true;
    }
    if let Some(categories) = param(query, "category") {
        let wanted: Vec<&str> = categories.split(',').collect();
        items.retain(|i| i["category"].as_str().is_some_and(|c| wanted.contains(&c)));
        filtered = true;
    }
    if filtered {
        for key in ["total_results", "total_count"] {
            if v.get(key).is_some() {
                v[key] = json!(items.len());
            }
        }
    }
    let start = number(query, "start_index", 0);
    let size = number(query, "items_per_page", 35);
    v["items"] = Value::Array(items.into_iter().skip(start).take(size).collect());
    v["start_index"] = json!(start);
    v["items_per_page"] = json!(size);
    Some(v)
}

/// The offset and page size a live `/search/*` endpoint actually serves: pages aligned to
/// `items_per_page`, the first match skipped when it is 1, and nothing from result 1000 on
/// (`None`, answered with 416).
pub fn search_offset(query: &[(String, String)]) -> Option<(usize, usize)> {
    let start = number(query, "start_index", 0);
    let size = number(query, "items_per_page", 20).max(1);
    if start >= 1000 {
        return None;
    }
    let served = if size == 1 {
        start.max(1)
    } else {
        start - start % size
    };
    Some((served, size))
}

fn search_page(all: Vec<Value>, query: &[(String, String)], kind: &str) -> (u16, Value) {
    let Some((start, size)) = search_offset(query) else {
        return (416, Value::Null);
    };
    let total = all.len();
    (
        200,
        json!({
            "kind": kind,
            "items": all.into_iter().skip(start).take(size).collect::<Vec<_>>(),
            "items_per_page": size,
            "start_index": start,
            "total_results": total,
        }),
    )
}

fn matches(title: &str, query: &[(String, String)]) -> bool {
    let q = param(query, "q").unwrap_or_default().to_lowercase();
    title.to_lowercase().contains(&q)
}

pub fn officer_search(query: &[(String, String)]) -> (u16, Value) {
    let all: Vec<Value> = [
        json!({
            "title": "Jane Anne SMITH", "kind": "searchresults#officer", "appointment_count": 3,
            "date_of_birth": {"month": 4, "year": 1980}, "address_snippet": "1 Park Row, Leeds, LS1 5AB",
            "description": "Total number of appointments 3 - Born April 1980",
            "links": {"self": "/officers/off-jane/appointments"}, "snippet": ""
        }),
        json!({
            "title": "Tom BROWN", "kind": "searchresults#officer", "appointment_count": 1,
            "address_snippet": "Leeds", "description": "Total number of appointments 1",
            "links": {"self": "/officers/off-tom/appointments"}
        }),
    ]
    .into_iter()
    .filter(|i| matches(i["title"].as_str().unwrap(), query))
    .collect();
    search_page(all, query, "search#officers")
}

pub fn disqualified_search(query: &[(String, String)]) -> (u16, Value) {
    let all: Vec<Value> = [
        json!({
            "title": "John BLOGGS", "kind": "searchresults#disqualified-officer",
            "date_of_birth": "1970-03-15", "address_snippet": "3 Lane, Hull, HU1 1AA",
            "description": "Born on 15 March 1970", "links": {"self": "/disqualified-officers/natural/dq-john"}
        }),
        json!({
            "title": "ROGUE TRADING LIMITED", "kind": "searchresults#disqualified-officer",
            "address_snippet": "London", "description": "",
            "links": {"self": "/disqualified-officers/corporate/dq-rogue"}
        }),
    ]
    .into_iter()
    .filter(|i| matches(i["title"].as_str().unwrap(), query))
    .collect();
    search_page(all, query, "search#disqualified-officers")
}

/// `/dissolved-search/companies` over the fake's dissolved companies; 404 when nothing matches.
pub fn dissolved(companies: &BTreeMap<String, Value>, query: &[(String, String)]) -> (u16, Value) {
    let Some(search_type) = param(query, "search_type") else {
        return (
            400,
            json!({"errors": [{"error": "search_type is required"}]}),
        );
    };
    let q = param(query, "q").unwrap_or_default().to_lowercase();
    let mut items: Vec<Value> = Vec::new();
    for c in companies
        .values()
        .filter(|c| c["company_status"] == "dissolved")
    {
        let previous = c
            .get("previous_company_names")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let matched_previous = previous
            .iter()
            .find(|p| p["name"].as_str().unwrap().to_lowercase().contains(&q))
            .cloned();
        let hit = match search_type {
            "best-match" => c["company_name"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains(&q),
            "previous-name-dissolved" => matched_previous.is_some(),
            _ => return (400, json!({"errors": [{"error": "invalid search_type"}]})),
        };
        if !hit {
            continue;
        }
        let mut item = json!({
            "company_name": c["company_name"], "company_number": c["company_number"],
            "company_status": "dissolved", "kind": "search-results#dissolved-company",
            "date_of_creation": c["date_of_creation"], "date_of_cessation": c["date_of_cessation"],
            "ordered_alpha_key_with_id": format!("KEY:{}", c["company_number"].as_str().unwrap()),
            "registered_office_address": c["registered_office_address"],
            "previous_company_names": previous,
        });
        if search_type == "previous-name-dissolved" {
            item["matched_previous_company_name"] = matched_previous.unwrap();
        }
        items.push(item);
    }
    if items.is_empty() {
        return (404, json!({}));
    }
    let size = number(query, "size", 20);
    let start = number(query, "start_index", 0);
    (
        200,
        json!({
            "etag": "d", "hits": items.len(), "kind": "search#dissolved",
            "top_hit": items[0].clone(),
            "items": items.into_iter().skip(start).take(size).collect::<Vec<_>>(),
        }),
    )
}

/// `/alphabetical-search/companies`: companies in name order from the first at or after `q`.
pub fn alphabetical(
    companies: &BTreeMap<String, Value>,
    query: &[(String, String)],
) -> (u16, Value) {
    let q = param(query, "q").unwrap_or_default().to_uppercase();
    let size = number(query, "size", 20);
    let mut all: Vec<&Value> = companies.values().collect();
    all.sort_by_key(|c| c["company_name"].as_str().unwrap().to_string());
    let items: Vec<Value> = all
        .into_iter()
        .filter(|c| c["company_name"].as_str().unwrap() >= q.as_str())
        .take(size)
        .map(|c| {
            json!({
                "company_name": c["company_name"], "company_number": c["company_number"],
                "company_status": c["company_status"], "company_type": c["type"],
                "kind": "search-results#alphabetical-search",
                "ordered_alpha_key_with_id": format!("KEY:{}", c["company_number"].as_str().unwrap()),
                "links": {"company_profile": format!("/company/{}", c["company_number"].as_str().unwrap())}
            })
        })
        .collect();
    if items.is_empty() {
        return (404, json!({}));
    }
    (
        200,
        json!({"kind": "search#alphabetical-search", "top_hit": items[0].clone(), "items": items}),
    )
}
