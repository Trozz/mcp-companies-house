//! Shapes Companies House responses into the compact JSON the tools return. Only known members
//! are picked from search results; unknown members of a company profile pass through, as the API
//! guidelines ask clients to expect members they have not seen before.

use serde_json::{Map, Value, json};

/// Profile members that are deprecated in favour of `links` or `subtype`, or are noise to a reader.
const PROFILE_DROPPED: &[&str] = &[
    "etag",
    "links",
    "has_been_liquidated",
    "has_charges",
    "has_insolvency_history",
    "is_community_interest_company",
];

/// Removes nulls, empty strings and empty arrays or objects, recursively. `None` when nothing is
/// left.
pub fn prune(v: Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::String(s) if s.trim().is_empty() => None,
        Value::Array(items) => {
            let items: Vec<Value> = items.into_iter().filter_map(prune).collect();
            (!items.is_empty()).then_some(Value::Array(items))
        }
        Value::Object(map) => {
            let map: Map<String, Value> = map
                .into_iter()
                .filter_map(|(k, v)| prune(v).map(|v| (k, v)))
                .collect();
            (!map.is_empty()).then_some(Value::Object(map))
        }
        other => Some(other),
    }
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// An address object on one line: `c/o`, PO box, premises, lines, locality, region, postcode,
/// country.
pub fn one_line_address(addr: &Value) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(c) = text(addr, "care_of") {
        parts.push(format!("c/o {c}"));
    }
    if let Some(p) = text(addr, "po_box") {
        parts.push(format!("PO Box {p}"));
    }
    for key in [
        "premises",
        "address_line_1",
        "address_line_2",
        "locality",
        "region",
        "postal_code",
        "country",
    ] {
        if let Some(t) = text(addr, key) {
            parts.push(t.to_string());
        }
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn object(pairs: Vec<(&str, Option<Value>)>) -> Value {
    let map: Map<String, Value> = pairs
        .into_iter()
        .filter_map(|(k, v)| v.and_then(prune).map(|v| (k.to_string(), v)))
        .collect();
    Value::Object(map)
}

fn field(v: &Value, key: &str) -> Option<Value> {
    v.get(key).cloned()
}

/// One result of `/search/companies`.
pub fn search_item(item: &Value) -> Value {
    let address = text(item, "address_snippet")
        .map(str::to_string)
        .or_else(|| item.get("address").and_then(one_line_address));
    object(vec![
        ("company_number", field(item, "company_number")),
        ("company_name", field(item, "title")),
        ("company_status", field(item, "company_status")),
        ("company_type", field(item, "company_type")),
        ("date_of_creation", field(item, "date_of_creation")),
        ("date_of_cessation", field(item, "date_of_cessation")),
        ("address", address.map(Value::from)),
        ("snippet", field(item, "snippet")),
    ])
}

/// One result of `/advanced-search/companies`.
pub fn advanced_item(item: &Value) -> Value {
    let address = item
        .get("registered_office_address")
        .and_then(one_line_address);
    object(vec![
        ("company_number", field(item, "company_number")),
        ("company_name", field(item, "company_name")),
        ("company_status", field(item, "company_status")),
        ("company_type", field(item, "company_type")),
        ("company_subtype", field(item, "company_subtype")),
        ("date_of_creation", field(item, "date_of_creation")),
        ("date_of_cessation", field(item, "date_of_cessation")),
        ("address", address.map(Value::from)),
        ("sic_codes", field(item, "sic_codes")),
    ])
}

/// A company profile without deprecated and internal members. `links` becomes
/// `records_available`: the related records Companies House holds for the company (for example
/// `charges` or `insolvency`).
pub fn company_profile(profile: Value) -> Value {
    let Value::Object(mut map) = profile else {
        return Value::Object(Map::new());
    };
    let mut records: Vec<String> = map
        .get("links")
        .and_then(Value::as_object)
        .map(|links| links.keys().filter(|k| *k != "self").cloned().collect())
        .unwrap_or_default();
    records.sort();
    for key in PROFILE_DROPPED {
        map.remove(*key);
    }
    if !records.is_empty() {
        map.insert("records_available".into(), json!(records));
    }
    prune(Value::Object(map)).unwrap_or_else(|| Value::Object(Map::new()))
}

/// A page of results with the paging fields every list tool returns. `next_start_index` is set
/// only when more results exist.
pub fn page(total: Option<u64>, start_index: u32, items: Vec<Value>) -> Value {
    let count = items.len() as u64;
    let mut out = Map::new();
    if let Some(t) = total {
        out.insert("total".into(), json!(t));
    }
    out.insert("start_index".into(), json!(start_index));
    out.insert("count".into(), json!(count));
    let next = u64::from(start_index) + count;
    if count > 0 && total.is_some_and(|t| next < t) {
        out.insert("next_start_index".into(), json!(next));
    }
    out.insert("items".into(), Value::Array(items));
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_drops_empty_values_recursively() {
        let v = json!({
            "a": "", "b": null, "c": [], "d": {}, "e": {"f": " ", "g": "x"},
            "h": [null, "", "y"], "i": false, "j": 0
        });
        assert_eq!(
            prune(v),
            Some(json!({"e": {"g": "x"}, "h": ["y"], "i": false, "j": 0}))
        );
        assert_eq!(prune(json!({"a": ""})), None);
    }

    #[test]
    fn addresses_are_one_line_in_reading_order() {
        let a = json!({
            "address_line_1": "Old Admiralty Building", "address_line_2": "Admiralty Place",
            "country": "United Kingdom", "locality": "London", "po_box": "",
            "postal_code": "SW1A 2DY", "premises": "House Name", "region": ""
        });
        assert_eq!(
            one_line_address(&a).as_deref(),
            Some(
                "House Name, Old Admiralty Building, Admiralty Place, London, SW1A 2DY, United Kingdom"
            )
        );
        let a = json!({"care_of": "Agent Ltd", "po_box": "12", "locality": "Cardiff"});
        assert_eq!(
            one_line_address(&a).as_deref(),
            Some("c/o Agent Ltd, PO Box 12, Cardiff")
        );
        assert_eq!(one_line_address(&json!({})), None);
    }

    #[test]
    fn search_items_use_the_snippet_address() {
        let item = json!({
            "kind": "searchresults#company", "title": "EXAMPLE TRADING LIMITED",
            "company_number": "01234567", "company_status": "active", "company_type": "ltd",
            "date_of_creation": "2001-02-03", "address_snippet": "1 High Street, Leeds, LS1 1AA",
            "address": {"address_line_1": "ignored"}, "description": "01234567 - Incorporated on 3 February 2001",
            "links": {"self": "/company/01234567"}, "snippet": "", "matches": {"title": [1, 7]}
        });
        assert_eq!(
            search_item(&item),
            json!({
                "company_number": "01234567", "company_name": "EXAMPLE TRADING LIMITED",
                "company_status": "active", "company_type": "ltd",
                "date_of_creation": "2001-02-03", "address": "1 High Street, Leeds, LS1 1AA"
            })
        );
    }

    #[test]
    fn search_items_fall_back_to_the_address_object() {
        let item = json!({"title": "X", "address": {"locality": "York", "postal_code": "YO1 1AA"}});
        assert_eq!(search_item(&item)["address"], "York, YO1 1AA");
    }

    #[test]
    fn advanced_items_keep_sic_codes_and_subtype() {
        let item = json!({
            "company_name": "EXAMPLE CIC", "company_number": "SC123456",
            "company_status": "dissolved", "company_type": "ltd",
            "company_subtype": "community-interest-company", "date_of_creation": "2010-01-01",
            "date_of_cessation": "2020-01-01", "kind": "search-results#company",
            "links": {"company_profile": "/company/SC123456"},
            "registered_office_address": {"address_line_1": "1 Street", "locality": "Glasgow", "postal_code": "G1 1AA"},
            "sic_codes": ["88990"]
        });
        assert_eq!(
            advanced_item(&item),
            json!({
                "company_number": "SC123456", "company_name": "EXAMPLE CIC",
                "company_status": "dissolved", "company_type": "ltd",
                "company_subtype": "community-interest-company",
                "date_of_creation": "2010-01-01", "date_of_cessation": "2020-01-01",
                "address": "1 Street, Glasgow, G1 1AA", "sic_codes": ["88990"]
            })
        );
    }

    #[test]
    fn profiles_drop_deprecated_members_and_list_records() {
        let p = json!({
            "company_name": "COMPANY 98827885 LIMITED", "company_number": "98827885",
            "company_status": "active", "etag": "d51004e3", "has_charges": false,
            "has_insolvency_history": false, "jurisdiction": "england-wales",
            "links": {"self": "/company/98827885", "officers": "/company/98827885/officers",
                      "filing_history": "/company/98827885/filing-history"},
            "registered_office_address": {"locality": "London", "po_box": "", "region": ""},
            "sic_codes": ["71200"], "type": "ltd", "can_file": true,
            "a_member_added_later": {"x": 1}
        });
        assert_eq!(
            company_profile(p),
            json!({
                "company_name": "COMPANY 98827885 LIMITED", "company_number": "98827885",
                "company_status": "active", "jurisdiction": "england-wales",
                "registered_office_address": {"locality": "London"},
                "sic_codes": ["71200"], "type": "ltd", "can_file": true,
                "a_member_added_later": {"x": 1},
                "records_available": ["filing_history", "officers"]
            })
        );
        assert_eq!(company_profile(json!("odd")), json!({}));
    }

    #[test]
    fn pages_report_the_next_start_index_only_when_more_exist() {
        let items = vec![json!(1), json!(2)];
        assert_eq!(
            page(Some(10), 4, items.clone()),
            json!({"total": 10, "start_index": 4, "count": 2, "next_start_index": 6, "items": [1, 2]})
        );
        assert_eq!(
            page(Some(6), 4, items.clone()),
            json!({"total": 6, "start_index": 4, "count": 2, "items": [1, 2]})
        );
        assert_eq!(
            page(None, 0, items),
            json!({"start_index": 0, "count": 2, "items": [1, 2]})
        );
        assert_eq!(
            page(Some(10), 20, vec![]),
            json!({"total": 10, "start_index": 20, "count": 0, "items": []})
        );
    }
}
