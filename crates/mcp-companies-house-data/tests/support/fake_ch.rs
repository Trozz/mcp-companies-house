//! In-memory Companies House REST API shaped like the real one: Basic auth with the API key as
//! the username, rate-limit headers on every response, `/search/companies`,
//! `/advanced-search/companies` (404 with `{}` when nothing matches, like the real service) and
//! `/company/{number}`. Every request is logged for assertions; failures can be injected.
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

pub const API_KEY: &str = "test-api-key";

/// The `Authorization` header Companies House expects: Basic with the key as the username and a
/// blank password.
pub fn auth_header() -> String {
    use base64::Engine as _;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{API_KEY}:"))
    )
}
pub const RATE_LIMIT: u64 = 600;

#[derive(Debug, Clone)]
pub struct Logged {
    pub path: String,
    pub query: Vec<(String, String)>,
    pub authorization: Option<String>,
    pub accept: Option<String>,
}

impl Logged {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A canned response used instead of the normal one.
#[derive(Debug, Clone)]
pub struct Injected {
    pub status: u16,
    pub body: String,
    pub headers: Vec<(&'static str, String)>,
}

impl Injected {
    pub fn status(status: u16) -> Self {
        Injected {
            status,
            body: String::new(),
            headers: Vec::new(),
        }
    }

