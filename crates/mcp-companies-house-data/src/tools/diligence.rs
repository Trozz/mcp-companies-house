//! `due_diligence_check`: observations from the public register for a reviewer to examine. It
//! never concludes that a company is sound: it says what it found, what it checked and what it
//! could not check.

use super::{Caller, CompaniesServer, json_result, normalise_company_number, run};
use crate::error;
use axum::http::request::Parts;
use chrono::{Datelike, Months, NaiveDate};
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde_json::{Map, Value, json};

/// Officers, PSCs and charges read for the check: the first page of this size.
const CHECK_PAGE: u32 = 100;

pub const DISCLAIMER: &str = "These observations come from the public Companies House register, which records what companies file but does not verify it. This is not a verification, credit check, sanctions or politically exposed person screening, or a clearance decision, and having few or no observations does not mean the company is sound.";

/// Checks this tool cannot run from the register, and why.
const NOT_RUN: &[(&str, &str)] = &[
    (
        "sanctions",
        "the register says nothing about sanctions; screen the company, its officers and owners against the UK sanctions list separately",
    ),
    (
        "politically_exposed_persons",
        "the register does not record politically exposed persons",
    ),
    (
        "credit_and_finances",
        "the register holds filed accounts but no credit data; read the accounts with download_filing_document or ask a credit reference agency",
    ),
    (
        "director_disqualifications",
        "disqualifications are not linked to officer ids; search each officer's name with search_disqualified_officers",
    ),
    (
        "litigation_trading_and_other_ownership",
        "court cases, trading performance and ownership outside the PSC rules are not on the register",
    ),
];

/// PSC statements that say the company has not identified, or not confirmed, who controls it.
const UNRESOLVED_STATEMENTS: &[&str] = &[
    "not-identified",
    "not-confirmed",
    "not-yet-completed",
    "failed-to",
    "restrictions-notice",
    "no-response",
    "unidentified",
    "not-provided",
    "awaiting-confirmation",
];

/// Company types the Companies Act's director rules apply to, including that at least one
/// director is a person.
const UK_COMPANY_TYPES: &[&str] = &[
    "ltd",
    "plc",
    "private-unlimited",
    "private-unlimited-nsc",
    "private-limited-guarant-nsc",
    "private-limited-guarant-nsc-limited-exemption",
    "private-limited-shares-section-30-exemption",
    "old-public-company",
];

/// Statuses of a company, entity or branch that is still live.
const LIVE_STATUSES: &[&str] = &["active", "registered", "open"];

/// Whether an officer role is one of the roles that run a company.
type RoleTest = fn(&str) -> bool;

/// The officers who run a company of `company_type` and how to name them; `None` for a type
/// outside the known ones, where the check only notes what it finds.
fn officers_in_charge(company_type: Option<&str>) -> Option<(&'static str, RoleTest)> {
    match company_type? {
        "llp" => Some(("members", |r| {
            r.starts_with("llp-") || r.starts_with("corporate-llp-")
        })),
        "registered-overseas-entity" => {
            Some(("managing officers", |r| r.contains("managing-officer")))
        }
        "oversea-company" => Some(("directors or equivalent officers", |r| {
            r.contains("director")
                || r.starts_with("member-of-")
                || r.starts_with("person-authorised")
        })),
        "eeig" => Some(("managers", |r| r.contains("manager-of-an-eeig"))),
        t if UK_COMPANY_TYPES.contains(&t) => Some(("directors", |r| r.contains("director"))),
        _ => None,
    }
}

/// Company types whose officers the register does not list: a limited partnership's partners
/// and a UK establishment's officers (who are its overseas parent's) have no officer records.
const NO_OFFICER_RECORDS: &[&str] = &["limited-partnership", "uk-establishment"];

/// Whether the PSC rules cover a company of this type and number, so that a missing PSC record
/// is worth a flag: UK companies, LLPs, SEs and UK societas, EEIGs, Scottish partnerships and
/// Scottish limited partnerships (numbered SL), and overseas entities (their beneficial owners).
fn reports_pscs(company_type: Option<&str>, number: &str) -> bool {
    match company_type {
        Some(t) if UK_COMPANY_TYPES.contains(&t) => true,
        Some(
            "llp"
            | "european-public-limited-liability-company-se"
            | "united-kingdom-societas"
            | "eeig"
            | "scottish-partnership"
            | "registered-overseas-entity",
        ) => true,
        Some("limited-partnership") => number.starts_with("SL"),
        _ => false,
    }
}

