//! People: company officers and their appointments, persons with significant control, and
//! disqualified officers.

use super::{
    Caller, CompaniesServer, Query, count, id_arg, items, json_result, limit_arg,
    normalise_company_number, nothing_listed, required_text, run,
};
use crate::error::{self, ChError};
use crate::view::people::{
    appointment_item, disqualified_search_item, officer_item, officer_search_item, psc_item,
    psc_statement_item,
};
use crate::view::{birth, detail, page};
use axum::http::request::Parts;
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Statements fetched with the first page of persons with significant control.
const MAX_STATEMENTS: u32 = 100;
/// Officers or appointments read per request when a tool reads a whole list.
const READ_PAGE: u32 = 100;
/// Requests `list_officers` makes at most to fill a page of current officers.
const MAX_OFFICER_READS: u32 = 5;
/// Requests `officer_network` makes at most for one officer's appointments.
const MAX_NETWORK_READS: u32 = 10;
/// Company statuses after which an appointment no longer counts as current.
const CLOSED_STATUSES: &[&str] = &["dissolved", "converted-closed", "closed", "removed"];
/// Candidates listed when an officer name matches no one, or more than one officer.
const MAX_CANDIDATES: usize = 10;

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
    /// true: only officers still in post. Companies House cannot filter officers, so the list is read from start_index (up to 500 officers) until the page is full; officers_read says how many were read and next_start_index continues from there. Default: false (current and resigned).
    pub current_only: Option<bool>,
    /// Results per page, 1 to 100. Default: 20.
    pub limit: Option<u32>,
    /// Index of the first result, from 0. Pass next_start_index from the previous page. Default: 0.
    pub start_index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct OfficerNetworkParams {
    /// officer_id from list_officers or search_officers. Preferred: officer names are not unique.
    pub officer_id: Option<String>,
    /// The officer's full name, used only without officer_id, and only when it matches exactly one officer in search_officers.
    pub officer_name: Option<String>,
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

/// A name for comparison: lower case, without titles or commas, words sorted, so that
/// "SMITH, Jane Anne" and "Mrs Jane Anne Smith" compare equal.
fn name_key(name: &str) -> Vec<String> {
    const TITLES: &[&str] = &[
        "mr", "mrs", "ms", "miss", "mx", "dr", "sir", "dame", "lord", "lady", "prof",
    ];
    let mut words: Vec<String> = name
        .replace([',', '.'], " ")
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|w| !TITLES.contains(&w.as_str()))
        .collect();
    words.sort();
    words
}

/// One search result on a line, for listing candidates in a message.
fn candidate(item: &Value) -> String {
    let id = item
        .get("officer_id")
        .and_then(Value::as_str)
        .unwrap_or("?");
    let mut about: Vec<String> = Vec::new();
    if let Some(n) = item.get("name").and_then(Value::as_str) {
        about.push(n.to_string());
    }
    if let Some(b) = item.get("date_of_birth").and_then(Value::as_str) {
        about.push(format!("born {b}"));
    }
    if let Some(n) = item.get("appointment_count").and_then(Value::as_u64) {
        about.push(format!("{n} appointments"));
    }
    if let Some(a) = item.get("address").and_then(Value::as_str) {
        about.push(a.to_string());
    }
    format!("{id} ({})", about.join(", "))
}

/// Whether an appointment is still held: not resigned, and the company not dissolved or closed.
fn is_current(appointment: &Value) -> bool {
    appointment.get("resigned_on").is_none()
        && !appointment
            .get("company_status")
            .and_then(Value::as_str)
            .is_some_and(|s| CLOSED_STATUSES.contains(&s))
}

