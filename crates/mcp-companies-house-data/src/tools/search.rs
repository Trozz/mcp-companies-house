//! Finding companies: best-match, advanced, dissolved and alphabetical search, name
//! availability, company profiles and SIC codes.

use super::{
    CompaniesServer, Query, count, date_range, items, json_result, limit_arg,
    normalise_company_number, required_text, run, text_arg,
};
use crate::codes::{self, CompanyStatus, CompanySubtype, CompanyType};
use crate::error::{self, ChError};
use crate::sic;
use crate::view::{
    advanced_item, alphabetical_item, company_profile, dissolved_item, page, search_item,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

pub const MAX_SIC_CODES: usize = 50;
/// Results the alphabetical and name-availability searches return at most.
pub const MAX_NEARBY: u32 = 100;

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchCompaniesParams {
    /// Company name or company number to search for, e.g. "rolls royce" or "00445790".
    pub query: String,
    /// true: only companies that are currently active.
    pub active_only: Option<bool>,
    /// Results per page, 1 to 100. Default: 20.
    pub limit: Option<u32>,
    /// Index of the first result, from 0. Pass next_start_index from the previous page. Default: 0.
    pub start_index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct FilterCompaniesParams {
    /// Words the company name must contain.
    pub name_includes: Option<String>,
    /// Words the company name must not contain.
    pub name_excludes: Option<String>,
    /// Only companies with one of these statuses.
    pub status: Option<Vec<CompanyStatus>>,
    /// Only companies of one of these types.
    pub company_type: Option<Vec<CompanyType>>,
    /// Only companies with one of these subtypes.
    pub subtype: Option<Vec<CompanySubtype>>,
    /// Nature of business as SIC 2007 codes, 4 or 5 digits, e.g. ["62012", "62020"] for software and IT consultancy (find codes with lookup_sic_codes). A company matches if it has any of them.
    pub sic_codes: Option<Vec<String>>,
    /// Text matched against the registered office address, e.g. a town, county or postcode.
    pub location: Option<String>,
    /// Incorporated on or after this date, YYYY-MM-DD.
    pub incorporated_from: Option<String>,
    /// Incorporated on or before this date, YYYY-MM-DD.
    pub incorporated_to: Option<String>,
    /// Dissolved on or after this date, YYYY-MM-DD.
    pub dissolved_from: Option<String>,
    /// Dissolved on or before this date, YYYY-MM-DD.
    pub dissolved_to: Option<String>,
    /// Results per page, 1 to 100. Default: 20.
    pub limit: Option<u32>,
    /// Index of the first result, from 0. Pass next_start_index from the previous page. Default: 0.
    pub start_index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CompanyNumberParams {
    /// Company number, e.g. 00445790 or SC123456. Spaces are ignored and shorter numbers are padded with zeros.
    pub company_number: String,
}

/// How `search_dissolved_companies` matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub enum DissolvedMatch {
    /// Best match on the company's last name.
    BestMatch,
    /// Best match on the names the company had before.
    PreviousNames,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchDissolvedParams {
    /// Company name to search for among dissolved companies.
    pub query: String,
    /// best_match (default) searches current names; previous_names searches names the companies had before.
    pub match_on: Option<DissolvedMatch>,
    /// Results per page, 1 to 100. Default: 20.
    pub limit: Option<u32>,
    /// Index of the first result, from 0. Pass next_start_index from the previous page. Default: 0.
    pub start_index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct NameAvailabilityParams {
    /// The proposed company name, e.g. "Acme Widgets Ltd".
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct BrowseCompanyNamesParams {
    /// Company name to start from; companies are listed in register (alphabetical) order around it.
    pub name: String,
    /// How many companies to list, 1 to 100. Default: 20.
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct LookupSicCodesParams {
    /// Words describing the business, e.g. "bakery", "software development", "letting property"; or the first digits of a code, e.g. "620".
    pub query: Option<String>,
    /// Codes to describe, e.g. ["62012", "99999"].
    pub codes: Option<Vec<String>>,
    /// Matches to return, 1 to 100. Default: 20.
    pub limit: Option<u32>,
}

fn sic_codes_arg(codes: Option<&[String]>) -> error::Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for raw in codes.unwrap_or_default() {
        let code = raw.trim();
        if !(4..=5).contains(&code.len()) || !code.chars().all(|c| c.is_ascii_digit()) {
            return Err(ChError::invalid(format!(
                "sic_codes must be SIC 2007 codes of 4 or 5 digits, e.g. 62012; got {code:?}"
            )));
        }
        if !out.iter().any(|c| c == code) {
            out.push(code.to_string());
        }
    }
    if out.len() > MAX_SIC_CODES {
        return Err(ChError::invalid(format!(
            "sic_codes is limited to {MAX_SIC_CODES} codes"
        )));
    }
    Ok(out)
}

fn dedup<T: PartialEq + Copy>(values: Option<&[T]>) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for v in values.unwrap_or_default() {
        if !out.contains(v) {
            out.push(*v);
        }
    }
    out
}

/// The query string of an advanced search, from validated arguments.
pub(crate) fn filter_query(
    p: &FilterCompaniesParams,
) -> error::Result<Vec<(&'static str, String)>> {
    let mut q: Vec<(&'static str, String)> = Vec::new();
    if let Some(v) = text_arg("name_includes", p.name_includes.as_deref())? {
        q.push(("company_name_includes", v));
    }
    if let Some(v) = text_arg("name_excludes", p.name_excludes.as_deref())? {
        q.push(("company_name_excludes", v));
    }
    let status = dedup(p.status.as_deref());
    if !status.is_empty() {
        q.push((
            "company_status",
            codes::join(&status, CompanyStatus::as_str),
        ));
    }
    let types = dedup(p.company_type.as_deref());
    if !types.is_empty() {
        q.push(("company_type", codes::join(&types, CompanyType::as_str)));
    }
    let subtypes = dedup(p.subtype.as_deref());
    if !subtypes.is_empty() {
        q.push((
            "company_subtype",
            codes::join(&subtypes, CompanySubtype::as_str),
        ));
    }
    let sic = sic_codes_arg(p.sic_codes.as_deref())?;
    if !sic.is_empty() {
        q.push(("sic_codes", sic.join(",")));
    }
    if let Some(v) = text_arg("location", p.location.as_deref())? {
        q.push(("location", v));
    }
    let (inc_from, inc_to) = date_range(
        "incorporated",
        p.incorporated_from.as_deref(),
        p.incorporated_to.as_deref(),
    )?;
    let (dis_from, dis_to) = date_range(
        "dissolved",
        p.dissolved_from.as_deref(),
        p.dissolved_to.as_deref(),
    )?;
    for (key, date) in [
        ("incorporated_from", inc_from),
        ("incorporated_to", inc_to),
        ("dissolved_from", dis_from),
        ("dissolved_to", dis_to),
    ] {
        if let Some(d) = date {
            q.push((key, d.format("%Y-%m-%d").to_string()));
        }
    }
    if q.is_empty() {
        return Err(ChError::invalid(
            "give at least one filter: name_includes, name_excludes, status, company_type, subtype, sic_codes, location or a date",
        ));
    }
    q.push(("size", limit_arg(p.limit)?.to_string()));
    q.push(("start_index", p.start_index.unwrap_or(0).to_string()));
    Ok(q)
}

/// SIC codes with their descriptions, for a profile's `sic_codes`.
pub(crate) fn nature_of_business(codes: Option<&Value>) -> Option<Value> {
    let codes = codes?.as_array()?;
    let described: Vec<Value> = codes
        .iter()
        .filter_map(Value::as_str)
        .map(|c| match sic::describe(c) {
            Some(d) => json!({"code": c, "description": d}),
            None => json!({"code": c}),
        })
        .collect();
    (!described.is_empty()).then_some(Value::Array(described))
}

impl CompaniesServer {
    /// The profile of one company, shaped for output, with SIC descriptions.
    pub(crate) async fn profile(&self, number: &str) -> error::Result<Value> {
        let profile = self
            .get_optional("company_profile", &["company", number], &[])
            .await?
            .ok_or_else(|| {
                ChError::NotFound(format!(
                    "no company with number {number}; find the number with search_companies"
                ))
            })?;
        let mut out = company_profile(profile);
        if let Some(nature) = nature_of_business(out.get("sic_codes"))
            && let Value::Object(map) = &mut out
        {
            map.insert("nature_of_business".into(), nature);
        }
        Ok(out)
    }
}

#[tool_router(router = search_router, vis = "pub(crate)")]
impl CompaniesServer {
    #[tool(
        description = "Search the Companies House register by company name or number, best match first. Returns company number, name, status, type, incorporation date and registered office address for each match. Use get_company for the full record, and filter_companies to filter by status, type, SIC code, location or dates.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn search_companies(
        &self,
        Parameters(p): Parameters<SearchCompaniesParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "search_companies");
        run("search_companies", async {
            let query = required_text("query", &p.query)?;
            let limit = limit_arg(p.limit)?;
            let start = p.start_index.unwrap_or(0);
            let mut q: Query = vec![("q", query)];
            if p.active_only == Some(true) {
                q.push(("restrictions", "active-companies".to_string()));
            }
            let out = self
                .search_page(
                    "search_companies",
                    &["search", "companies"],
                    q,
                    limit,
                    start,
                    search_item,
                )
                .await?;
            Ok(json_result(&out))
        })
        .await
    }

    #[tool(
        description = "Find companies matching filters (the Companies House advanced search): words in the name, status, company type, subtype, SIC codes, registered office location, and incorporation or dissolution date ranges. Give at least one filter; filters combine with AND, values within one filter with OR. total is the number of matching companies, so limit 1 is enough to count them. Use lookup_sic_codes to find SIC codes for a kind of business.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn filter_companies(
        &self,
        Parameters(p): Parameters<FilterCompaniesParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "filter_companies");
        run("filter_companies", async {
            let q = filter_query(&p)?;
            let start = p.start_index.unwrap_or(0);
            // The advanced search answers 404 when no company matches.
            let found = self
                .get_optional("advanced_search", &["advanced-search", "companies"], &q)
                .await?
                .unwrap_or_default();
            let total = count(&found, &["hits"]).unwrap_or(0);
            let results = items(&found).iter().map(advanced_item).collect();
            Ok(json_result(&page(Some(total), start, results)))
        })
        .await
    }

    #[tool(
        description = "Read one company's profile from Companies House: name, status, type, jurisdiction, incorporation and cessation dates, registered office address, SIC codes with their descriptions (nature_of_business), previous names, next accounts and confirmation statement due dates (and whether overdue), and records_available listing the other records held, such as officers, filing_history, charges or insolvency.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn get_company(
        &self,
        Parameters(p): Parameters<CompanyNumberParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_company");
        run("get_company", async {
            let number = normalise_company_number(&p.company_number)?;
            Ok(json_result(&self.profile(&number).await?))
        })
        .await
    }

    #[tool(
        description = "Search companies that have been dissolved, by name. match_on previous_names finds companies by a name they had before. Returns number, name, incorporation and dissolution dates, last registered office and previous names.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn search_dissolved_companies(
        &self,
        Parameters(p): Parameters<SearchDissolvedParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "search_dissolved_companies");
        run("search_dissolved_companies", async {
            let query = required_text("query", &p.query)?;
            let limit = limit_arg(p.limit)?;
            let start = p.start_index.unwrap_or(0);
            let search_type = match p.match_on.unwrap_or(DissolvedMatch::BestMatch) {
                DissolvedMatch::BestMatch => "best-match",
                DissolvedMatch::PreviousNames => "previous-name-dissolved",
            };
            let q = vec![
                ("q", query),
                ("search_type", search_type.to_string()),
                ("size", limit.to_string()),
                ("start_index", start.to_string()),
            ];
            let found = self
                .get_optional("dissolved_search", &["dissolved-search", "companies"], &q)
                .await?
                .unwrap_or_default();
            let total = count(&found, &["hits"]).unwrap_or(0);
            let results = items(&found).iter().map(dissolved_item).collect();
            Ok(json_result(&page(Some(total), start, results)))
        })
        .await
    }

    #[tool(
        description = "Check whether a proposed company name is already taken: lists active companies whose names Companies House treats as the same (for example ignoring Limited or Ltd and punctuation). available is true when none exist. This does not check sensitive words or names that are merely too like an existing one, which Companies House can also refuse.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn check_company_name_availability(
        &self,
        Parameters(p): Parameters<NameAvailabilityParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "check_company_name_availability");
        run("check_company_name_availability", async {
            let name = required_text("name", &p.name)?;
            let q = vec![
                ("q", name.clone()),
                ("items_per_page", MAX_NEARBY.to_string()),
                (
                    "restrictions",
                    "active-companies legally-equivalent-company-name".to_string(),
                ),
            ];
            let found = self
                .get_optional("search_companies", &["search", "companies"], &q)
                .await?
                .unwrap_or_default();
            let conflicts: Vec<Value> = items(&found).iter().map(search_item).collect();
            let total = count(&found, &["total_results"]).unwrap_or(conflicts.len() as u64);
            Ok(json_result(&json!({
                "name": name,
                "available": total == 0,
                "conflicting_companies": conflicts,
                "note": "Only checks active companies with a legally equivalent name; Companies House may still refuse a name that is too like another or uses a sensitive word.",
            })))
        })
        .await
    }

    #[tool(
        description = "List companies in register (alphabetical) order around a name, e.g. to see similarly named companies or every company starting with a word. Returns number, name, status and type.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn browse_company_names(
        &self,
        Parameters(p): Parameters<BrowseCompanyNamesParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "browse_company_names");
        run("browse_company_names", async {
            let name = required_text("name", &p.name)?;
            let limit = limit_arg(p.limit)?.min(MAX_NEARBY);
            let q = vec![("q", name), ("size", limit.to_string())];
            let found = self
                .get_optional(
                    "alphabetical_search",
                    &["alphabetical-search", "companies"],
                    &q,
                )
                .await?
                .unwrap_or_default();
            let results: Vec<Value> = items(&found).iter().map(alphabetical_item).collect();
            Ok(json_result(
                &json!({ "count": results.len(), "items": results }),
            ))
        })
        .await
    }

    #[tool(
        description = "Find SIC 2007 nature-of-business codes, as Companies House uses them, by words (e.g. \"bakery\", \"software development\", \"letting property\") or code digits, or describe given codes. Use the codes with filter_companies. Matching uses the official wording, so if nothing matches try a broader or more formal word (e.g. \"bread\" for a bakery). Does not call Companies House.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    pub async fn lookup_sic_codes(
        &self,
        Parameters(p): Parameters<LookupSicCodesParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "lookup_sic_codes");
        run("lookup_sic_codes", async {
            let limit = limit_arg(p.limit)? as usize;
            let query = text_arg("query", p.query.as_deref())?;
            let codes = sic_codes_arg(p.codes.as_deref())?;
            if query.is_none() && codes.is_empty() {
                return Err(ChError::invalid("give a query or codes"));
            }
            let mut out = serde_json::Map::new();
            if !codes.is_empty() {
                let described: Vec<Value> = codes
                    .iter()
                    .map(|c| match sic::describe(c) {
                        Some(d) => json!({"code": c, "description": d}),
                        None => json!({"code": c, "description": null, "note": "not on the Companies House SIC 2007 list"}),
                    })
                    .collect();
                out.insert("codes".into(), Value::Array(described));
            }
            if let Some(q) = query {
                let matches: Vec<Value> = sic::search(&q, limit)
                    .into_iter()
                    .map(|c| json!({"code": c.code, "description": c.description, "section": c.section}))
                    .collect();
                out.insert("matches".into(), Value::Array(matches));
            }
            Ok(json_result(&Value::Object(out)))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(v: Value) -> FilterCompaniesParams {
        serde_json::from_value(v).expect("valid arguments")
    }

    #[test]
    fn filter_query_maps_every_argument() {
        let q = filter_query(&filter(json!({
            "name_includes": "  bakery ", "name_excludes": "cake",
            "status": ["active", "active", "liquidation"],
            "company_type": ["ltd", "llp"], "subtype": ["community-interest-company"],
            "sic_codes": ["10710", " 47240", "10710"], "location": "Leeds",
            "incorporated_from": "2020-01-01", "incorporated_to": "2024-12-31",
            "dissolved_from": "2025-01-01", "dissolved_to": "2025-06-30",
            "limit": 50, "start_index": 100
        })))
        .unwrap();
        let q: Vec<(&str, &str)> = q.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(
            q,
            vec![
                ("company_name_includes", "bakery"),
                ("company_name_excludes", "cake"),
                ("company_status", "active,liquidation"),
                ("company_type", "ltd,llp"),
                ("company_subtype", "community-interest-company"),
                ("sic_codes", "10710,47240"),
                ("location", "Leeds"),
                ("incorporated_from", "2020-01-01"),
                ("incorporated_to", "2024-12-31"),
                ("dissolved_from", "2025-01-01"),
                ("dissolved_to", "2025-06-30"),
                ("size", "50"),
                ("start_index", "100"),
            ]
        );
    }

    #[test]
    fn filter_query_needs_a_filter() {
        for args in [
            json!({}),
            json!({"name_includes": "  ", "limit": 5, "status": []}),
        ] {
            let e = filter_query(&filter(args)).unwrap_err();
            assert_eq!(e.kind(), "validation");
            assert!(e.to_string().contains("at least one filter"), "{e}");
        }
    }

    #[test]
    fn filter_query_validates_dates_and_codes() {
        let cases = [
            (json!({"incorporated_from": "01/02/2020"}), "YYYY-MM-DD"),
            (json!({"dissolved_to": "2025-02-30"}), "YYYY-MM-DD"),
            (
                json!({"incorporated_from": "2024-01-02", "incorporated_to": "2024-01-01"}),
                "incorporated_from must not be after incorporated_to",
            ),
            (json!({"sic_codes": ["6201a"]}), "SIC 2007"),
            (json!({"sic_codes": ["123"]}), "SIC 2007"),
            (json!({"location": "x".repeat(201)}), "200 characters"),
        ];
        for (args, msg) in cases {
            let e = filter_query(&filter(args.clone())).unwrap_err();
            assert_eq!(e.kind(), "validation", "{args}");
            assert!(e.to_string().contains(msg), "{args}: {e}");
        }
    }

    #[test]
    fn unknown_codes_fail_argument_parsing() {
        assert!(
            serde_json::from_value::<FilterCompaniesParams>(json!({"status": ["closing"]}))
                .is_err()
        );
    }

    #[test]
    fn nature_of_business_describes_known_codes() {
        assert_eq!(
            nature_of_business(Some(&json!(["62012", "12345"]))),
            Some(json!([
                {"code": "62012", "description": "Business and domestic software development"},
                {"code": "12345"}
            ]))
        );
        assert_eq!(nature_of_business(Some(&json!([]))), None);
        assert_eq!(nature_of_business(None), None);
    }
}