/// What a check found. `flag`: something a reviewer should look into; `note`: context.
struct Findings {
    observations: Vec<Value>,
    checks_run: Vec<&'static str>,
    not_run: Vec<Value>,
}

impl Findings {
    fn new() -> Self {
        Findings {
            observations: Vec::new(),
            checks_run: Vec::new(),
            not_run: Vec::new(),
        }
    }

    fn flag(&mut self, check: &str, detail: impl Into<String>) {
        self.observe(check, "flag", detail.into());
    }

    fn note(&mut self, check: &str, detail: impl Into<String>) {
        self.observe(check, "note", detail.into());
    }

    fn observe(&mut self, check: &str, level: &str, detail: String) {
        self.observations
            .push(json!({"check": check, "level": level, "detail": detail}));
    }

    fn ran(&mut self, check: &'static str) {
        self.checks_run.push(check);
    }

    fn failed(&mut self, check: &str, e: &crate::error::ChError) {
        self.not_run.push(json!({
            "check": check,
            "reason": format!("could not be read: {}: {e}", e.kind()),
        }));
    }
}

fn text<'a>(v: &'a Value, path: &str) -> Option<&'a str> {
    crate::view::at(v, path).and_then(Value::as_str)
}

fn flag_set(v: &Value, path: &str) -> bool {
    crate::view::at(v, path).and_then(Value::as_bool) == Some(true)
}

fn date(v: &Value, path: &str) -> Option<NaiveDate> {
    text(v, path).and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
}

/// How long ago, in whole months: "less than a month", "1 month", "5 months".
fn months_ago(months: i64) -> String {
    match months {
        ..=0 => "less than a month".to_string(),
        1 => "1 month".to_string(),
        n => format!("{n} months"),
    }
}

/// Whole months from `from` to `to`.
fn months_between(from: NaiveDate, to: NaiveDate) -> i64 {
    let mut months =
        i64::from(to.year() - from.year()) * 12 + i64::from(to.month()) - i64::from(from.month());
    if to.day() < from.day() {
        months -= 1;
    }
    months
}

