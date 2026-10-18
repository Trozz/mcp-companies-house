//! People: company officers and their appointments, persons with significant control, and
//! disqualified officers.

use super::{
    CompaniesServer, Query, count, id_arg, items, json_result, limit_arg, normalise_company_number,
    nothing_listed, required_text, run,
};
use crate::error::{self, ChError};
use crate::view::people::{
    appointment_item, disqualified_search_item, officer_item, officer_search_item, psc_item,
    psc_statement_item,
};
use crate::view::{birth, detail, page};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Statements fetched with the first page of persons with significant control.
const MAX_STATEMENTS: u32 = 100;

/// Order of `list_officers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub enum OfficerOrder {
    AppointedOn,
    ResignedOn,
    Surname,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListOfficersParams {
    /// Company number, e.g. 00445790 or SC123456.
    pub company_number: String,
    /// Sort order. Default: Companies House's own order.
    pub order_by: Option<OfficerOrder>,
    /// Results per page, 1 to 100. Default: 20.
    pub limit: Option<u32>,
    /// Index of the first result, from 0. Pass next_start_index from the previous page. Default: 0.
    pub start_index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct OfficerAppointmentsParams {
    /// officer_id from list_officers or search_officers.
    pub officer_id: String,
    /// true: only appointments the officer still holds.
    pub active_only: Option<bool>,
    /// Results per page, 1 to 100. Default: 20.
    pub limit: Option<u32>,
    /// Index of the first result, from 0. Pass next_start_index from the previous page. Default: 0.
    pub start_index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchPeopleParams {
    /// Name to search for, e.g. "Jane Smith".
    pub query: String,
    /// Results per page, 1 to 100. Default: 20.
    pub limit: Option<u32>,
    /// Index of the first result, from 0. Pass next_start_index from the previous page. Default: 0.
    pub start_index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListPscParams {
    /// Company number, e.g. 00445790 or SC123456.
    pub company_number: String,
    /// Results per page, 1 to 100. Default: 20.
    pub limit: Option<u32>,
    /// Index of the first result, from 0. Pass next_start_index from the previous page. Default: 0.
    pub start_index: Option<u32>,
}

/// Whether a disqualified officer is a person or a company.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub enum DisqualifiedKind {
    Natural,
    Corporate,
}

impl DisqualifiedKind {
    fn segment(self) -> &'static str {
        match self {
            DisqualifiedKind::Natural => "natural",
            DisqualifiedKind::Corporate => "corporate",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct GetDisqualifiedOfficerParams {
    /// officer_id from search_disqualified_officers.
    pub officer_id: String,
    /// officer_kind from search_disqualified_officers. Default: try natural, then corporate.
    pub officer_kind: Option<DisqualifiedKind>,
}

/// The start index and paging parameters of a list request.
fn paging(limit: Option<u32>, start: Option<u32>) -> error::Result<(u32, Query)> {
    let limit = limit_arg(limit)?;
    let start = start.unwrap_or(0);
    Ok((
        start,
        vec![
            ("items_per_page", limit.to_string()),
            ("start_index", start.to_string()),
        ],
    ))
}

/// Adds the numeric `keys` of `source` to a page object.
fn with_counts(mut page: Value, source: &Value, keys: &[&str]) -> Value {
    if let Value::Object(map) = &mut page {
        for key in keys {
            if let Some(n) = source.get(*key).filter(|n| n.is_u64()) {
                map.insert((*key).to_string(), n.clone());
            }
        }
    }
    page
}

impl CompaniesServer {
    /// One page of a company's officers, shaped for output.
    pub(crate) async fn officers_page(
        &self,
        number: &str,
        query: &[(&str, String)],
        start: u32,
    ) -> error::Result<Value> {
        // An unknown company gets an empty list (or a 404); callers that need to tell "no
        // officers" from "no such company" check with ensure_company.
        let found = self
            .get_optional("company_officers", &["company", number, "officers"], query)
            .await?
            .unwrap_or_default();
        let results = items(&found).iter().map(officer_item).collect();
        Ok(with_counts(
            page(
                Some(count(&found, &["total_results"]).unwrap_or(0)),
                start,
                results,
            ),
            &found,
            &["active_count", "resigned_count", "inactive_count"],
        ))
    }

    /// One page of a company's persons with significant control plus, on the first page, its
    /// PSC statements. A company without either has empty lists.
    pub(crate) async fn psc_page(
        &self,
        number: &str,
        query: &[(&str, String)],
        start: u32,
    ) -> error::Result<Value> {
        let found = self
            .get_optional(
                "company_psc",
                &["company", number, "persons-with-significant-control"],
                query,
            )
            .await?
            .unwrap_or_default();
        let results = items(&found).iter().map(psc_item).collect();
        let mut out = with_counts(
            page(
                Some(count(&found, &["total_results"]).unwrap_or(0)),
                start,
                results,
            ),
            &found,
            &["active_count", "ceased_count"],
        );
        if start == 0 {
            // Statements are secondary: if they cannot be read, still return the PSCs.
            let statements = self
                .get_optional(
                    "company_psc_statements",
                    &[
                        "company",
                        number,
                        "persons-with-significant-control-statements",
                    ],
                    &[("items_per_page", MAX_STATEMENTS.to_string())],
                )
                .await;
            if let Value::Object(map) = &mut out {
                match statements {
                    Ok(found) => {
                        let found = found.unwrap_or_default();
                        let statements: Vec<Value> =
                            items(&found).iter().map(psc_statement_item).collect();
                        map.insert("statements".into(), Value::Array(statements));
                    }
                    Err(e) => {
                        tracing::warn!(kind = e.kind(), error = %e, "psc statements unavailable");
                        map.insert(
                            "statements_unavailable".into(),
                            json!(format!("{}: {e}", e.kind())),
                        );
                    }
                }
            }
        }
        Ok(out)
    }
}

#[tool_router(router = people_router, vis = "pub(crate)")]
impl CompaniesServer {
    #[tool(
        description = "List a company's officers (directors, secretaries, LLP members and others), current and resigned: role, appointment and resignation dates, occupation, nationality, country of residence, month and year of birth, correspondence address, and officer_id for get_officer_appointments. active_count and resigned_count give the totals.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn list_officers(
        &self,
        Parameters(p): Parameters<ListOfficersParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "list_officers");
        run("list_officers", async {
            let number = normalise_company_number(&p.company_number)?;
            let (start, mut q) = paging(p.limit, p.start_index)?;
            if let Some(order) = p.order_by {
                let o = match order {
                    OfficerOrder::AppointedOn => "appointed_on",
                    OfficerOrder::ResignedOn => "resigned_on",
                    OfficerOrder::Surname => "surname",
                };
                q.push(("order_by", o.to_string()));
            }
            let out = self.officers_page(&number, &q, start).await?;
            if nothing_listed(&out, start) {
                self.ensure_company(&number).await?;
            }
            Ok(json_result(&out))
        })
        .await
    }

    #[tool(
        description = "List every company appointment of one officer (other directorships and roles, current and past), with company number, name and status, role and dates. Takes officer_id from list_officers or search_officers.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn get_officer_appointments(
        &self,
        Parameters(p): Parameters<OfficerAppointmentsParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_officer_appointments");
        run("get_officer_appointments", async {
            let id = id_arg("officer_id", &p.officer_id)?;
            let (start, mut q) = paging(p.limit, p.start_index)?;
            if p.active_only == Some(true) {
                q.push(("filter", "active".to_string()));
            }
            let found = self
                .get_optional("officer_appointments", &["officers", &id, "appointments"], &q)
                .await?
                .ok_or_else(|| {
                    ChError::NotFound(format!(
                        "no officer with id {id}; take officer_id from list_officers or search_officers"
                    ))
                })?;
            let results = items(&found).iter().map(appointment_item).collect();
            let mut out = page(count(&found, &["total_results"]), start, results);
            if let Value::Object(map) = &mut out {
                map.insert("officer_id".into(), json!(id));
                if let Some(name) = found.get("name") {
                    map.insert("name".into(), name.clone());
                }
                if let Some(b) = birth(found.get("date_of_birth")) {
                    map.insert("date_of_birth".into(), b);
                }
                if let Some(c) = found.get("is_corporate_officer") {
                    map.insert("is_corporate_officer".into(), c.clone());
                }
            }
            Ok(json_result(&out))
        })
        .await
    }

    #[tool(
        description = "Search company officers (directors, secretaries and others) by name. Returns each match's officer_id for get_officer_appointments, number of appointments, month and year of birth and address. The same person can appear more than once.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn search_officers(
        &self,
        Parameters(p): Parameters<SearchPeopleParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "search_officers");
        run("search_officers", async {
            let query = required_text("query", &p.query)?;
            let limit = limit_arg(p.limit)?;
            let out = self
                .search_page(
                    "search_officers",
                    &["search", "officers"],
                    vec![("q", query)],
                    limit,
                    p.start_index.unwrap_or(0),
                    officer_search_item,
                )
                .await?;
            Ok(json_result(&out))
        })
        .await
    }

    #[tool(
        description = "List a company's persons with significant control (people, companies or legal persons owning or controlling it, and beneficial owners of overseas entities): kind, natures of control (for example ownership-of-shares-75-to-100-percent), notified and ceased dates, nationality, month and year of birth, address. The first page also includes statements, such as that the company knows of no person with significant control.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn list_persons_with_significant_control(
        &self,
        Parameters(p): Parameters<ListPscParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "list_persons_with_significant_control");
        run("list_persons_with_significant_control", async {
            let number = normalise_company_number(&p.company_number)?;
            let (start, q) = paging(p.limit, p.start_index)?;
            let out = self.psc_page(&number, &q, start).await?;
            if nothing_listed(&out, start) {
                self.ensure_company(&number).await?;
            }
            Ok(json_result(&out))
        })
        .await
    }

    #[tool(
        description = "Search people and companies disqualified from acting as company directors, by name. Returns officer_id and officer_kind for get_disqualified_officer, date of birth and address.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn search_disqualified_officers(
        &self,
        Parameters(p): Parameters<SearchPeopleParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "search_disqualified_officers");
        run("search_disqualified_officers", async {
            let query = required_text("query", &p.query)?;
            let limit = limit_arg(p.limit)?;
            let out = self
                .search_page(
                    "search_disqualified",
                    &["search", "disqualified-officers"],
                    vec![("q", query)],
                    limit,
                    p.start_index.unwrap_or(0),
                    disqualified_search_item,
                )
                .await?;
            Ok(json_result(&out))
        })
        .await
    }

    #[tool(
        description = "Read a disqualified officer's record: each disqualification's type, dates, reason, the companies where the misconduct happened and the court, plus any permissions to act. Takes officer_id and officer_kind from search_disqualified_officers.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn get_disqualified_officer(
        &self,
        Parameters(p): Parameters<GetDisqualifiedOfficerParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_disqualified_officer");
        run("get_disqualified_officer", async {
            let id = id_arg("officer_id", &p.officer_id)?;
            let kinds = match p.officer_kind {
                Some(k) => vec![k],
                None => vec![DisqualifiedKind::Natural, DisqualifiedKind::Corporate],
            };
            for kind in kinds {
                if let Some(v) = self
                    .get_optional(
                        "disqualified_officer",
                        &["disqualified-officers", kind.segment(), &id],
                        &[],
                    )
                    .await?
                {
                    let mut out = Map::new();
                    out.insert("officer_id".into(), json!(id));
                    out.insert("officer_kind".into(), json!(kind.segment()));
                    if let Value::Object(rest) = detail(v) {
                        out.extend(rest);
                    }
                    return Ok(json_result(&Value::Object(out)));
                }
            }
            Err(ChError::NotFound(format!(
                "no disqualified officer with id {id}; take officer_id from search_disqualified_officers"
            )))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_added_to_the_page() {
        let p = page(Some(5), 0, vec![json!(1)]);
        let v = with_counts(
            p,
            &json!({"active_count": 3, "resigned_count": "2", "x": 1}),
            &["active_count", "resigned_count"],
        );
        assert_eq!(
            v,
            json!({"total": 5, "active_count": 3, "start_index": 0, "count": 1, "next_start_index": 1, "items": [1]})
        );
    }

    #[test]
    fn paging_validates_the_limit() {
        let (start, q) = paging(Some(5), Some(10)).unwrap();
        assert_eq!(start, 10);
        assert_eq!(q[0], ("items_per_page", "5".to_string()));
        assert_eq!(paging(Some(0), None).unwrap_err().kind(), "validation");
    }
}
