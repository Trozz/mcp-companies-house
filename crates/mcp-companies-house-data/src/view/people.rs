//! Shapes for officers, appointments, persons with significant control and disqualified officers.

use super::{at, birth, field, object, one_line_address, segment_after, text};
use serde_json::Value;

fn address(v: &Value, key: &str) -> Option<Value> {
    v.get(key).and_then(one_line_address).map(Value::from)
}

fn link<'a>(v: &'a Value, path: &str) -> Option<&'a str> {
    at(v, path).and_then(Value::as_str)
}

/// One officer of `/company/{number}/officers`. `officer_id` is what
/// `get_officer_appointments` takes.
pub fn officer_item(item: &Value) -> Value {
    let officer_id =
        link(item, "links/officer/appointments").and_then(|l| segment_after(l, "officers"));
    let former: Vec<Value> = item
        .get("former_names")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(|n| {
                    let parts: Vec<&str> = [text(n, "forenames"), text(n, "surname")]
                        .into_iter()
                        .flatten()
                        .collect();
                    (!parts.is_empty()).then(|| Value::from(parts.join(" ")))
                })
                .collect()
        })
        .unwrap_or_default();
    object(vec![
        ("name", field(item, "name")),
        ("officer_role", field(item, "officer_role")),
        ("appointed_on", field(item, "appointed_on")),
        ("appointed_before", field(item, "appointed_before")),
        ("resigned_on", field(item, "resigned_on")),
        ("occupation", field(item, "occupation")),
        ("nationality", field(item, "nationality")),
        ("country_of_residence", field(item, "country_of_residence")),
        ("date_of_birth", birth(item.get("date_of_birth"))),
        ("address", address(item, "address")),
        ("former_names", Some(Value::Array(former))),
        ("identification", field(item, "identification")),
        ("officer_id", officer_id.map(Value::from)),
    ])
}

/// One appointment of `/officers/{id}/appointments`.
pub fn appointment_item(item: &Value) -> Value {
    object(vec![
        (
            "company_number",
            at(item, "appointed_to/company_number").cloned(),
        ),
        (
            "company_name",
            at(item, "appointed_to/company_name").cloned(),
        ),
        (
            "company_status",
            at(item, "appointed_to/company_status").cloned(),
        ),
        ("officer_role", field(item, "officer_role")),
        ("appointed_on", field(item, "appointed_on")),
        ("appointed_before", field(item, "appointed_before")),
        ("resigned_on", field(item, "resigned_on")),
        ("occupation", field(item, "occupation")),
        ("name", field(item, "name")),
        ("address", address(item, "address")),
    ])
}

/// One result of `/search/officers`.
pub fn officer_search_item(item: &Value) -> Value {
    let officer_id = link(item, "links/self").and_then(|l| segment_after(l, "officers"));
    let address = text(item, "address_snippet")
        .map(Value::from)
        .or_else(|| address(item, "address"));
    object(vec![
        ("name", field(item, "title")),
        ("officer_id", officer_id.map(Value::from)),
        ("appointment_count", field(item, "appointment_count")),
        ("date_of_birth", birth(item.get("date_of_birth"))),
        ("address", address),
        ("description", field(item, "description")),
    ])
}

/// One person with significant control (or beneficial owner) of a company.
pub fn psc_item(item: &Value) -> Value {
    let kind = text(item, "kind").map(|k| {
        Value::from(
            k.strip_suffix("-person-with-significant-control")
                .unwrap_or(k),
        )
    });
    object(vec![
        ("name", field(item, "name")),
        ("kind", kind),
        ("notified_on", field(item, "notified_on")),
        ("ceased_on", field(item, "ceased_on")),
        ("natures_of_control", field(item, "natures_of_control")),
        ("nationality", field(item, "nationality")),
        ("country_of_residence", field(item, "country_of_residence")),
        ("date_of_birth", birth(item.get("date_of_birth"))),
        ("address", address(item, "address")),
        (
            "principal_office_address",
            address(item, "principal_office_address"),
        ),
        ("identification", field(item, "identification")),
        ("is_sanctioned", field(item, "is_sanctioned")),
        ("description", field(item, "description")),
    ])
}

/// One persons with significant control statement, e.g. that the company knows of no PSC.
pub fn psc_statement_item(item: &Value) -> Value {
    object(vec![
        ("statement", field(item, "statement")),
        ("notified_on", field(item, "notified_on")),
        ("ceased_on", field(item, "ceased_on")),
        ("linked_psc_name", field(item, "linked_psc_name")),
        (
            "restrictions_notice_withdrawal_reason",
            field(item, "restrictions_notice_withdrawal_reason"),
        ),
    ])
}

