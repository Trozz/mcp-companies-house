//! Records about a company: filing history, charges (secured lending) and insolvency.

use super::{
    Caller, CompaniesServer, count, id_arg, items, json_result, limit_arg,
    normalise_company_number, nothing_listed, run,
};
use crate::error::{self, ChError};
use crate::view::page;
use crate::view::records::{
    charge_detail, charge_item, exemptions, filing_item, insolvency, registers,
    uk_establishment_item,
};
use axum::http::request::Parts;
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_CATEGORIES: usize = 20;

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListFilingHistoryParams {
    /// Company number, e.g. 00445790 or SC123456.
    pub company_number: String,
    /// Only filings in these categories, e.g. ["accounts"]. Categories include accounts, address, annual-return, capital, change-of-name, confirmation-statement, incorporation, insolvency, liquidation, miscellaneous, mortgage (charges), officers, persons-with-significant-control and resolution.
    pub categories: Option<Vec<String>>,
    /// Results per page, 1 to 100. Default: 20.
    pub limit: Option<u32>,
    /// Index of the first result, from 0. Pass next_start_index from the previous page. Default: 0.
    pub start_index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ListChargesParams {
    /// Company number, e.g. 00445790 or SC123456.
    pub company_number: String,
    /// Results per page, 1 to 100. Default: 20.
    pub limit: Option<u32>,
    /// Index of the first result, from 0. Pass next_start_index from the previous page. Default: 0.
    pub start_index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct GetChargeParams {
    /// Company number, e.g. 00445790 or SC123456.
    pub company_number: String,
    /// charge_id from list_charges.
    pub charge_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct GetFilingParams {
    /// Company number, e.g. 00445790 or SC123456.
    pub company_number: String,
    /// transaction_id from list_filing_history.
    pub transaction_id: String,
}

fn categories_arg(categories: Option<&[String]>) -> error::Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for raw in categories.unwrap_or_default() {
        let c = raw.trim().to_ascii_lowercase();
        if c.is_empty() || c.len() > 40 || !c.chars().all(|ch| ch.is_ascii_lowercase() || ch == '-')
        {
            return Err(ChError::invalid(format!(
                "categories must be Companies House category names such as accounts or officers; got {raw:?}"
            )));
        }
        if !out.contains(&c) {
            out.push(c);
        }
    }
    if out.len() > MAX_CATEGORIES {
        return Err(ChError::invalid(format!(
            "categories is limited to {MAX_CATEGORIES} names"
        )));
    }
    Ok(out)
}

impl Caller {
    /// One page of a company's filing history, newest first, shaped for output.
    pub(crate) async fn filings_page(
        &self,
        number: &str,
        query: &[(&str, String)],
        start: u32,
    ) -> error::Result<Value> {
        let found = self
            .get_company_resource(
                "filing_history",
                number,
                "filing history",
                &["filing-history"],
                query,
            )
            .await?;
        let results = items(&found).iter().map(filing_item).collect();
        Ok(page(count(&found, &["total_count"]), start, results))
    }

    /// One page of a company's charges, shaped for output. A company without charges has an
    /// empty page.
    pub(crate) async fn charges_page(
        &self,
        number: &str,
        query: &[(&str, String)],
        start: u32,
    ) -> error::Result<Value> {
        let found = self
            .get_optional("company_charges", &["company", number, "charges"], query)
            .await?
            .unwrap_or_default();
        let results = items(&found).iter().map(charge_item).collect();
        let mut out = page(
            Some(count(&found, &["total_count"]).unwrap_or(0)),
            start,
            results,
        );
        if let Value::Object(map) = &mut out {
            for key in ["satisfied_count", "part_satisfied_count"] {
                if let Some(n) = found.get(key).filter(|n| n.is_u64()) {
                    map.insert(key.to_string(), n.clone());
                }
            }
        }
        Ok(out)
    }

    /// A company's exemptions, shaped for output; `None` when it has none.
    pub(crate) async fn exemptions_record(&self, number: &str) -> error::Result<Option<Value>> {
        Ok(self
            .get_optional(
                "company_exemptions",
                &["company", number, "exemptions"],
                &[],
            )
            .await?
            .map(|v| exemptions(&v)))
    }

    /// A company's insolvency record, shaped for output; `None` when it has none.
    pub(crate) async fn insolvency_record(&self, number: &str) -> error::Result<Option<Value>> {
        Ok(self
            .get_optional(
                "company_insolvency",
                &["company", number, "insolvency"],
                &[],
            )
            .await?
            .map(|v| insolvency(&v)))
    }
}

#[tool_router(router = records_router, vis = "pub(crate)")]
impl CompaniesServer {
    #[tool(
        description = "List a company's filing history, newest first: date, form type (e.g. AA, CS01), category, the Companies House description key (e.g. accounts-with-accounts-type-micro-entity) with its values (e.g. made_up_date), transaction_id and document_id. Filter with categories, e.g. [\"accounts\"].",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn list_filing_history(
        &self,
        Parameters(p): Parameters<ListFilingHistoryParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "list_filing_history");
        run("list_filing_history", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let limit = limit_arg(p.limit)?;
            let start = p.start_index.unwrap_or(0);
            let mut q = vec![
                ("items_per_page", limit.to_string()),
                ("start_index", start.to_string()),
            ];
            let categories = categories_arg(p.categories.as_deref())?;
            if !categories.is_empty() {
                q.push(("category", categories.join(",")));
            }
            Ok(json_result(&c.filings_page(&number, &q, start).await?))
        })
        .await
    }

    #[tool(
        description = "Read one filing from a company's filing history: date, form type, category, description key and values, page count, transaction_id and document_id. This is the filing record, not the document itself: pass document_id to download_filing_document for that. Takes transaction_id from list_filing_history.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn get_filing(
        &self,
        Parameters(p): Parameters<GetFilingParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_filing");
        run("get_filing", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let id = id_arg("transaction_id", &p.transaction_id)?;
            let found = c
                .get_optional(
                    "company_filing",
                    &["company", &number, "filing-history", &id],
                    &[],
                )
                .await?
                .ok_or_else(|| {
                    ChError::NotFound(format!(
                        "no filing {id} for company {number}; take transaction_id from list_filing_history"
                    ))
                })?;
            Ok(json_result(&filing_item(&found)))
        })
        .await
    }

    #[tool(
        description = "Say where a company keeps its statutory registers (directors, secretaries, persons with significant control, members, and directors' residential addresses), with the history of each move. location is public-register (the register is held at Companies House), registered-office, single-alternative-inspection-location or unspecified-location. Most companies have told Companies House nothing, which means their registers are kept at their registered office.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn get_company_registers(
        &self,
        Parameters(p): Parameters<super::CompanyNumberParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_company_registers");
        run("get_company_registers", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let found = c
                .get_optional("company_registers", &["company", &number, "registers"], &[])
                .await?;
            let out = match found {
                Some(v) => registers(&v),
                None => {
                    c.ensure_company(&number).await?;
                    json!({
                        "registers": [],
                        "note": "Companies House holds no register information for this company, which usually means it keeps its registers at its registered office",
                    })
                }
            };
            Ok(json_result(&out))
        })
        .await
    }

    #[tool(
        description = "List a company's exemptions from disclosure: for example from the persons with significant control rules because its shares trade on a regulated market. Each exemption has the periods it applies for and current (still applies). Most companies have none.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn get_exemptions(
        &self,
        Parameters(p): Parameters<super::CompanyNumberParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_exemptions");
        run("get_exemptions", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let out = match c.exemptions_record(&number).await? {
                Some(v) => v,
                None => {
                    c.ensure_company(&number).await?;
                    json!({
                        "exemptions": [],
                        "note": "Companies House holds no exemptions for this company",
                    })
                }
            };
            Ok(json_result(&out))
        })
        .await
    }

    #[tool(
        description = "List the UK establishments (branches) of an overseas company, each with its own company number (BR...), name, status and locality. For overseas companies (numbers starting FC); other companies have none.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn get_uk_establishments(
        &self,
        Parameters(p): Parameters<super::CompanyNumberParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_uk_establishments");
        run("get_uk_establishments", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let found = c
                .get_optional(
                    "company_uk_establishments",
                    &["company", &number, "uk-establishments"],
                    &[],
                )
                .await?
                .unwrap_or_default();
            let results: Vec<Value> = items(&found).iter().map(uk_establishment_item).collect();
            if results.is_empty() {
                c.ensure_company(&number).await?;
            }
            let total = results.len();
            Ok(json_result(&json!({"total": total, "items": results})))
        })
        .await
    }

    #[tool(
        description = "List the charges (mortgages and other secured lending) registered against a company: status (outstanding, satisfied, part-satisfied), who is entitled to them (usually the lender), what they cover, and created, delivered and satisfied dates. satisfied_count and part_satisfied_count give the totals. Takes charge_id to get_charge for the full record.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn list_charges(
        &self,
        Parameters(p): Parameters<ListChargesParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "list_charges");
        run("list_charges", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let limit = limit_arg(p.limit)?;
            let start = p.start_index.unwrap_or(0);
            let q = vec![
                ("items_per_page", limit.to_string()),
                ("start_index", start.to_string()),
            ];
            let out = c.charges_page(&number, &q, start).await?;
            if nothing_listed(&out, start) {
                c.ensure_company(&number).await?;
            }
            Ok(json_result(&out))
        })
        .await
    }

    #[tool(
        description = "Read one charge in full: classification, particulars (what is charged, fixed or floating), amount or obligations secured, persons entitled, the filings made about it, and any insolvency cases. Takes charge_id from list_charges.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn get_charge(
        &self,
        Parameters(p): Parameters<GetChargeParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_charge");
        run("get_charge", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let id = id_arg("charge_id", &p.charge_id)?;
            let found = c
                .get_optional("company_charge", &["company", &number, "charges", &id], &[])
                .await?
                .ok_or_else(|| {
                    ChError::NotFound(format!(
                        "no charge {id} for company {number}; take charge_id from list_charges"
                    ))
                })?;
            Ok(json_result(&charge_detail(found)))
        })
        .await
    }

    #[tool(
        description = "Read a company's insolvency history: status and each case (for example creditors-voluntary-liquidation or in-administration) with its dates and insolvency practitioners. A company with no insolvency record returns no cases.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn get_insolvency(
        &self,
        Parameters(p): Parameters<super::CompanyNumberParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_insolvency");
        run("get_insolvency", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let out = match c.insolvency_record(&number).await? {
                Some(v) => v,
                None => {
                    c.ensure_company(&number).await?;
                    json!({
                    "cases": [],
                        "note": "Companies House holds no insolvency record for this company",
                    })
                }
            };
            Ok(json_result(&out))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_are_validated_and_deduplicated() {
        assert_eq!(
            categories_arg(Some(&[
                " Accounts".into(),
                "officers".into(),
                "accounts".into()
            ]))
            .unwrap(),
            vec!["accounts", "officers"]
        );
        assert!(categories_arg(None).unwrap().is_empty());
        for bad in ["", "acc ounts", "a,b", "accounts1"] {
            assert_eq!(
                categories_arg(Some(&[bad.to_string()])).unwrap_err().kind(),
                "validation",
                "{bad}"
            );
        }
    }
}
