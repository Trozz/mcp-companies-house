//! `company_report`: one call that gathers what a due-diligence question usually needs.

use super::{CompaniesServer, json_result, normalise_company_number, run};
use crate::error::{self, ChError};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

pub const DEFAULT_FILINGS: u32 = 10;
pub const MAX_FILINGS: u32 = 50;
/// Officers, PSCs and charges read for the report: the first page of this size.
const SECTION_PAGE: u32 = 100;

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CompanyReportParams {
    /// Company number, e.g. 00445790 or SC123456.
    pub company_number: String,
    /// How many recent filings to include, 0 to 50. Default: 10.
    pub filings: Option<u32>,
}

fn paging(size: u32) -> Vec<(&'static str, String)> {
    vec![
        ("items_per_page", size.to_string()),
        ("start_index", "0".to_string()),
    ]
}

/// The items of a page that have no `key` (resigned_on, ceased_on), i.e. current ones.
fn current(page: &Value, key: &str) -> Vec<Value> {
    page.get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|i| i.get(key).is_none())
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn copy(from: &Value, to: &mut Map<String, Value>, keys: &[&str]) {
    for k in keys {
        if let Some(v) = from.get(*k) {
            to.insert((*k).to_string(), v.clone());
        }
    }
}

/// Whether more items exist than the first page held.
fn truncated(page: &Value) -> bool {
    page.get("next_start_index").is_some()
}

fn officers_section(page: Value) -> Value {
    let mut out = Map::new();
    copy(&page, &mut out, &["active_count", "resigned_count"]);
    out.insert("active".into(), Value::Array(current(&page, "resigned_on")));
    if truncated(&page) {
        out.insert("truncated".into(), json!(true));
    }
    Value::Object(out)
}

