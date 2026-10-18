//! Shapes for filing history, charges and insolvency.

use super::{at, field, last_segment, object, one_line_address, prune, text};
use serde_json::{Map, Value};

/// An object, or the first object of an array: the specification and the live API disagree on
/// whether some charge and insolvency members are objects or arrays.
fn first_object(v: Option<&Value>) -> Option<&Value> {
    match v? {
        Value::Array(items) => items.first(),
        other @ Value::Object(_) => Some(other),
        _ => None,
    }
}

/// One filing of `/company/{number}/filing-history`. `description` is the Companies House
/// description key (for example `accounts-with-accounts-type-micro-entity`) and
/// `description_values` its placeholders; `document_id` identifies the filed document.
pub fn filing_item(item: &Value) -> Value {
    let document_id = at(item, "links/document_metadata")
        .and_then(Value::as_str)
        .and_then(last_segment);
    object(vec![
        ("date", field(item, "date")),
        ("type", field(item, "type")),
        ("category", field(item, "category")),
        ("subcategory", field(item, "subcategory")),
        ("description", field(item, "description")),
        ("description_values", field(item, "description_values")),
        ("action_date", field(item, "action_date")),
        ("pages", field(item, "pages")),
        ("paper_filed", field(item, "paper_filed")),
        ("transaction_id", field(item, "transaction_id")),
        ("document_id", document_id.map(Value::from)),
        ("resolutions", field(item, "resolutions")),
        ("annotations", field(item, "annotations")),
        ("associated_filings", field(item, "associated_filings")),
    ])
}

fn persons_entitled(item: &Value) -> Option<Value> {
    let names: Vec<Value> = item
        .get("persons_entitled")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|p| text(p, "name").map(Value::from))
        .collect();
    Some(Value::Array(names))
}

/// One charge of `/company/{number}/charges`, compact. `charge_id` is what `get_charge` takes.
pub fn charge_item(item: &Value) -> Value {
    let classification = first_object(item.get("classification"))
        .and_then(|c| c.get("description"))
        .cloned();
    let particulars = first_object(item.get("particulars")).map(|p| {
        let mut p = p.clone();
        if let Value::Object(m) = &mut p {
            m.remove("type");
        }
        p
    });
    object(vec![
        ("charge_id", field(item, "id")),
        ("charge_code", field(item, "charge_code")),
        ("charge_number", field(item, "charge_number")),
        ("status", field(item, "status")),
        ("classification", classification),
        ("created_on", field(item, "created_on")),
        ("delivered_on", field(item, "delivered_on")),
        ("satisfied_on", field(item, "satisfied_on")),
        ("persons_entitled", persons_entitled(item)),
        (
            "more_than_four_persons_entitled",
            field(item, "more_than_four_persons_entitled"),
        ),
        ("particulars", particulars),
    ])
}

/// A charge in full: every member except `etag` and `links`, with `charge_id` first.
pub fn charge_detail(item: Value) -> Value {
    let Value::Object(mut map) = item else {
        return Value::Object(Map::new());
    };
    map.remove("etag");
    map.remove("links");
    let mut out = Map::new();
    if let Some(id) = map.remove("id") {
        out.insert("charge_id".into(), id);
    }
    out.extend(map);
    prune(Value::Object(out)).unwrap_or_else(|| Value::Object(Map::new()))
}

fn practitioner(p: &Value) -> Value {
    object(vec![
        ("name", field(p, "name")),
        ("role", field(p, "role")),
        ("appointed_on", field(p, "appointed_on")),
        ("ceased_to_act_on", field(p, "ceased_to_act_on")),
        (
            "address",
            first_object(p.get("address"))
                .and_then(one_line_address)
                .map(Value::from),
        ),
    ])
}

