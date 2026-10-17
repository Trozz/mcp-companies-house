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
}