/// Status, filing deadlines, registered office, age and name changes: all from the profile.
fn profile_checks(f: &mut Findings, profile: &Value, today: NaiveDate) {
    f.ran("company_status");
    let status = text(profile, "company_status").unwrap_or("unknown");
    if !LIVE_STATUSES.contains(&status) {
        f.flag(
            "company_status",
            format!("the company's status is {status}"),
        );
    }
    if let Some(detail) = text(profile, "company_status_detail") {
        f.flag("company_status", format!("status detail: {detail}"));
    }

    f.ran("accounts");
    if flag_set(profile, "accounts/overdue") || flag_set(profile, "accounts/next_accounts/overdue")
    {
        let due = text(profile, "accounts/next_accounts/due_on")
            .or_else(|| text(profile, "accounts/next_due"))
            .unwrap_or("an earlier date");
        f.flag(
            "accounts",
            format!("accounts are overdue: they were due by {due}"),
        );
    }
    if text(profile, "accounts/last_accounts/type") == Some("dormant") {
        let made_up = text(profile, "accounts/last_accounts/made_up_to").unwrap_or("?");
        f.note(
            "accounts",
            format!("the latest accounts (made up to {made_up}) were filed as dormant"),
        );
    }

    f.ran("confirmation_statement");
    if flag_set(profile, "confirmation_statement/overdue") {
        let due = text(profile, "confirmation_statement/next_due").unwrap_or("an earlier date");
        f.flag(
            "confirmation_statement",
            format!("the confirmation statement is overdue: it was due by {due}"),
        );
    }

    f.ran("registered_office");
    if flag_set(profile, "registered_office_is_in_dispute") {
        f.flag(
            "registered_office",
            "the registered office address is in dispute",
        );
    }
    if flag_set(profile, "undeliverable_registered_office_address") {
        f.flag(
            "registered_office",
            "Companies House has marked the registered office address as undeliverable",
        );
    }

    f.ran("company_age");
    if let Some(created) = date(profile, "date_of_creation") {
        let months = months_between(created, today);
        let detail = format!(
            "the company was incorporated {} ago, on {created}",
            months_ago(months)
        );
        if months < 12 {
            f.flag("company_age", detail);
        } else if months < 24 {
            f.note("company_age", detail);
        }
    }

    f.ran("name_changes");
    let year_ago = today.checked_sub_months(Months::new(12)).unwrap_or(today);
    let recent: Vec<String> = profile
        .get("previous_company_names")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter(|n| date(n, "ceased_on").is_some_and(|d| d >= year_ago))
                .filter_map(|n| {
                    Some(format!(
                        "{} until {}",
                        text(n, "name")?,
                        text(n, "ceased_on")?
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    if !recent.is_empty() {
        f.note(
            "name_changes",
            format!(
                "the company changed its name in the last 12 months: {}",
                recent.join("; ")
            ),
        );
    }
}

/// Insolvency cases, each with its dates.
fn insolvency_checks(f: &mut Findings, record: Option<Value>) {
    f.ran("insolvency");
    let cases = record
        .as_ref()
        .and_then(|r| r.get("cases"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for case in &cases {
        let kind = text(case, "type").unwrap_or("insolvency case");
        let dates: Vec<String> = case
            .get("dates")
            .and_then(Value::as_object)
            .map(|d| {
                d.iter()
                    .filter_map(|(k, v)| Some(format!("{k} {}", v.as_str()?)))
                    .collect()
            })
            .unwrap_or_default();
        let detail = if dates.is_empty() {
            format!("insolvency case: {kind}")
        } else {
            format!("insolvency case: {kind} ({})", dates.join(", "))
        };
        f.flag("insolvency", detail);
    }
    if cases.is_empty()
        && let Some(status) = record.as_ref().and_then(|r| text(r, "status"))
    {
        f.flag("insolvency", format!("insolvency status: {status}"));
    }
}

/// Charges not yet satisfied: common for trading companies, so a note, with the lenders.
fn charges_checks(f: &mut Findings, page: Option<Value>) {
    f.ran("charges");
    let Some(page) = page else { return };
    let listed: Vec<&Value> = page
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
                .collect()
        })
        .unwrap_or_default();
    // The totals cover every charge, the page only the first 100.
    let num = |k: &str| page.get(k).and_then(Value::as_u64);
    let open = match (num("total"), num("satisfied_count")) {
        (Some(total), Some(satisfied)) => total.saturating_sub(satisfied) as usize,
        _ => listed.len(),
    };
    if open == 0 {
        return;
    }
    let mut lenders: Vec<&str> = listed
        .iter()
        .filter_map(|c| c.get("persons_entitled").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    lenders.sort_unstable();
    lenders.dedup();
    let held_by = if lenders.is_empty() {
        String::new()
    } else if page.get("next_start_index").is_some() {
        format!(
            ", held by {} among the first 100 charges",
            lenders.join(", ")
        )
    } else {
        format!(", held by {}", lenders.join(", "))
    };
    f.note(
        "charges",
        format!("charges not satisfied: {open}{held_by}; see list_charges"),
    );
}

/// Directors (or the type's equivalent) in post, and how many officers joined or left in the
/// last year.
fn officer_checks(
    f: &mut Findings,
    page: &Value,
    company_type: Option<&str>,
    psc: Option<&Value>,
    today: NaiveDate,
) {
    f.ran("officers");
    if company_type.is_some_and(|t| NO_OFFICER_RECORDS.contains(&t)) {
        return;
    }
    let items = page
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let current: Vec<&Value> = items
        .iter()
        .filter(|o| o.get("resigned_on").is_none())
        .collect();
    let role = |o: &Value| {
        o.get("officer_role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let truncated = page.get("next_start_index").is_some();
    match officers_in_charge(company_type) {
        Some((what, in_charge)) => {
            let running: Vec<&&Value> = current.iter().filter(|o| in_charge(&role(o))).collect();
            // An overseas entity needs managing officers on the register only when it has not
            // identified all its beneficial owners, or given their information.
            let unresolved_owners = psc
                .and_then(|p| p.get("statements"))
                .and_then(Value::as_array)
                .is_some_and(|st| {
                    st.iter().any(|s| {
                        s.get("ceased_on").is_none()
                            && text(s, "statement").is_some_and(|c| {
                                c.contains("unidentified") || c.contains("not-provided")
                            })
                    })
                });
            let required = company_type != Some("registered-overseas-entity") || unresolved_owners;
            if running.is_empty() && !required {
                // Nothing to look into: managing officers are optional here.
            } else if running.is_empty() && truncated {
                f.note(
                    "officers",
                    format!("no current {what} are among the first 100 officer records; see list_officers with current_only"),
                );
            } else if running.is_empty() && company_type == Some("registered-overseas-entity") {
                f.flag(
                    "officers",
                    "no current managing officers are recorded, though not every beneficial owner is identified",
                );
            } else if running.is_empty() {
                f.flag("officers", format!("no current {what} are recorded"));
            } else if company_type.is_some_and(|t| UK_COMPANY_TYPES.contains(&t))
                && running.iter().all(|o| role(o).starts_with("corporate-"))
            {
                f.flag(
                    "officers",
                    "every current director is a company; a UK company must have at least one director who is a person",
                );
            }
        }
        None => {
            if current.is_empty() && !truncated {
                f.note(
                    "officers",
                    format!(
                        "no current officers are recorded; whether a {} must have any was not checked",
                        company_type.unwrap_or("company of this type")
                    ),
                );
            }
        }
    }
    let year_ago = today.checked_sub_months(Months::new(12)).unwrap_or(today);
    let within = |o: &Value, key: &str| date(o, key).is_some_and(|d| d >= year_ago);
    let appointed = items.iter().filter(|o| within(o, "appointed_on")).count();
    let resigned = items.iter().filter(|o| within(o, "resigned_on")).count();
    if appointed + resigned > 0 {
        f.note(
            "officers",
            format!(
                "officer changes in the last 12 months: {appointed} appointed, {resigned} resigned"
            ),
        );
    }
    if truncated {
        f.note(
            "officers",
            "the company has more than 100 officer records; only the first 100 were checked",
        );
    }
}

/// Where someone's identity verification statement stands, from `identity_verification`.
#[derive(Debug, PartialEq, Eq)]
enum Verification {
    /// A statement is on record for this role.
    OnRecord,
    /// None on record and the due date has passed.
    Overdue(String),
    /// None on record (yet).
    NotOnRecord,
}

fn verification(person: &Value, today: NaiveDate) -> Verification {
    let iv = person.get("identity_verification").unwrap_or(&Value::Null);
    let start = date(iv, "appointment_verification_start_on");
    let end = date(iv, "appointment_verification_end_on");
    let due = date(iv, "appointment_verification_statement_due_on");
    if start.is_some_and(|s| s <= today) && end.is_none_or(|e| e > today) {
        return Verification::OnRecord;
    }
    match due {
        Some(d) if d < today => Verification::Overdue(d.to_string()),
        _ => Verification::NotOnRecord,
    }
}

/// Identity verification statements of the people in post: directors (or the type's
/// equivalent) who are people, and individual PSCs. Companies House is phasing verification in,
/// so "none on record" is a note; a statement past its due date is a flag.
fn identity_checks(
    f: &mut Findings,
    officers: Option<&Value>,
    pscs: Option<&Value>,
    company_type: Option<&str>,
    today: NaiveDate,
) {
    f.ran("identity_verification");
    for (half, page, what) in [
        ("identity_verification_officers", officers, "officers"),
        (
            "identity_verification_pscs",
            pscs,
            "persons with significant control",
        ),
    ] {
        if page.is_none() {
            f.not_run.push(json!({
                "check": half,
                "reason": format!("the {what} could not be read"),
            }));
        }
    }
    let current = |page: Option<&Value>, ended: &str| -> Vec<Value> {
        page.and_then(|p| p.get("items"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|i| i.get(ended).is_none())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    let in_charge = officers_in_charge(company_type).map(|(_, f)| f);
    let people: Vec<Value> = current(officers, "resigned_on")
        .into_iter()
        .filter(|o| {
            let role = o.get("officer_role").and_then(Value::as_str).unwrap_or("");
            !role.starts_with("corporate-") && in_charge.is_some_and(|f| f(role))
        })
        .collect();
    let owners: Vec<Value> = current(pscs, "ceased_on")
        .into_iter()
        .filter(|p| text(p, "kind") == Some("individual"))
        .collect();
    let mut overdue: Vec<String> = Vec::new();
    let mut tally = |group: &[Value]| {
        let mut on_record = 0;
        for person in group {
            match verification(person, today) {
                Verification::OnRecord => on_record += 1,
                Verification::Overdue(due) => overdue.push(format!(
                    "{} (due {due})",
                    text(person, "name").unwrap_or("unnamed")
                )),
                Verification::NotOnRecord => {}
            }
        }
        on_record
    };
    let people_verified = tally(&people);
    let owners_verified = tally(&owners);
    if !overdue.is_empty() {
        f.flag(
            "identity_verification",
            format!(
                "identity verification statements are overdue for: {}",
                overdue.join(", ")
            ),
        );
    }
    let mut tallies: Vec<String> = Vec::new();
    if officers.is_some() && !people.is_empty() {
        tallies.push(format!(
            "{people_verified} of {} officers in charge who are people",
            people.len()
        ));
    }
    if pscs.is_some() && !owners.is_empty() {
        tallies.push(format!(
            "{owners_verified} of {} individual PSCs",
            owners.len()
        ));
    }
    if !tallies.is_empty() {
        f.note(
            "identity_verification",
            format!(
                "identity verification statements on record: {}. Companies House is phasing verification in, so none on record is not a failure; the register shows that a statement was filed, not the check itself",
                tallies.join(", ")
            ),
        );
    }
}

/// Who controls the company: PSCs, PSC statements, and, when there are neither, exemptions.
async fn ownership_checks(
    f: &mut Findings,
    c: &Caller,
    number: &str,
    company_type: Option<&str>,
    page: &Value,
) {
    f.ran("ownership");
    if page.get("statements").is_none() {
        let why = page
            .get("statements_unavailable")
            .and_then(Value::as_str)
            .unwrap_or("not read");
        f.not_run.push(json!({
            "check": "ownership_statements",
            "reason": format!("the PSC statements could not be read: {why}"),
        }));
    }
    let current = |key: &str| -> Vec<Value> {
        page.get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|i| i.get("ceased_on").is_none())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    let pscs = current("items");
    let statements = current("statements");
    let statements_read = page.get("statements").is_some();
    for st in &statements {
        let code = text(st, "statement").unwrap_or("unknown");
        if UNRESOLVED_STATEMENTS.iter().any(|u| code.contains(u)) {
            f.flag("ownership", format!("PSC statement: {code}"));
        } else {
            f.note("ownership", format!("PSC statement: {code}"));
        }
    }
    let corporate: Vec<&str> = pscs
        .iter()
        .filter(|p| {
            text(p, "kind").is_some_and(|k| k.contains("corporate") || k.contains("legal-person"))
        })
        .filter_map(|p| text(p, "name"))
        .collect();
    if !corporate.is_empty() {
        f.note(
            "ownership",
            format!(
                "control passes through other entities: {}; check who controls them",
                corporate.join(", ")
            ),
        );
    }
    let sanctioned: Vec<&str> = pscs
        .iter()
        .filter(|p| p.get("is_sanctioned") == Some(&Value::Bool(true)))
        .map(|p| text(p, "name").unwrap_or("a person with significant control"))
        .collect();
    if !sanctioned.is_empty() {
        f.flag(
            "ownership",
            format!(
                "the register marks these owners as sanctioned: {}",
                sanctioned.join(", ")
            ),
        );
    }
    if pscs
        .iter()
        .any(|p| text(p, "kind").is_some_and(|k| k.contains("super-secure")))
    {
        f.note(
            "ownership",
            "the details of a person with significant control are protected from the public register",
        );
    }
    if pscs.is_empty() && statements.is_empty() && page.get("next_start_index").is_none() {
        if !statements_read {
            return;
        }
        if !reports_pscs(company_type, number) {
            f.note(
                "ownership",
                format!(
                    "the register shows no owners; a {} is not one of the kinds of company this check expects to report persons with significant control",
                    company_type.unwrap_or("company of unknown type")
                ),
            );
            return;
        }
        let ceased = page
            .get("items")
            .and_then(Value::as_array)
            .is_some_and(|i| !i.is_empty());
        let note = c.psc_absence_note(number, ceased).await;
        if note.contains("exempt from the PSC rules") {
            f.note("ownership", note);
        } else {
            f.flag("ownership", note);
        }
    }
}

/// The company members a reviewer needs at the top.
fn summary(profile: &Value) -> Value {
    let mut out = Map::new();
    for key in [
        "company_number",
        "company_name",
        "company_status",
        "company_status_detail",
        "type",
        "date_of_creation",
        "date_of_cessation",
        "jurisdiction",
        "registered_office_address",
        "nature_of_business",
    ] {
        if let Some(v) = profile.get(key) {
            out.insert(key.to_string(), v.clone());
        }
    }
    Value::Object(out)
}

fn paging(size: u32) -> Vec<(&'static str, String)> {
    vec![
        ("items_per_page", size.to_string()),
        ("start_index", "0".to_string()),
    ]
}

impl Caller {
    pub(crate) async fn due_diligence(
        &self,
        number: &str,
        today: NaiveDate,
    ) -> error::Result<Value> {
        let profile = self.profile(number).await?;
        let records: Vec<&str> = profile
            .get("records_available")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let has_charges = records.contains(&"charges");
        let has_insolvency = records.contains(&"insolvency");
        let page = paging(CHECK_PAGE);
        let (officers, psc, charges, insolvency) = tokio::join!(
            self.officers_page(number, &page, 0),
            self.psc_page(number, &page, 0),
            async {
                if !has_charges {
                    return Ok(None);
                }
                self.charges_page(number, &page, 0).await.map(Some)
            },
            async {
                if !has_insolvency {
                    return Ok(None);
                }
                self.insolvency_record(number).await
            },
        );

        let mut f = Findings::new();
        profile_checks(&mut f, &profile, today);
        match insolvency {
            Ok(record) => insolvency_checks(&mut f, record),
            Err(e) => f.failed("insolvency", &e),
        }
        match charges {
            Ok(page) => charges_checks(&mut f, page),
            Err(e) => f.failed("charges", &e),
        }
        let company_type = profile.get("type").and_then(Value::as_str);
        match &officers {
            Ok(page) => officer_checks(&mut f, page, company_type, psc.as_ref().ok(), today),
            Err(e) => f.failed("officers", e),
        }
        match &psc {
            Ok(page) => ownership_checks(&mut f, self, number, company_type, page).await,
            Err(e) => f.failed("ownership", e),
        }
        match (&officers, &psc) {
            (Err(e), Err(_)) => f.failed("identity_verification", e),
            _ => identity_checks(
                &mut f,
                officers.as_ref().ok(),
                psc.as_ref().ok(),
                company_type,
                today,
            ),
        }
        let mut not_run = f.not_run;
        not_run.extend(
            NOT_RUN
                .iter()
                .map(|(check, reason)| json!({"check": check, "reason": reason})),
        );
        Ok(json!({
            "company": summary(&profile),
            "checked_on": today.to_string(),
            "observations": f.observations,
            "checks_run": f.checks_run,
            "checks_not_run": not_run,
            "disclaimer": DISCLAIMER,
        }))
    }
}

#[tool_router(router = diligence_router, vis = "pub(crate)")]
impl CompaniesServer {
    #[tool(
        description = "Screen one company against the public register and return observations for a reviewer to examine: status and status detail, insolvency cases, charges not satisfied, overdue accounts and confirmation statements, registered office disputes or undeliverable addresses, directors in post and recent officer changes, ownership (PSCs, PSC statements and exemptions), company age and recent name changes. Each observation is a flag (look into it) or a note (context). Also tallies the identity verification statements of the directors and individual PSCs in post, flagging any overdue, and lists the checks run and those it cannot run (sanctions, politically exposed persons, credit and others). It never concludes that a company is sound. Uses up to 7 Companies House requests.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn due_diligence_check(
        &self,
        Parameters(p): Parameters<super::CompanyNumberParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "due_diligence_check");
        run("due_diligence_check", async {
            let c = self.caller(&parts)?;
            let number = normalise_company_number(&p.company_number)?;
            let today = chrono::Utc::now().date_naive();
            Ok(json_result(&c.due_diligence(&number, today).await?))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn levels(f: &Findings) -> Vec<(String, String)> {
        f.observations
            .iter()
            .map(|o| {
                (
                    o["check"].as_str().unwrap().to_string(),
                    o["level"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn ages_read_naturally() {
        assert_eq!(months_ago(0), "less than a month");
        assert_eq!(months_ago(1), "1 month");
        assert_eq!(months_ago(7), "7 months");
    }

    #[test]
    fn months_are_whole() {
        assert_eq!(months_between(day("2025-10-18"), day("2026-10-18")), 12);
        assert_eq!(months_between(day("2025-10-19"), day("2026-10-18")), 11);
        assert_eq!(months_between(day("2026-10-01"), day("2026-10-18")), 0);
    }

    #[test]
    fn a_healthy_profile_raises_nothing() {
        let mut f = Findings::new();
        let profile = json!({
            "company_status": "active", "date_of_creation": "2015-03-01",
            "accounts": {"overdue": false, "next_accounts": {"overdue": false}},
            "confirmation_statement": {"overdue": false},
            "registered_office_is_in_dispute": false,
        });
        profile_checks(&mut f, &profile, day("2026-10-18"));
        assert!(f.observations.is_empty(), "{:?}", f.observations);
        assert_eq!(
            f.checks_run,
            vec![
                "company_status",
                "accounts",
                "confirmation_statement",
                "registered_office",
                "company_age",
                "name_changes"
            ]
        );
    }

    #[test]
    fn profile_problems_are_flagged() {
        let mut f = Findings::new();
        let profile = json!({
            "company_status": "active", "company_status_detail": "active-proposal-to-strike-off",
            "date_of_creation": "2026-04-01",
            "accounts": {"next_accounts": {"overdue": true, "due_on": "2026-09-30"},
                         "last_accounts": {"type": "dormant", "made_up_to": "2025-12-31"}},
            "confirmation_statement": {"overdue": true, "next_due": "2026-08-01"},
            "registered_office_is_in_dispute": true,
            "undeliverable_registered_office_address": true,
            "previous_company_names": [{"name": "OLD NAME LTD", "ceased_on": "2026-06-01"},
                                       {"name": "OLDER NAME LTD", "ceased_on": "2019-06-01"}],
        });
        profile_checks(&mut f, &profile, day("2026-10-18"));
        let found = levels(&f);
        let expect = [
            ("company_status", "flag"),
            ("accounts", "flag"),
            ("accounts", "note"),
            ("confirmation_statement", "flag"),
            ("registered_office", "flag"),
            ("registered_office", "flag"),
            ("company_age", "flag"),
            ("name_changes", "note"),
        ];
        assert_eq!(
            found,
            expect
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect::<Vec<_>>()
        );
        let text = serde_json::to_string(&f.observations).unwrap();
        assert!(text.contains("due by 2026-09-30"), "{text}");
        assert!(text.contains("OLD NAME LTD until 2026-06-01"), "{text}");
        assert!(!text.contains("OLDER NAME LTD"), "{text}");
    }

    #[test]
    fn officers_without_a_person_director_are_flagged() {
        let mut f = Findings::new();
        let page = json!({"items": [
            {"name": "A LTD", "officer_role": "corporate-director", "appointed_on": "2026-09-01"},
            {"name": "B", "officer_role": "director", "appointed_on": "2010-01-01", "resigned_on": "2026-08-01"},
            {"name": "C", "officer_role": "secretary", "appointed_on": "2010-01-01"},
        ]});
        officer_checks(&mut f, &page, Some("ltd"), None, day("2026-10-18"));
        let text = serde_json::to_string(&f.observations).unwrap();
        assert!(
            text.contains("every current director is a company"),
            "{text}"
        );
        assert!(
            text.contains("officer changes in the last 12 months: 1 appointed, 1 resigned"),
            "{text}"
        );

        let mut f = Findings::new();
        officer_checks(
            &mut f,
            &json!({"items": [{"name": "C", "officer_role": "secretary"}]}),
            Some("ltd"),
            None,
            day("2026-10-18"),
        );
        assert_eq!(levels(&f), vec![("officers".into(), "flag".into())]);

        let mut f = Findings::new();
        officer_checks(
            &mut f,
            &json!({"items": [{"name": "D", "officer_role": "llp-designated-member"}]}),
            Some("llp"),
            None,
            day("2026-10-18"),
        );
        assert!(f.observations.is_empty(), "{:?}", f.observations);
    }

    #[test]
    fn a_long_officer_list_without_a_director_on_its_first_page_is_only_noted() {
        let mut f = Findings::new();
        let page = json!({"next_start_index": 100, "items": [
            {"name": "OLD", "officer_role": "director", "resigned_on": "2010-01-01"}
        ]});
        officer_checks(&mut f, &page, Some("ltd"), None, day("2026-10-18"));
        assert!(
            f.observations.iter().all(|o| o["level"] == "note"),
            "{:?}",
            f.observations
        );
    }

    #[test]
    fn officers_are_judged_by_the_company_type() {
        let mut f = Findings::new();
        let page = json!({"items": [{"name": "M", "officer_role": "managing-officer"}]});
        officer_checks(
            &mut f,
            &page,
            Some("registered-overseas-entity"),
            None,
            day("2026-10-18"),
        );
        assert!(f.observations.is_empty(), "{:?}", f.observations);

        let mut f = Findings::new();
        let page = json!({"items": [{"name": "C LTD", "officer_role": "corporate-director"}]});
        officer_checks(
            &mut f,
            &page,
            Some("oversea-company"),
            None,
            day("2026-10-18"),
        );
        assert!(
            f.observations.is_empty(),
            "the person-director rule is for UK companies: {:?}",
            f.observations
        );

        let mut f = Findings::new();
        officer_checks(
            &mut f,
            &json!({"items": []}),
            Some("royal-charter"),
            None,
            day("2026-10-18"),
        );
        assert_eq!(levels(&f), vec![("officers".into(), "note".into())]);
    }

    #[test]
    fn live_statuses_of_entities_and_branches_are_not_flagged() {
        for status in ["active", "registered", "open"] {
            let mut f = Findings::new();
            profile_checks(
                &mut f,
                &json!({"company_status": status, "date_of_creation": "2015-03-01"}),
                day("2026-10-18"),
            );
            assert!(f.observations.is_empty(), "{status}: {:?}", f.observations);
        }
    }

    #[test]
    fn open_charges_count_every_charge_not_just_the_first_page() {
        let mut f = Findings::new();
        charges_checks(
            &mut f,
            Some(
                json!({"total": 150, "satisfied_count": 100, "next_start_index": 100,
                        "items": [{"status": "fully-satisfied", "persons_entitled": ["Old Bank"]}]}),
            ),
        );
        assert_eq!(
            f.observations,
            vec![json!({"check": "charges", "level": "note",
                        "detail": "charges not satisfied: 50; see list_charges"})]
        );
    }

    #[test]
    fn verification_reads_the_statement_dates() {
        let today = day("2026-10-18");
        let with = |iv: Value| json!({"identity_verification": iv});
        assert_eq!(
            verification(
                &with(json!({"appointment_verification_start_on": "2025-11-18"})),
                today
            ),
            Verification::OnRecord
        );
        assert_eq!(
            verification(
                &with(json!({"appointment_verification_start_on": "2025-11-18",
                             "appointment_verification_end_on": "9999-12-31"})),
                today
            ),
            Verification::OnRecord
        );
        assert_eq!(
            verification(
                &with(json!({"appointment_verification_statement_due_on": "2026-01-01"})),
                today
            ),
            Verification::Overdue("2026-01-01".into())
        );
        assert_eq!(
            verification(
                &with(json!({"appointment_verification_statement_due_on": "2027-01-01"})),
                today
            ),
            Verification::NotOnRecord
        );
        assert_eq!(verification(&json!({}), today), Verification::NotOnRecord);
    }

    #[test]
    fn identity_checks_count_people_in_charge_and_flag_overdue_statements() {
        let mut f = Findings::new();
        let officers = json!({"items": [
            {"name": "A", "officer_role": "director",
             "identity_verification": {"appointment_verification_start_on": "2025-11-18"}},
            {"name": "B", "officer_role": "director",
             "identity_verification": {"appointment_verification_statement_due_on": "2026-02-01"}},
            {"name": "C LTD", "officer_role": "corporate-director"},
            {"name": "D", "officer_role": "secretary"},
            {"name": "E", "officer_role": "director", "resigned_on": "2020-01-01"},
        ]});
        let pscs = json!({"items": [
            {"name": "P", "kind": "individual"},
            {"name": "Q LTD", "kind": "corporate-entity"},
        ]});
        identity_checks(
            &mut f,
            Some(&officers),
            Some(&pscs),
            Some("ltd"),
            day("2026-10-18"),
        );
        assert_eq!(
            levels(&f),
            vec![
                ("identity_verification".into(), "flag".into()),
                ("identity_verification".into(), "note".into())
            ]
        );
        let text = serde_json::to_string(&f.observations).unwrap();
        assert!(text.contains("overdue for: B (due 2026-02-01)"), "{text}");
        assert!(
            text.contains("1 of 2 officers in charge who are people, 0 of 1 individual PSCs"),
            "{text}"
        );
    }

    #[test]
    fn open_charges_are_noted_with_their_lenders() {
        let mut f = Findings::new();
        charges_checks(
            &mut f,
            Some(json!({"items": [
                {"status": "outstanding", "persons_entitled": ["Big Bank PLC"]},
                {"status": "part-satisfied", "persons_entitled": ["Big Bank PLC", "Other Bank"]},
                {"status": "fully-satisfied", "persons_entitled": ["Old Bank"]},
            ]})),
        );
        assert_eq!(
            f.observations,
            vec![json!({"check": "charges", "level": "note",
                        "detail": "charges not satisfied: 2, held by Big Bank PLC, Other Bank; see list_charges"})]
        );
    }

    #[test]
    fn insolvency_cases_are_flagged_with_their_dates() {
        let mut f = Findings::new();
        insolvency_checks(
            &mut f,
            Some(json!({"status": "liquidation", "cases": [
                {"type": "creditors-voluntary-liquidation", "dates": {"wound-up-on": "2025-02-01"}}
            ]})),
        );
        assert_eq!(
            f.observations,
            vec![json!({"check": "insolvency", "level": "flag",
                        "detail": "insolvency case: creditors-voluntary-liquidation (wound-up-on 2025-02-01)"})]
        );
        let mut f = Findings::new();
        insolvency_checks(&mut f, None);
        assert!(f.observations.is_empty());
    }
}