/// One result of `/search/disqualified-officers`. `officer_id` and `officer_kind` are what
/// `get_disqualified_officer` takes.
pub fn disqualified_search_item(item: &Value) -> Value {
    let link = link(item, "links/self").unwrap_or_default();
    let (kind, id) = if let Some(id) = segment_after(link, "natural") {
        (Some("natural"), Some(id))
    } else if let Some(id) = segment_after(link, "corporate") {
        (Some("corporate"), Some(id))
    } else {
        (None, None)
    };
    let address = text(item, "address_snippet")
        .map(Value::from)
        .or_else(|| address(item, "address"));
    object(vec![
        ("name", field(item, "title")),
        ("officer_id", id.map(Value::from)),
        ("officer_kind", kind.map(Value::from)),
        ("date_of_birth", birth(item.get("date_of_birth"))),
        ("address", address),
        ("description", field(item, "description")),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn officers_carry_their_officer_id() {
        let item = json!({
            "name": "SMITH, Jane Anne", "officer_role": "director", "appointed_on": "2015-03-01",
            "occupation": "Engineer", "nationality": "British", "country_of_residence": "England",
            "date_of_birth": {"month": 4, "year": 1980},
            "address": {"premises": "1", "address_line_1": "Park Row", "locality": "Leeds", "postal_code": "LS1 5AB"},
            "former_names": [{"forenames": "Jane", "surname": "Jones"}],
            "links": {"self": "/company/00000001/appointments/apt1", "officer": {"appointments": "/officers/off-Jane_1/appointments"}},
            "etag": "e"
        });
        assert_eq!(
            officer_item(&item),
            json!({
                "name": "SMITH, Jane Anne", "officer_role": "director", "appointed_on": "2015-03-01",
                "occupation": "Engineer", "nationality": "British", "country_of_residence": "England",
                "date_of_birth": "1980-04", "address": "1, Park Row, Leeds, LS1 5AB",
                "former_names": ["Jane Jones"], "officer_id": "off-Jane_1"
            })
        );
    }

    #[test]
    fn appointments_name_the_company() {
        let item = json!({
            "appointed_to": {"company_number": "00000002", "company_name": "ACME BAKERY LIMITED", "company_status": "active"},
            "officer_role": "director", "appointed_on": "2021-06-15", "name": "Jane Anne SMITH",
            "links": {"company": "/company/00000002"}
        });
        assert_eq!(
            appointment_item(&item),
            json!({
                "company_number": "00000002", "company_name": "ACME BAKERY LIMITED",
                "company_status": "active", "officer_role": "director",
                "appointed_on": "2021-06-15", "name": "Jane Anne SMITH"
            })
        );
    }

    #[test]
    fn officer_search_items_carry_the_officer_id() {
        let item = json!({
            "title": "Jane Anne SMITH", "appointment_count": 2, "kind": "searchresults#officer",
            "date_of_birth": {"month": 4, "year": 1980}, "address_snippet": "Leeds",
            "description": "Total number of appointments 2 - Born April 1980",
            "links": {"self": "/officers/off-Jane_1/appointments"}
        });
        let v = officer_search_item(&item);
        assert_eq!(v["officer_id"], "off-Jane_1");
        assert_eq!(v["appointment_count"], 2);
        assert_eq!(v["date_of_birth"], "1980-04");
    }

    #[test]
    fn psc_kinds_are_shortened() {
        let item = json!({
            "name": "Mrs Jane Anne Smith", "kind": "individual-person-with-significant-control",
            "notified_on": "2016-04-06", "natures_of_control": ["ownership-of-shares-75-to-100-percent"],
            "date_of_birth": {"month": 4, "year": 1980}, "etag": "e", "links": {"self": "/x"}
        });
        assert_eq!(
            psc_item(&item),
            json!({
                "name": "Mrs Jane Anne Smith", "kind": "individual", "notified_on": "2016-04-06",
                "natures_of_control": ["ownership-of-shares-75-to-100-percent"], "date_of_birth": "1980-04"
            })
        );
        let bo = json!({"name": "X", "kind": "individual-beneficial-owner"});
        assert_eq!(psc_item(&bo)["kind"], "individual-beneficial-owner");
    }

    #[test]
    fn disqualified_items_carry_kind_and_id() {
        let item = json!({"title": "John BLOGGS", "date_of_birth": "1970-03-15",
            "links": {"self": "/disqualified-officers/natural/dq-1"}, "address_snippet": "Hull"});
        let v = disqualified_search_item(&item);
        assert_eq!(v["officer_id"], "dq-1");
        assert_eq!(v["officer_kind"], "natural");
        assert_eq!(v["date_of_birth"], "1970-03-15");
        let item =
            json!({"title": "BAD CO", "links": {"self": "/disqualified-officers/corporate/dq-2"}});
        assert_eq!(disqualified_search_item(&item)["officer_kind"], "corporate");
    }
}