fn psc_section(page: Value) -> Value {
    let mut out = Map::new();
    copy(&page, &mut out, &["active_count", "ceased_count"]);
    out.insert("active".into(), Value::Array(current(&page, "ceased_on")));
    copy(&page, &mut out, &["statements_unavailable"]);
    let statements: Vec<Value> = page
        .get("statements")
        .and_then(Value::as_array)
        .map(|s| {
            s.iter()
                .filter(|st| st.get("ceased_on").is_none())
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if !statements.is_empty() {
        out.insert("statements".into(), Value::Array(statements));
    }
    if truncated(&page) {
        out.insert("truncated".into(), json!(true));
    }
    Value::Object(out)
}

fn charges_section(page: Value) -> Value {
    let mut out = Map::new();
    copy(
        &page,
        &mut out,
        &["total", "satisfied_count", "part_satisfied_count"],
    );
    let open: Vec<Value> = page
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|c| {
                    !matches!(
                        c.get("status").and_then(Value::as_str),
                        Some("satisfied" | "fully-satisfied")
                    )
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    out.insert("not_satisfied".into(), Value::Array(open));
    if truncated(&page) {
        out.insert("truncated".into(), json!(true));
    }
    Value::Object(out)
}

/// Puts a section into the report, or its error into `unavailable`.
fn section(
    report: &mut Map<String, Value>,
    unavailable: &mut Map<String, Value>,
    name: &str,
    result: error::Result<Value>,
) {
    match result {
        Ok(v) => {
            report.insert(name.to_string(), v);
        }
        Err(e) => {
            unavailable.insert(name.to_string(), json!(format!("{}: {e}", e.kind())));
        }
    }
}

#[tool_router(router = report_router, vis = "pub(crate)")]
impl CompaniesServer {
    #[tool(
        description = "One-call overview of a company for due diligence: its profile, current officers, current persons with significant control (and PSC statements), the most recent filings, charges not yet satisfied and any insolvency cases. Uses up to 6 Companies House requests; sections that could not be read are listed under unavailable. Use the list_ and get_ tools for full or older records.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn company_report(
        &self,
        Parameters(p): Parameters<CompanyReportParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "company_report");
        run("company_report", async {
            let number = normalise_company_number(&p.company_number)?;
            let filings = p.filings.unwrap_or(DEFAULT_FILINGS);
            if filings > MAX_FILINGS {
                return Err(ChError::invalid(format!(
                    "filings must be between 0 and {MAX_FILINGS}"
                )));
            }
            let profile = self.profile(&number).await?;
            let records: Vec<&str> = profile
                .get("records_available")
                .and_then(Value::as_array)
                .map(|r| r.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let has_charges = records.contains(&"charges");
            let has_insolvency = records.contains(&"insolvency");

            let section_page = paging(SECTION_PAGE);
            let filing_page = paging(filings.max(1));
            let (officers, psc, recent, charges, insolvency) = tokio::join!(
                self.officers_page(&number, &section_page, 0),
                self.psc_page(&number, &section_page, 0),
                async {
                    if filings == 0 {
                        return Ok(None);
                    }
                    self.filings_page(&number, &filing_page, 0).await.map(Some)
                },
                async {
                    if !has_charges {
                        return Ok(None);
                    }
                    self.charges_page(&number, &section_page, 0).await.map(Some)
                },
                async {
                    if !has_insolvency {
                        return Ok(None);
                    }
                    self.insolvency_record(&number).await
                },
            );

            let mut report = Map::new();
            let mut unavailable = Map::new();
            report.insert("company".into(), profile);
            section(
                &mut report,
                &mut unavailable,
                "officers",
                officers.map(officers_section),
            );
            section(
                &mut report,
                &mut unavailable,
                "persons_with_significant_control",
                psc.map(psc_section),
            );
            match recent {
                Ok(Some(page)) => {
                    report.insert(
                        "recent_filings".into(),
                        page.get("items").cloned().unwrap_or_else(|| json!([])),
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    unavailable
                        .insert("recent_filings".into(), json!(format!("{}: {e}", e.kind())));
                }
            }
            match charges {
                Ok(Some(page)) => {
                    report.insert("charges".into(), charges_section(page));
                }
                Ok(None) => {
                    report.insert("charges".into(), json!({"total": 0}));
                }
                Err(e) => {
                    unavailable.insert("charges".into(), json!(format!("{}: {e}", e.kind())));
                }
            }
            match insolvency {
                Ok(Some(v)) => {
                    report.insert("insolvency".into(), v);
                }
                Ok(None) => {
                    report.insert("insolvency".into(), json!({"cases": []}));
                }
                Err(e) => {
                    unavailable.insert("insolvency".into(), json!(format!("{}: {e}", e.kind())));
                }
            }
            if !unavailable.is_empty() {
                report.insert("unavailable".into(), Value::Object(unavailable));
            }
            Ok(json_result(&Value::Object(report)))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_keep_only_current_entries() {
        let officers = json!({
            "total": 3, "active_count": 2, "resigned_count": 1, "start_index": 0, "count": 3,
            "items": [{"name": "A"}, {"name": "B", "resigned_on": "2020-01-01"}, {"name": "C"}]
        });
        assert_eq!(
            officers_section(officers),
            json!({"active_count": 2, "resigned_count": 1, "active": [{"name": "A"}, {"name": "C"}]})
        );
        let psc = json!({
            "items": [{"name": "X", "ceased_on": "2019-01-01"}], "active_count": 0, "ceased_count": 1,
            "statements": [{"statement": "no-individual-or-entity-with-signficant-control"},
                           {"statement": "old", "ceased_on": "2018-01-01"}],
            "next_start_index": 100
        });
        assert_eq!(
            psc_section(psc),
            json!({"active_count": 0, "ceased_count": 1, "active": [],
                   "statements": [{"statement": "no-individual-or-entity-with-signficant-control"}],
                   "truncated": true})
        );
        let charges = json!({
            "total": 3, "satisfied_count": 1, "part_satisfied_count": 1,
            "items": [{"charge_id": "a", "status": "outstanding"}, {"charge_id": "b", "status": "fully-satisfied"},
                      {"charge_id": "c", "status": "part-satisfied"}, {"charge_id": "d", "status": "satisfied"}]
        });
        assert_eq!(
            charges_section(charges),
            json!({"total": 3, "satisfied_count": 1, "part_satisfied_count": 1,
                   "not_satisfied": [{"charge_id": "a", "status": "outstanding"}, {"charge_id": "c", "status": "part-satisfied"}]})
        );
    }
}
