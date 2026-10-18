//! Shapes Companies House responses into the compact JSON the tools return. Only known members
//! are picked from search results; unknown members of a company profile pass through, as the API
//! guidelines ask clients to expect members they have not seen before.

use serde_json::{Map, Value, json};

pub mod people;
pub mod records;

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

pub(crate) fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
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

pub(crate) fn object(pairs: Vec<(&str, Option<Value>)>) -> Value {
    let map: Map<String, Value> = pairs
        .into_iter()
        .filter_map(|(k, v)| v.and_then(prune).map(|v| (k.to_string(), v)))
        .collect();
    Value::Object(map)
}

pub(crate) fn field(v: &Value, key: &str) -> Option<Value> {
    v.get(key).cloned()
}

/// A value at a `/`-separated path of object keys, e.g. `links/officer/appointments`.
pub(crate) fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('/').try_fold(v, |v, k| v.get(k))
}

/// The path segment after `key` in a Companies House link, for example the officer id in
/// `/officers/{id}/appointments`.
pub fn segment_after(link: &str, key: &str) -> Option<String> {
    let mut parts = link.split(['/', '?']).filter(|p| !p.is_empty());
    parts.find(|p| *p == key)?;
    parts.next().map(str::to_string)
}

/// The last path segment of a Companies House link.
pub fn last_segment(link: &str) -> Option<String> {
    link.split('?')
        .next()?
        .rsplit('/')
        .find(|p| !p.is_empty())
        .map(str::to_string)
}

/// A partial date of birth `{"month": 3, "year": 1970}` as `1970-03`; a full date string as is.
pub fn birth(v: Option<&Value>) -> Option<Value> {
    let v = v?;
    if let Some(s) = v.as_str() {
        return Some(Value::from(s));
    }
    let year = v.get("year").and_then(Value::as_u64)?;
    match v.get("month").and_then(Value::as_u64) {
        Some(m) => Some(Value::from(format!("{year}-{m:02}"))),
        None => Some(Value::from(year.to_string())),
    }
}

/// A resource without `etag`, `links` and empty members.
pub fn detail(v: Value) -> Value {
    let Value::Object(mut map) = v else {
        return Value::Object(Map::new());
    };
    map.remove("etag");
    map.remove("links");
    prune(Value::Object(map)).unwrap_or_else(|| Value::Object(Map::new()))
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

/// One result of `/dissolved-search/companies`.
pub fn dissolved_item(item: &Value) -> Value {
    let previous: Vec<Value> = item
        .get("previous_company_names")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .map(|n| {
                    object(vec![
                        ("name", field(n, "name")),
                        ("effective_from", field(n, "effective_from")),
                        ("ceased_on", field(n, "ceased_on")),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    object(vec![
        ("company_number", field(item, "company_number")),
        ("company_name", field(item, "company_name")),
        ("company_status", field(item, "company_status")),
        ("date_of_creation", field(item, "date_of_creation")),
        ("date_of_cessation", field(item, "date_of_cessation")),
        (
            "address",
            item.get("registered_office_address")
                .and_then(one_line_address)
                .map(Value::from),
        ),
        ("previous_company_names", Some(Value::Array(previous))),
        (
            "matched_previous_name",
            item.get("matched_previous_company_name")
                .and_then(|m| m.get("name"))
                .cloned(),
        ),
    ])
}

/// One result of `/alphabetical-search/companies`.
pub fn alphabetical_item(item: &Value) -> Value {
    object(vec![
        ("company_number", field(item, "company_number")),
        ("company_name", field(item, "company_name")),
        ("company_status", field(item, "company_status")),
        ("company_type", field(item, "company_type")),
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
    fn link_segments_are_extracted() {
        assert_eq!(
            segment_after("/officers/AbC-1_x/appointments", "officers").as_deref(),
            Some("AbC-1_x")
        );
        assert_eq!(
            segment_after("/disqualified-officers/natural/xyz", "natural").as_deref(),
            Some("xyz")
        );
        assert_eq!(segment_after("/company/1", "officers"), None);
        assert_eq!(
            last_segment("/company/1/charges/ch-1?x=1").as_deref(),
            Some("ch-1")
        );
        assert_eq!(
            last_segment("https://document-api.example/document/doc-9/").as_deref(),
            Some("doc-9")
        );
        assert_eq!(at(&json!({"a": {"b": {"c": 1}}}), "a/b/c"), Some(&json!(1)));
    }

    #[test]
    fn births_are_shown_as_year_and_month() {
        assert_eq!(
            birth(Some(&json!({"month": 3, "year": 1970}))),
            Some(json!("1970-03"))
        );
        assert_eq!(birth(Some(&json!({"year": 1970}))), Some(json!("1970")));
        assert_eq!(birth(Some(&json!("1970-03-15"))), Some(json!("1970-03-15")));
        assert_eq!(birth(Some(&json!({}))), None);
        assert_eq!(birth(None), None);
    }

    #[test]
    fn details_drop_etag_links_and_empties() {
        assert_eq!(
            detail(json!({"etag": "e", "links": {"self": "/x"}, "a": "", "b": 1})),
            json!({"b": 1})
        );
        assert_eq!(detail(json!([1])), json!({}));
    }

    #[test]
    fn dissolved_items_show_previous_names() {
        let item = json!({
            "company_name": "OLD CO LIMITED", "company_number": "01234567",
            "company_status": "dissolved", "date_of_creation": "1990-01-01",
            "date_of_cessation": "2010-01-01", "kind": "search-results#dissolved-company",
            "ordered_alpha_key_with_id": "OLDCO:01234567",
            "registered_office_address": {"address_line_1": "1 Street", "locality": "Hull", "postal_code": "HU1 1AA"},
            "previous_company_names": [{"name": "OLDER CO LIMITED", "company_number": "01234567", "effective_from": "1990-01-01", "ceased_on": "2000-01-01"}],
            "matched_previous_company_name": {"name": "OLDER CO LIMITED"}
        });
        assert_eq!(
            dissolved_item(&item),
            json!({
                "company_number": "01234567", "company_name": "OLD CO LIMITED",
                "company_status": "dissolved", "date_of_creation": "1990-01-01",
                "date_of_cessation": "2010-01-01", "address": "1 Street, Hull, HU1 1AA",
                "previous_company_names": [{"name": "OLDER CO LIMITED", "effective_from": "1990-01-01", "ceased_on": "2000-01-01"}],
                "matched_previous_name": "OLDER CO LIMITED"
            })
        );
    }

    #[test]
    fn alphabetical_items_are_compact() {
        let item = json!({"company_name": "A LTD", "company_number": "1", "company_status": "active",
            "company_type": "ltd", "ordered_alpha_key_with_id": "A:1", "kind": "search-results#alphabetical-search",
            "links": {"company_profile": "/company/1"}});
        assert_eq!(
            alphabetical_item(&item),
            json!({"company_number": "1", "company_name": "A LTD", "company_status": "active", "company_type": "ltd"})
        );
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