    pub fn json(status: u16, body: Value) -> Self {
        Injected {
            status,
            body: body.to_string(),
            headers: Vec::new(),
        }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

/// A filed document on the fake Document API: its formats (content type and bytes).
#[derive(Debug, Clone)]
pub struct FakeDocument {
    pub formats: Vec<(String, Vec<u8>)>,
    /// Whether the metadata gives each format's size, as the live API does.
    pub sized: bool,
}

#[derive(Debug)]
pub struct FakeState {
    /// Company profiles by company number.
    pub companies: BTreeMap<String, Value>,
    /// Other resources by request path (see `fake_records`).
    pub resources: BTreeMap<String, Value>,
    pub log: Vec<Logged>,
    /// Responses to send instead of the normal ones, one per request, in order.
    pub inject: VecDeque<Injected>,
    pub rate_remaining: u64,
    pub rate_reset_unix: u64,
    /// Delay before every response, to exercise timeouts.
    pub delay: Duration,
    /// Filed documents by document id, served under `/docapi/document/{id}` with their content
    /// redirected to `/storage/{id}` (which, like the live storage, takes no API key).
    pub documents: BTreeMap<String, FakeDocument>,
    /// Paths that always answer 500.
    pub fail: Vec<String>,
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn address(premises: &str, line1: &str, locality: &str, postcode: &str) -> Value {
    json!({
        "premises": premises,
        "address_line_1": line1,
        "locality": locality,
        "postal_code": postcode,
        "country": "England",
        "region": "",
        "po_box": ""
    })
}

#[allow(clippy::too_many_arguments)]
fn profile(
    number: &str,
    name: &str,
    status: &str,
    kind: &str,
    subtype: Option<&str>,
    created: &str,
    ceased: Option<&str>,
    sic: &[&str],
    office: Value,
) -> Value {
    let mut p = json!({
        "accounts": {
            "accounting_reference_date": {"day": "31", "month": "03"},
            "next_accounts": {"due_on": "2027-12-31", "overdue": false, "period_end_on": "2027-03-31", "period_start_on": "2026-04-01"},
            "next_due": "2027-12-31", "next_made_up_to": "2027-03-31", "overdue": false
        },
        "can_file": status == "active",
        "company_name": name,
        "company_number": number,
        "company_status": status,
        "confirmation_statement": {"next_due": "2027-06-14", "next_made_up_to": "2027-05-31", "overdue": false},
        "date_of_creation": created,
        "etag": format!("etag-{number}"),
        "has_charges": false,
        "has_insolvency_history": false,
        "jurisdiction": if number.starts_with("SC") { "scotland" } else { "england-wales" },
        "links": {
            "self": format!("/company/{number}"),
            "filing_history": format!("/company/{number}/filing-history"),
            "officers": format!("/company/{number}/officers"),
        },
        "registered_office_address": office,
        "registered_office_is_in_dispute": false,
        "sic_codes": sic,
        "type": kind,
        "undeliverable_registered_office_address": false,
    });
    if let Some(s) = subtype {
        p["subtype"] = json!(s);
    }
    if let Some(c) = ceased {
        p["date_of_cessation"] = json!(c);
    }
    p
}

impl FakeState {
    /// Six companies across statuses, types, a subtype, SIC codes, towns and dates.
    pub fn register() -> Self {
        let mut companies = BTreeMap::new();
        let mut add = |p: Value| {
            companies.insert(p["company_number"].as_str().unwrap().to_string(), p);
        };
        add(profile(
            "00000001",
            "ACME WIDGETS LIMITED",
            "active",
            "ltd",
            None,
            "2015-03-01",
            None,
            &["62012"],
            address("1", "Park Row", "Leeds", "LS1 5AB"),
        ));
        add(profile(
            "00000002",
            "ACME BAKERY LIMITED",
            "active",
            "ltd",
            None,
            "2021-06-15",
            None,
            &["10710", "47240"],
            address("Unit 4", "Kirkstall Road", "Leeds", "LS3 1LH"),
        ));
        let mut thistle = profile(
            "SC000003",
            "THISTLE SOFTWARE LTD",
            "dissolved",
            "ltd",
            None,
            "2010-01-10",
            Some("2020-05-05"),
            &["62020"],
            address("", "10 Buchanan Street", "Glasgow", "G1 3HL"),
        );
        thistle["registered_office_address"]["country"] = json!("Scotland");
        thistle["previous_company_names"] = json!([
            {"name": "THISTLE CODE LTD", "effective_from": "2010-01-10", "ceased_on": "2012-02-02"}
        ]);
        add(thistle);
        add(profile(
            "OC000004",
            "NORTHERN PARTNERS LLP",
            "active",
            "llp",
            None,
            "2018-09-20",
            None,
            &["69201"],
            address("", "1 Spinningfields", "Manchester", "M3 3AP"),
        ));
        add(profile(
            "00000005",
            "GREEN FUTURES CIC",
            "active",
            "ltd",
            Some("community-interest-company"),
            "2022-02-02",
            None,
            &["88990"],
            address("", "2 Briggate", "Leeds", "LS1 6HD"),
        ));
        let mut holdings = profile(
            "00000006",
            "ACME HOLDINGS PLC",
            "liquidation",
            "plc",
            None,
            "1999-11-11",
            None,
            &["64209"],
            address("", "1 Poultry", "London", "EC2R 8EJ"),
        );
        holdings["links"]["insolvency"] = json!("/company/00000006/insolvency");
        holdings["links"]["charges"] = json!("/company/00000006/charges");
        add(holdings);
        FakeState {
            companies,
            resources: super::fake_records::fixtures(),
            log: Vec::new(),
            inject: VecDeque::new(),
            rate_remaining: RATE_LIMIT,
            rate_reset_unix: now_unix() + 300,
            delay: Duration::ZERO,
            documents: super::fake_records::documents(),
            fail: Vec::new(),
        }
    }
}

pub struct FakeCh {
    pub url: Url,
    state: Arc<Mutex<FakeState>>,
}

impl FakeCh {
    pub async fn start(state: FakeState) -> FakeCh {
        let state = Arc::new(Mutex::new(state));
        let app = axum::Router::new()
            .fallback(handle)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeCh {
            url: Url::parse(&format!("http://{addr}/")).unwrap(),
            state,
        }
    }

    pub fn state(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap()
    }

    pub fn requests(&self) -> Vec<Logged> {
        self.state().log.clone()
    }

    pub fn inject(&self, response: Injected) {
        self.state().inject.push_back(response);
    }
}

fn respond(
    status: u16,
    body: String,
    rate: (u64, u64),
    extra: &[(&'static str, String)],
) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::from_u16(status).unwrap())
        .header("content-type", "application/json")
        .header("x-ratelimit-limit", RATE_LIMIT.to_string())
        .header("x-ratelimit-remain", rate.0.to_string())
        .header("x-ratelimit-reset", rate.1.to_string())
        .header("x-ratelimit-window", "5m");
    for (k, v) in extra {
        builder = builder.header(*k, v);
    }
    builder.body(Body::from(body)).unwrap()
}

fn one_line(addr: &Value) -> String {
    [
        "premises",
        "address_line_1",
        "locality",
        "postal_code",
        "country",
    ]
    .iter()
    .filter_map(|k| addr.get(*k).and_then(Value::as_str))
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(", ")
}

fn param<'a>(query: &'a [(String, String)], name: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn list(query: &[(String, String)], name: &str) -> Vec<String> {
    param(query, name)
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default()
}

fn paging(query: &[(String, String)], size_key: &str, default: usize) -> (usize, usize) {
    let size = param(query, size_key)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default);
    let start = param(query, "start_index")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    (start, size)
}

fn search(state: &FakeState, query: &[(String, String)]) -> (u16, Value) {
    let Some(q) = param(query, "q") else {
        return (
            400,
            json!({"errors": [{"error": "q is required", "type": "ch:validation"}]}),
        );
    };
    let q = q.to_lowercase();
    let active_only = param(query, "restrictions").is_some_and(|r| r.contains("active-companies"));
    let matches: Vec<&Value> = state
        .companies
        .values()
        .filter(|c| {
            c["company_name"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains(&q)
                || c["company_number"].as_str().unwrap().to_lowercase() == q
        })
        .filter(|c| !active_only || c["company_status"] == "active")
        .collect();
    let Some((start, size)) = super::fake_records::search_offset(query) else {
        return (416, Value::Null);
    };
    let items: Vec<Value> = matches
        .iter()
        .skip(start)
        .take(size)
        .map(|c| {
            let number = c["company_number"].as_str().unwrap();
            let mut item = json!({
                "kind": "searchresults#company",
                "title": c["company_name"],
                "company_number": number,
                "company_status": c["company_status"],
                "company_type": c["type"],
                "date_of_creation": c["date_of_creation"],
                "address_snippet": one_line(&c["registered_office_address"]),
                "address": c["registered_office_address"],
                "description": format!("{number} - Incorporated on {}", c["date_of_creation"].as_str().unwrap()),
                "description_identifier": ["incorporated-on"],
                "links": {"self": format!("/company/{number}")},
                "matches": {"title": [1, 4]},
                "snippet": "",
            });
            if let Some(ceased) = c.get("date_of_cessation") {
                item["date_of_cessation"] = ceased.clone();
            }
            item
        })
        .collect();
    (
        200,
        json!({
            "kind": "search#companies",
            "items": items,
            "items_per_page": size,
            "page_number": start / size.max(1) + 1,
            "start_index": start,
            "total_results": matches.len(),
        }),
    )
}

fn advanced(state: &FakeState, query: &[(String, String)]) -> (u16, Value) {
    let includes = param(query, "company_name_includes").map(str::to_lowercase);
    let excludes = param(query, "company_name_excludes").map(str::to_lowercase);
    let statuses = list(query, "company_status");
    let types = list(query, "company_type");
    let subtypes = list(query, "company_subtype");
    let sic = list(query, "sic_codes");
    let location = param(query, "location").map(str::to_lowercase);
    let date_ok = |c: &Value, field: &str, from: &str, to: &str| {
        let d = c.get(field).and_then(Value::as_str);
        let from = param(query, from);
        let to = param(query, to);
        if from.is_none() && to.is_none() {
            return true;
        }
        let Some(d) = d else { return false };
        from.is_none_or(|f| d >= f) && to.is_none_or(|t| d <= t)
    };
    let matches: Vec<&Value> = state
        .companies
        .values()
        .filter(|c| {
            let name = c["company_name"].as_str().unwrap().to_lowercase();
            includes.as_ref().is_none_or(|i| name.contains(i.as_str()))
                && excludes.as_ref().is_none_or(|e| !name.contains(e.as_str()))
                && (statuses.is_empty()
                    || statuses.iter().any(|s| c["company_status"] == s.as_str()))
                && (types.is_empty() || types.iter().any(|t| c["type"] == t.as_str()))
                && (subtypes.is_empty()
                    || subtypes
                        .iter()
                        .any(|s| c.get("subtype").and_then(Value::as_str) == Some(s.as_str())))
                && (sic.is_empty()
                    || c["sic_codes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|code| sic.iter().any(|s| code == s.as_str())))
                && location.as_ref().is_none_or(|l| {
                    one_line(&c["registered_office_address"])
                        .to_lowercase()
                        .contains(l.as_str())
                })
                && date_ok(
                    c,
                    "date_of_creation",
                    "incorporated_from",
                    "incorporated_to",
                )
                && date_ok(c, "date_of_cessation", "dissolved_from", "dissolved_to")
        })
        .collect();
    if matches.is_empty() {
        return (404, json!({}));
    }
    let (start, size) = paging(query, "size", 20);
    let items: Vec<Value> = matches
        .iter()
        .skip(start)
        .take(size)
        .map(|c| {
            let number = c["company_number"].as_str().unwrap();
            let mut item = json!({
                "company_name": c["company_name"],
                "company_number": number,
                "company_status": c["company_status"],
                "company_type": c["type"],
                "date_of_creation": c["date_of_creation"],
                "kind": "search-results#company",
                "links": {"company_profile": format!("/company/{number}")},
                "registered_office_address": c["registered_office_address"],
                "sic_codes": c["sic_codes"],
            });
            for (from, to) in [
                ("subtype", "company_subtype"),
                ("date_of_cessation", "date_of_cessation"),
            ] {
                if let Some(v) = c.get(from) {
                    item[to] = v.clone();
                }
            }
            item
        })
        .collect();
    (
        200,
        json!({
            "etag": "advanced-etag",
            "hits": matches.len(),
            "items": items,
            "kind": "search#advanced-search",
            "top_hit": items.first().cloned().unwrap_or(Value::Null),
        }),
    )
}

fn document_metadata(id: &str, doc: &FakeDocument) -> Value {
    let mut resources = serde_json::Map::new();
    for (content_type, bytes) in &doc.formats {
        let mut r = json!({"created_at": "2024-12-20T10:00:00Z"});
        if doc.sized {
            r["content_length"] = json!(bytes.len());
        }
        resources.insert(content_type.clone(), r);
    }
    json!({
        "company_number": "00000001",
        "barcode": "XBARCODE",
        "category": "accounts",
        "pages": 3,
        "created_at": "2024-12-20T10:00:00Z",
        "significant_date": null,
        "etag": "",
        "links": {
            "self": format!("/document/{id}"),
            "document": format!("/document/{id}/content"),
        },
        "resources": resources,
    })
}

/// Document storage: no API key (a request carrying one is refused, as presigned storage URLs
/// refuse a second form of authentication), the format chosen by the redirect's `type`.
fn storage(st: &FakeState, id: &str, query: &[(String, String)], authorized: bool) -> Response {
    let plain = |status: u16, body: &str| {
        Response::builder()
            .status(status)
            .header("content-type", "text/plain")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    if authorized {
        return plain(400, "only one authentication mechanism is allowed");
    }
    let wanted = param(query, "type").unwrap_or("");
    match st
        .documents
        .get(id)
        .and_then(|d| d.formats.iter().find(|(t, _)| t == wanted))
    {
        Some((content_type, bytes)) => Response::builder()
            .status(200)
            .header("content-type", content_type.as_str())
            .body(Body::from(bytes.clone()))
            .unwrap(),
        None => plain(404, "no such object"),
    }
}

async fn handle(
    State(state): State<Arc<Mutex<FakeState>>>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let path = uri.path().to_string();
    let query: Vec<(String, String)> = uri
        .query()
        .map(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default();
    let authorization = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let delay = {
        let mut st = state.lock().unwrap();
        st.log.push(Logged {
            path: path.clone(),
            query: query.clone(),
            authorization: authorization.clone(),
            accept: headers
                .get("accept")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
        });
        st.delay
    };
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let mut st = state.lock().unwrap();
    if let Some(id) = path.strip_prefix("/storage/") {
        return storage(&st, id, &query, authorization.is_some());
    }
    if authorization.as_deref() != Some(auth_header().as_str()) {
        return respond(
            401,
            json!({"error": "Invalid Authorization", "type": "ch:service"}).to_string(),
            (st.rate_remaining, st.rate_reset_unix),
            &[],
        );
    }
    st.rate_remaining = st.rate_remaining.saturating_sub(1);
    let rate = (st.rate_remaining, st.rate_reset_unix);
    if let Some(inj) = st.inject.pop_front() {
        return respond(inj.status, inj.body, rate, &inj.headers);
    }
    if st.fail.contains(&path) {
        return respond(
            500,
            json!({"error": "internal error"}).to_string(),
            rate,
            &[],
        );
    }
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let accept = headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match segments.as_slice() {
        ["docapi", "document", id] => {
            let (status, body) = match st.documents.get(*id) {
                Some(doc) => (200, document_metadata(id, doc)),
                None => (404, json!({"error": "document not found"})),
            };
            return respond(status, body.to_string(), rate, &[]);
        }
        ["docapi", "document", id, "content"] => {
            let Some(doc) = st.documents.get(*id) else {
                return respond(
                    404,
                    json!({"error": "document not found"}).to_string(),
                    rate,
                    &[],
                );
            };
            if !doc.formats.iter().any(|(t, _)| t == accept) {
                return respond(406, String::new(), rate, &[]);
            }
            let location = format!(
                "/storage/{id}?type={}",
                url::form_urlencoded::byte_serialize(accept.as_bytes()).collect::<String>()
            );
            return respond(302, String::new(), rate, &[("location", location)]);
        }
        _ => {}
    }
    let (status, body) = match segments.as_slice() {
        ["search", "companies"] => search(&st, &query),
        ["advanced-search", "companies"] => advanced(&st, &query),
        ["search", "officers"] => super::fake_records::officer_search(&query),
        ["search", "disqualified-officers"] => super::fake_records::disqualified_search(&query),
        ["dissolved-search", "companies"] => super::fake_records::dissolved(&st.companies, &query),
        ["alphabetical-search", "companies"] => {
            super::fake_records::alphabetical(&st.companies, &query)
        }
        ["company", number] => match st.companies.get(*number) {
            Some(p) => (200, p.clone()),
            None => (
                404,
                json!({
                    "message": format!("404 NOT_FOUND \"Resource not found for company profile {number}\""),
                    "request_id": "fake",
                }),
            ),
        },
        _ => match super::fake_records::canned(&st.resources, &path, &query) {
            Some(v) => (200, v),
            // Like the live API: an unknown disqualified officer is 204 No Content, and the
            // officer list of any company number, even one that does not exist, is a 200.
            None => match segments.as_slice() {
                ["disqualified-officers", "natural" | "corporate", _] => (204, Value::Null),
                ["company", _, "officers"] => (
                    200,
                    json!({
                        "kind": "officer-list", "items": [], "items_per_page": 35, "start_index": 0,
                        "total_results": 0, "active_count": 0, "resigned_count": 0, "inactive_count": 0
                    }),
                ),
                _ => (404, json!({"error": "Not found"})),
            },
        },
    };
    let text = if body.is_null() {
        String::new()
    } else {
        body.to_string()
    };
    respond(status, text, rate, &[]).into_response()
}