/// `/company/{number}/insolvency`: status and cases, each case's dates as a map from date type
/// (for example `wound-up-on`) to date.
pub fn insolvency(v: &Value) -> Value {
    let cases: Vec<Value> = v
        .get("cases")
        .and_then(Value::as_array)
        .map(|cases| {
            cases
                .iter()
                .map(|c| {
                    let dates: Map<String, Value> = c
                        .get("dates")
                        .and_then(Value::as_array)
                        .map(|ds| {
                            ds.iter()
                                .filter_map(|d| {
                                    Some((text(d, "type")?.to_string(), d.get("date")?.clone()))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let practitioners: Vec<Value> = c
                        .get("practitioners")
                        .and_then(Value::as_array)
                        .map(|ps| ps.iter().map(practitioner).collect())
                        .unwrap_or_default();
                    object(vec![
                        ("type", field(c, "type")),
                        ("number", field(c, "number")),
                        ("dates", Some(Value::Object(dates))),
                        ("practitioners", Some(Value::Array(practitioners))),
                        ("notes", field(c, "notes")),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    let mut out = Map::new();
    if let Some(s) = v.get("status").cloned().and_then(prune) {
        out.insert("status".into(), s);
    }
    out.insert("cases".into(), Value::Array(cases));
    Value::Object(out)
}

/// `/company/{number}/registers`: where each statutory register is kept, latest move first.
/// `location` is where the register is now: `public-register` (held at Companies House),
/// `registered-office`, `single-alternative-inspection-location` or `unspecified-location`.
pub fn registers(v: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    if let Some(all) = v.get("registers").and_then(Value::as_object) {
        for (name, register) in all {
            let mut moves: Vec<Value> = register
                .get("items")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|i| {
                            let transaction_id = at(i, "links/filing")
                                .and_then(Value::as_str)
                                .and_then(last_segment);
                            object(vec![
                                ("location", field(i, "register_moved_to")),
                                ("moved_on", field(i, "moved_on")),
                                ("transaction_id", transaction_id.map(Value::from)),
                            ])
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Dates are YYYY-MM-DD, so they sort as text; undated moves go last.
            moves.sort_by(|a, b| {
                let date = |m: &Value| m.get("moved_on").and_then(Value::as_str).map(String::from);
                date(b).cmp(&date(a))
            });
            let latest = moves.first().cloned().unwrap_or(Value::Null);
            out.push(object(vec![
                ("register", Some(Value::from(name.as_str()))),
                ("location", latest.get("location").cloned()),
                ("since", latest.get("moved_on").cloned()),
                ("history", Some(Value::Array(moves))),
            ]));
        }
    }
    let mut map = Map::new();
    map.insert("registers".into(), Value::Array(out));
    Value::Object(map)
}

/// `/company/{number}/exemptions`: each exemption with its periods; `current` when a period has
/// no end date.
pub fn exemptions(v: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    if let Some(all) = v.get("exemptions").and_then(Value::as_object) {
        for (name, exemption) in all {
            let periods: Vec<Value> = exemption
                .get("items")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|i| {
                            object(vec![
                                ("exempt_from", field(i, "exempt_from")),
                                ("exempt_to", field(i, "exempt_to")),
                            ])
                        })
                        .collect()
                })
                .unwrap_or_default();
            let current = periods.iter().any(|p| p.get("exempt_to").is_none());
            let kind = text(exemption, "exemption_type").unwrap_or(name);
            out.push(object(vec![
                ("exemption", Some(Value::from(kind))),
                ("current", Some(Value::Bool(current))),
                ("periods", Some(Value::Array(periods))),
            ]));
        }
    }
    let mut map = Map::new();
    map.insert("exemptions".into(), Value::Array(out));
    Value::Object(map)
}

/// One UK establishment (branch) of an overseas company.
pub fn uk_establishment_item(item: &Value) -> Value {
    object(vec![
        ("company_number", field(item, "company_number")),
        ("company_name", field(item, "company_name")),
        ("company_status", field(item, "company_status")),
        ("locality", field(item, "locality")),
    ])
}

/// The format names `download_filing_document` takes, with their content types.
pub const DOCUMENT_FORMATS: &[(&str, &str)] = &[
    ("pdf", "application/pdf"),
    ("xhtml", "application/xhtml+xml"),
    ("xml", "application/xml"),
    ("json", "application/json"),
];

/// The format name of a content type, or the content type itself when it has none.
pub fn format_name(content_type: &str) -> &str {
    DOCUMENT_FORMATS
        .iter()
        .find(|(_, t)| t.eq_ignore_ascii_case(content_type))
        .map(|(f, _)| *f)
        .unwrap_or(content_type)
}

/// Document API metadata: what the document is and the formats it can be downloaded in.
pub fn document_metadata(id: &str, v: &Value) -> Value {
    let formats: Vec<Value> = v
        .get("resources")
        .and_then(Value::as_object)
        .map(|resources| {
            resources
                .iter()
                .map(|(content_type, r)| {
                    object(vec![
                        ("format", Some(Value::from(format_name(content_type)))),
                        ("content_type", Some(Value::from(content_type.as_str()))),
                        ("size_bytes", field(r, "content_length")),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    object(vec![
        ("document_id", Some(Value::from(id))),
        ("company_number", field(v, "company_number")),
        ("category", field(v, "category")),
        ("pages", field(v, "pages")),
        ("created_at", field(v, "created_at")),
        ("significant_date", field(v, "significant_date")),
        ("significant_date_type", field(v, "significant_date_type")),
        ("formats", Some(Value::Array(formats))),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filings_keep_the_description_key_and_document_id() {
        let item = json!({
            "category": "accounts", "date": "2024-12-20", "type": "AA",
            "description": "accounts-with-accounts-type-micro-entity",
            "description_values": {"made_up_date": "2024-03-31"},
            "transaction_id": "MzQ1Njc4", "pages": 3, "barcode": "XD1",
            "links": {"self": "/company/00000001/filing-history/MzQ1Njc4",
                      "document_metadata": "https://frontend-doc-api.company-information.service.gov.uk/document/doc-abc"}
        });
        assert_eq!(
            filing_item(&item),
            json!({
                "date": "2024-12-20", "type": "AA", "category": "accounts",
                "description": "accounts-with-accounts-type-micro-entity",
                "description_values": {"made_up_date": "2024-03-31"}, "pages": 3,
                "transaction_id": "MzQ1Njc4", "document_id": "doc-abc"
            })
        );
    }

    #[test]
    fn charges_accept_objects_or_arrays() {
        let live = json!({
            "id": "ch-1", "charge_code": "000000060001", "charge_number": 1, "status": "outstanding",
            "classification": {"type": "charge-description", "description": "A registered charge"},
            "created_on": "2019-01-02", "delivered_on": "2019-01-10",
            "persons_entitled": [{"name": "Big Bank PLC"}],
            "particulars": {"type": "brief-description", "description": "Land at 1 Poultry", "contains_fixed_charge": true},
            "etag": "e", "links": {"self": "/company/00000006/charges/ch-1"}
        });
        let spec = json!({
            "id": "ch-1", "status": "outstanding",
            "classification": [{"type": "charge-description", "description": "A registered charge"}],
            "particulars": [{"type": "brief-description", "description": "Land at 1 Poultry", "contains_fixed_charge": true}]
        });
        let v = charge_item(&live);
        assert_eq!(
            v,
            json!({
                "charge_id": "ch-1", "charge_code": "000000060001", "charge_number": 1,
                "status": "outstanding", "classification": "A registered charge",
                "created_on": "2019-01-02", "delivered_on": "2019-01-10",
                "persons_entitled": ["Big Bank PLC"],
                "particulars": {"description": "Land at 1 Poultry", "contains_fixed_charge": true}
            })
        );
        let s = charge_item(&spec);
        assert_eq!(s["classification"], v["classification"]);
        assert_eq!(s["particulars"], v["particulars"]);
    }

    #[test]
    fn charge_details_rename_the_id() {
        let v = charge_detail(
            json!({"id": "ch-1", "etag": "e", "links": {}, "status": "satisfied", "transactions": []}),
        );
        assert_eq!(v, json!({"charge_id": "ch-1", "status": "satisfied"}));
    }

    #[test]
    fn insolvency_dates_become_a_map() {
        let v = json!({
            "etag": "e", "status": "liquidation",
            "cases": [{
                "type": "creditors-voluntary-liquidation", "number": "1",
                "dates": [{"type": "wound-up-on", "date": "2025-02-01"}],
                "practitioners": [{"name": "A Liquidator", "role": "practitioner", "appointed_on": "2025-02-01",
                                   "address": {"address_line_1": "1 Way", "locality": "York", "postal_code": "YO1 1AA"}}],
                "links": {"charge": "/x"}
            }]
        });
        assert_eq!(
            insolvency(&v),
            json!({
                "status": "liquidation",
                "cases": [{
                    "type": "creditors-voluntary-liquidation", "number": "1",
                    "dates": {"wound-up-on": "2025-02-01"},
                    "practitioners": [{"name": "A Liquidator", "role": "practitioner",
                        "appointed_on": "2025-02-01", "address": "1 Way, York, YO1 1AA"}]
                }]
            })
        );
        assert_eq!(insolvency(&json!({})), json!({"cases": []}));
    }

    #[test]
    fn registers_show_where_each_is_kept_now() {
        let v = json!({
            "registers": {
                "directors": {"register_type": "directors", "items": [
                    {"moved_on": "2018-01-01", "register_moved_to": "single-alternative-inspection-location"},
                    {"moved_on": "2021-05-05", "register_moved_to": "public-register",
                     "links": {"filing": "/company/00000001/filing-history/tx9"}}
                ]},
                "members": {"register_type": "members", "items": []}
            }
        });
        assert_eq!(
            registers(&v),
            json!({"registers": [
                {"register": "directors", "location": "public-register", "since": "2021-05-05", "history": [
                    {"location": "public-register", "moved_on": "2021-05-05", "transaction_id": "tx9"},
                    {"location": "single-alternative-inspection-location", "moved_on": "2018-01-01"}
                ]},
                {"register": "members"}
            ]})
        );
        assert_eq!(registers(&json!({})), json!({"registers": []}));
    }

    #[test]
    fn exemptions_say_whether_they_still_apply() {
        let v = json!({"exemptions": {
            "psc_exempt_as_trading_on_regulated_market": {
                "exemption_type": "psc-exempt-as-trading-on-regulated-market",
                "items": [{"exempt_from": "2016-04-06"}]
            },
            "disclosure_transparency_rules_chapter_five_applies": {
                "exemption_type": "disclosure-transparency-rules-chapter-five-applies",
                "items": [{"exempt_from": "2016-04-06", "exempt_to": "2019-01-01"}]
            }
        }});
        assert_eq!(
            exemptions(&v),
            json!({"exemptions": [
                {"exemption": "disclosure-transparency-rules-chapter-five-applies", "current": false,
                 "periods": [{"exempt_from": "2016-04-06", "exempt_to": "2019-01-01"}]},
                {"exemption": "psc-exempt-as-trading-on-regulated-market", "current": true,
                 "periods": [{"exempt_from": "2016-04-06"}]}
            ]})
        );
    }

    #[test]
    fn document_metadata_lists_formats_by_name() {
        let v = json!({
            "company_number": "00000001", "category": "accounts", "pages": 3, "etag": "",
            "created_at": "2024-12-20T10:00:00Z", "significant_date": null,
            "links": {"self": "x", "document": "x/content"},
            "resources": {"application/pdf": {"content_length": 1234}, "application/xhtml+xml": {"content_length": 99}}
        });
        assert_eq!(
            document_metadata("doc-1", &v),
            json!({
                "document_id": "doc-1", "company_number": "00000001", "category": "accounts", "pages": 3,
                "created_at": "2024-12-20T10:00:00Z",
                "formats": [
                    {"format": "pdf", "content_type": "application/pdf", "size_bytes": 1234},
                    {"format": "xhtml", "content_type": "application/xhtml+xml", "size_bytes": 99}
                ]
            })
        );
        assert_eq!(format_name("text/csv"), "text/csv");
    }
}