impl Caller {
    /// Officers still in post, from position `start` of the company's officer list: pages are
    /// read until `limit` are found, the list ends or [`MAX_OFFICER_READS`] pages were read.
    pub(crate) async fn current_officers_page(
        &self,
        number: &str,
        order_by: Option<&'static str>,
        limit: u32,
        start: u32,
    ) -> error::Result<Value> {
        let mut found: Vec<Value> = Vec::new();
        let mut first: Option<Value> = None;
        let mut position = start;
        let mut next: Option<u32> = None;
        'read: for _ in 0..MAX_OFFICER_READS {
            let mut q: Query = vec![
                ("items_per_page", READ_PAGE.to_string()),
                ("start_index", position.to_string()),
            ];
            if let Some(o) = order_by {
                q.push(("order_by", o.to_string()));
            }
            let page = self
                .get_optional("company_officers", &["company", number, "officers"], &q)
                .await?
                .unwrap_or_default();
            let total = count(&page, &["total_results"]).unwrap_or(0);
            let listed = items(&page).to_vec();
            if first.is_none() {
                first = Some(page);
            }
            if listed.is_empty() {
                next = None;
                break;
            }
            for (i, item) in listed.iter().enumerate() {
                if item.get("resigned_on").is_none() {
                    found.push(officer_item(item));
                    if found.len() == limit as usize {
                        let after = position + i as u32 + 1;
                        next = (u64::from(after) < total).then_some(after);
                        position = after;
                        break 'read;
                    }
                }
            }
            position += listed.len() as u32;
            next = (u64::from(position) < total).then_some(position);
            if next.is_none() {
                break;
            }
        }
        let first = first.unwrap_or_default();
        let mut out = Map::new();
        for key in ["active_count", "resigned_count", "inactive_count"] {
            if let Some(n) = first.get(key).filter(|n| n.is_u64()) {
                out.insert(key.to_string(), n.clone());
            }
        }
        out.insert("start_index".into(), json!(start));
        out.insert("count".into(), json!(found.len()));
        out.insert("officers_read".into(), json!(position - start));
        if let Some(n) = next {
            out.insert("next_start_index".into(), json!(n));
            if found.len() < limit as usize {
                out.insert(
                    "note".into(),
                    json!(format!(
                        "read {} officers without filling the page; pass next_start_index to read on",
                        position - start
                    )),
                );
            }
        }
        out.insert("items".into(), Value::Array(found));
        Ok(Value::Object(out))
    }

    /// The officer id of the one officer whose name is `name`, by search_officers.
    async fn officer_named(&self, name: &str) -> error::Result<String> {
        let found = self
            .get_optional(
                "search_officers",
                &["search", "officers"],
                &[
                    ("q", name.to_string()),
                    ("items_per_page", READ_PAGE.to_string()),
                    ("start_index", "0".to_string()),
                ],
            )
            .await?
            .unwrap_or_default();
        let results: Vec<Value> = items(&found).iter().map(officer_search_item).collect();
        let wanted = name_key(name);
        let exact: Vec<&Value> = results
            .iter()
            .filter(|r| {
                r.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| name_key(n) == wanted)
            })
            .collect();
        let list = |items: &[&Value]| {
            items
                .iter()
                .take(MAX_CANDIDATES)
                .map(|i| candidate(i))
                .collect::<Vec<_>>()
                .join("; ")
        };
        match exact.as_slice() {
            [one] => one
                .get("officer_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    ChError::Upstream(format!("the officer named {name} has no officer id"))
                }),
            [] if results.is_empty() => Err(ChError::NotFound(format!(
                "no officer is named {name}; check the name with search_officers"
            ))),
            [] => {
                let close: Vec<&Value> = results.iter().collect();
                Err(ChError::invalid(format!(
                    "no officer is named exactly {name}; pass officer_id instead. Closest matches: {}",
                    list(&close)
                )))
            }
            many => Err(ChError::invalid(format!(
                "{} officers are named {name} (names are not unique, and one person can have several officer ids); pass the officer_id of one: {}",
                many.len(),
                list(many)
            ))),
        }
    }

    /// Why a company lists no persons with significant control and no PSC statements: an
    /// exemption, or a gap.
    pub(crate) async fn psc_absence_note(&self, number: &str) -> String {
        match self.exemptions_record(number).await {
            Ok(found) => {
                let current: Vec<String> = found
                    .as_ref()
                    .and_then(|v| v.get("exemptions"))
                    .and_then(Value::as_array)
                    .map(|all| {
                        all.iter()
                            .filter(|e| e.get("current") == Some(&Value::Bool(true)))
                            .filter_map(|e| e.get("exemption").and_then(Value::as_str))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                if current.is_empty() {
                    "Companies House records no person with significant control, no PSC statement and no PSC exemption for this company. A UK company or LLP should have filed one of these, so this may be a gap in the register; some kinds of company, such as overseas companies, do not report PSCs".to_string()
                } else {
                    format!(
                        "no persons with significant control are listed because the company is exempt from the PSC rules: {}",
                        current.join(", ")
                    )
                }
            }
            Err(e) => format!(
                "Companies House lists no person with significant control and no PSC statement; whether the company is exempt could not be checked ({}: {e})",
                e.kind()
            ),
        }
    }

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
        description = "List a company's officers (directors, secretaries, LLP members and others), current and resigned: role, appointment and resignation dates, occupation, nationality, country of residence, month and year of birth, correspondence address, and officer_id for get_officer_appointments. active_count and resigned_count give the totals. current_only lists only officers still in post.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn list_officers(
        &self,
        Parameters(p): Parameters<ListOfficersParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "list_officers");
        run("list_officers", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let (start, mut q) = paging(p.limit, p.start_index)?;
            let order = p.order_by.map(|order| match order {
                OfficerOrder::AppointedOn => "appointed_on",
                OfficerOrder::ResignedOn => "resigned_on",
                OfficerOrder::Surname => "surname",
            });
            let out = if p.current_only == Some(true) {
                c.current_officers_page(&number, order, limit_arg(p.limit)?, start)
                    .await?
            } else {
                if let Some(o) = order {
                    q.push(("order_by", o.to_string()));
                }
                c.officers_page(&number, &q, start).await?
            };
            if nothing_listed(&out, start) {
                c.ensure_company(&number).await?;
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
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_officer_appointments");
        run("get_officer_appointments", async {
            let c = self.caller(&parts)?;
            let id = id_arg("officer_id", &p.officer_id)?;
            let (start, mut q) = paging(p.limit, p.start_index)?;
            if p.active_only == Some(true) {
                q.push(("filter", "active".to_string()));
            }
            let found = c
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
        description = "Map an officer's network: every company they are or were appointed to, split into current and past (resigned, or the company dissolved or closed), with each company's status, the officer's role and dates, and a count of the companies by status. Reads the whole appointment list (up to 1000 appointments). Takes officer_id (preferred) or officer_name, which must match exactly one officer in search_officers because names are not unique.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn officer_network(
        &self,
        Parameters(p): Parameters<OfficerNetworkParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "officer_network");
        run("officer_network", async {
            let c = self.caller(&parts)?;
            let given_id = p
                .officer_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let id = match (given_id, super::text_arg("officer_name", p.officer_name.as_deref())?) {
                (Some(id), _) => id_arg("officer_id", id)?,
                (None, Some(name)) => c.officer_named(&name).await?,
                (None, None) => {
                    return Err(ChError::invalid(
                        "pass officer_id (from list_officers or search_officers) or officer_name",
                    ));
                }
            };
            let mut appointments: Vec<Value> = Vec::new();
            let mut head: Option<Value> = None;
            let mut total: u64 = 0;
            let mut position: u32 = 0;
            for _ in 0..MAX_NETWORK_READS {
                let q: Query = vec![
                    ("items_per_page", READ_PAGE.to_string()),
                    ("start_index", position.to_string()),
                ];
                let found = c
                    .get_optional("officer_appointments", &["officers", &id, "appointments"], &q)
                    .await?;
                let Some(found) = found else {
                    if head.is_none() {
                        return Err(ChError::NotFound(format!(
                            "no officer with id {id}; take officer_id from list_officers or search_officers"
                        )));
                    }
                    break;
                };
                total = count(&found, &["total_results"]).unwrap_or(0);
                let listed = items(&found);
                appointments.extend(listed.iter().map(appointment_item));
                position += listed.len() as u32;
                let done = listed.is_empty() || u64::from(position) >= total;
                if head.is_none() {
                    head = Some(found);
                }
                if done {
                    break;
                }
            }
            let head = head.unwrap_or_default();
            let (current, past): (Vec<Value>, Vec<Value>) =
                appointments.iter().cloned().partition(is_current);
            let mut statuses: std::collections::BTreeMap<String, u64> = Default::default();
            let mut seen = std::collections::BTreeSet::new();
            for a in &appointments {
                let number = a.get("company_number").and_then(Value::as_str);
                if let Some(n) = number
                    && !seen.insert(n.to_string())
                {
                    continue;
                }
                let status = a
                    .get("company_status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                *statuses.entry(status.to_string()).or_default() += 1;
            }
            let mut out = Map::new();
            out.insert("officer_id".into(), json!(id));
            if let Some(name) = head.get("name") {
                out.insert("name".into(), name.clone());
            }
            if let Some(b) = birth(head.get("date_of_birth")) {
                out.insert("date_of_birth".into(), b);
            }
            if let Some(c) = head.get("is_corporate_officer") {
                out.insert("is_corporate_officer".into(), c.clone());
            }
            out.insert("total_appointments".into(), json!(total));
            out.insert("appointments_read".into(), json!(appointments.len()));
            if (appointments.len() as u64) < total {
                out.insert(
                    "note".into(),
                    json!(format!(
                        "only the first {} of {total} appointments were read; use get_officer_appointments with start_index {} for the rest",
                        appointments.len(),
                        appointments.len()
                    )),
                );
            }
            out.insert("companies".into(), json!(seen.len()));
            out.insert("company_statuses".into(), json!(statuses));
            out.insert("current_count".into(), json!(current.len()));
            out.insert("past_count".into(), json!(past.len()));
            out.insert("current".into(), Value::Array(current));
            out.insert("past".into(), Value::Array(past));
            Ok(json_result(&Value::Object(out)))
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
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "search_officers");
        run("search_officers", async {
            let c = self.caller(&parts)?;
            let query = required_text("query", &p.query)?;
            let limit = limit_arg(p.limit)?;
            let out = c
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
        description = "List a company's persons with significant control (people, companies or legal persons owning or controlling it, and beneficial owners of overseas entities): kind, natures of control (for example ownership-of-shares-75-to-100-percent), notified and ceased dates, nationality, month and year of birth, address. The first page also includes statements, such as that the company knows of no person with significant control. When there are neither, note says whether the company is exempt.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn list_persons_with_significant_control(
        &self,
        Parameters(p): Parameters<ListPscParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "list_persons_with_significant_control");
        run("list_persons_with_significant_control", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let (start, q) = paging(p.limit, p.start_index)?;
            let mut out = c.psc_page(&number, &q, start).await?;
            if nothing_listed(&out, start) {
                c.ensure_company(&number).await?;
                // Explain the absence, unless the statements could not be read.
                if out.get("statements").is_some()
                    && let Value::Object(map) = &mut out
                {
                    map.insert("note".into(), json!(c.psc_absence_note(&number).await));
                }
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
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "search_disqualified_officers");
        run("search_disqualified_officers", async {
            let c = self.caller(&parts)?;
            let query = required_text("query", &p.query)?;
            let limit = limit_arg(p.limit)?;
            let out = c
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
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "get_disqualified_officer");
        run("get_disqualified_officer", async {
            let c = self.caller(&parts)?;
            let id = id_arg("officer_id", &p.officer_id)?;
            let kinds = match p.officer_kind {
                Some(k) => vec![k],
                None => vec![DisqualifiedKind::Natural, DisqualifiedKind::Corporate],
            };
            for kind in kinds {
                if let Some(v) = c
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
    fn names_compare_without_titles_order_or_case() {
        assert_eq!(
            name_key("SMITH, Jane Anne"),
            name_key("Mrs Jane Anne Smith")
        );
        assert_eq!(name_key("Dr. Jane  SMITH"), name_key("jane smith"));
        assert_ne!(name_key("Jane Smith"), name_key("Jane Anne Smith"));
    }

    #[test]
    fn appointments_are_current_until_resigned_or_closed() {
        assert!(is_current(&json!({"company_status": "active"})));
        assert!(is_current(&json!({"company_status": "liquidation"})));
        assert!(is_current(&json!({})));
        assert!(!is_current(
            &json!({"company_status": "active", "resigned_on": "2020-01-01"})
        ));
        assert!(!is_current(&json!({"company_status": "dissolved"})));
    }

    #[test]
    fn candidates_are_described_on_one_line() {
        assert_eq!(
            candidate(
                &json!({"officer_id": "abc", "name": "Jane SMITH", "date_of_birth": "1980-04",
                              "appointment_count": 3, "address": "1 Park Row, Leeds"})
            ),
            "abc (Jane SMITH, born 1980-04, 3 appointments, 1 Park Row, Leeds)"
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
