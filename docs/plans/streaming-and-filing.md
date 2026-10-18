# Plan: `/streaming` and `/filing` services

Draft for discussion; no code for these services exists yet. Researched on 17 October 2026 from the Companies House specs, the Developer Hub and the Companies House forum. **Unverified** marks anything the official documentation does not say and I could not confirm. Forum posts are cited as reports, not as policy.

## Summary

Both services can be built, but each does less than its name suggests. The Streaming API is a feed of every change across the whole register, delivered over long-lived connections, with an official limit of two connections per account. The filing API can file only three things: a registered office address change (AD01), a change of registered email address, and insolvency forms that only recognised insolvency software may file. Confirmation statements, accounts, and officer and PSC filings are not available through it. A Companies House staff member confirmed this in September 2025 and said accounts must go through the XML gateway. **Recommendation: build `/streaming` first** as one bounded, stateless `read_stream` tool. It is read-only, has no storage needs and fits the request/response model. It needs a stream key from a **live** application, which is free and self-service; the current key comes from a test application and will keep returning 401. Treat `/filing` as a second phase behind a short go/no-go spike. First prove that the sandbox Companies House sign-in works for this user: several 2025–26 forum reports say sandbox sign-in and test web client creation fail, with no answer from Companies House. Also confirm that changing a registered office or email address from Claude is worth the work, since both are rare, one-off filings that the Companies House website already handles.

**Shared decision.** `COMPANIES_HOUSE_SERVICES` defaults to "all". If "all" grows to include `streaming` and `filing`, existing deployments will fail at startup because they have no stream key or OAuth client. Proposal: the new services run only when listed explicitly, so the default stays `companies`.

**Sign-in.** People now sign in with their own REST key and the server holds no Companies House key. The core has one sign-in per server, and a second enabled service defining `credential_verifier` is a startup error. A `/streaming` service would need either a server-wide stream key (as configured below, without its own verifier), or a core change so people can also give their own stream key at sign-in. `/filing` already signs each person in with Companies House itself, so it fits the per-person model.

## 1. Streaming service (`/streaming`)

### What the API offers (verified)

- Host `https://stream.companieshouse.gov.uk`. Auth is HTTP Basic with the stream key as the username and an empty password. REST keys and stream keys are not interchangeable. Streaming does not run in the sandbox, so the key must come from a live application: Developer Hub, open the application, **Create new key**, choose type **stream key**.
- There are nine streams. Each is a `GET` with an optional `?timepoint=<integer>`:

| Stream | Path | Stream | Path |
|---|---|---|---|
| Company information | `/companies` | Persons with significant control | `/persons-with-significant-control` |
| Filing history | `/filings` | PSC statements | `/persons-with-significant-control-statements` |
| Officers | `/officers` | Disqualified officers | `/disqualified-officers` |
| Charges | `/charges` | Company exemptions | `/company-exemptions` |
| Insolvency cases | `/insolvency-cases` | | |

- Responses are `200` with one JSON document per line, with no end. Blank lines are heartbeats and must be ignored. Errors: `401` (key not authorised), `416` (timepoint invalid or too old), `429` (rate limited).
- Each line is an envelope: `resource_kind`, `resource_id`, `resource_uri`, `data` (the same resource the REST API returns; may be absent on `deleted`) and `event { timepoint, published_at, type: changed | deleted, fields_changed[] }`. `published_at` has the form `yyyy-MM-ddTHH:mm:ss` with no time zone; the zone is unverified. The spec's `resource_kind` enum lists only two values, so the parser must accept unknown values.
- **Timepoints** are sequential counters, one per stream. They cannot be calculated: Companies House staff call them indexes. To resume, reconnect with the last processed timepoint; without one, the stream starts from now. The backlog is a finite event buffer, not a fixed period of time:
  - Companies House staff said in 2020 that it goes back 7 to 10 days.
  - After a bulk load of about 5 million PSC records in late 2025, users saw only about 7 days on the PSC stream.
  - One user reported PSC and officer timepoints expiring between hourly runs.
  - One user reported that an expired timepoint sometimes returns nothing instead of `416` (unverified).
- Not every event comes from a filing. Companies House maintenance work also publishes events, for example the bulk loading of identity verification dates in November 2025.
- **Connection rules from the docs:**
  - At most **two concurrent connections per account**. A third connection closes the oldest one.
  - Clients that read too slowly are disconnected.
  - Frequent reconnects lead to `429`. After a `429`, wait one minute; repeated `429`s make the penalty longer, and ignoring them gets the client IP blocked.
  - Retry other retryable errors after 10 seconds, and network errors after a few seconds.
- **Forum reports, not official:** one user says the limit was raised to 9 per key around 2023. Another reposted a Companies House support answer (December 2025): reconnecting every 30 minutes is acceptable "provided there are no more than 10 active connections per API key". We design to the documented limit of two.

### How it fits a request/response MCP server

**Option A: stateless bounded read (recommended first).** Each tool call opens one connection, reads until a limit is reached, closes it, and returns the events plus `next_timepoint`. The caller keeps the timepoint, so the server stores nothing.
- Good for questions such as "what is being filed right now", "which companies were incorporated in the last few minutes", or replaying from a recent timepoint.
- Poor for "tell me when company X files". A week of backlog is hundreds of thousands to millions of events, which one call cannot scan, so a company filter finds only events inside the window it scanned.

**Option B: background worker with a watch list.**
- The service keeps connections open to the chosen streams and filters by `COMPANIES_HOUSE_STREAM_WATCH`. The watch list is a list of company numbers in config, so the list itself needs no storage.
- Matching events go into an in-memory ring buffer, which tools read.
- **No-storage variant:** a restart loses the buffer and the timepoints. The worker then restarts from "now" and reports `history_since` so answers stay honest.
- **Storage variant:** an optional `COMPANIES_HOUSE_STREAM_STATE_DIR` (this needs a volume) persists the timepoints and the buffer. The worker resumes after a restart if the timepoint is still in the backlog.
- Costs: one connection per watched stream, competing with Option A under the two-connection limit. The service also needs a long-running task. Whether mcp-server-core gives services a start-up and shutdown hook for one is unverified.

**Alternative for a small watch list.** The existing REST key can already answer "what changed for these companies since date D" from the filing history, officers and PSC endpoints, with no stream key and no state. For up to about 50 companies this is simpler than any streaming design. Consider a `changes_since` tool in `/companies` instead of Option B.

### Proposed tools (Option A)

`read_stream`
- `stream` (required): one of the nine names.
- `timepoint` (optional integer). If omitted, reading starts from now.
- `company_numbers` (optional, up to 50, normalised like `get_company`). An event matches when `resource_uri` starts with `/company/{number}/` or `data.company_number` is in the list. Disqualified-officer events are about officers, not companies, and their `resource_uri` shape is unverified, so the filter is refused for that stream until a live test confirms it.
- `event_types` (optional): `changed` and/or `deleted`.
- `max_events` returned (default 50, maximum 200) and `max_seconds` (default 15, maximum `COMPANIES_HOUSE_STREAM_MAX_SECONDS`). How long MCP clients wait for a tool call is unverified.
- `include_data` (default false). By default each event is a compact projection: kind, URI, company number, type, `published_at`, `fields_changed`, timepoint, and a one-line summary (for example filing type and description, or company name and status). With `true`, the full `data` is included, truncated at 16 KB per event.
- Result fields:
  - `events`.
  - `scanned`: lines read, including filtered-out events.
  - `first_timepoint`.
  - `next_timepoint`: the timepoint of the last *scanned* event, so a filtered read still moves forward.
  - `stopped`: `max_events`, `max_seconds`, `idle` (no event for 3 s, which means caught up) or `disconnected`.
  - `gap_warning`: set when the first timepoint is well above the one requested.
- Events with a timepoint at or below the requested one are dropped. Whether Companies House resends the requested event itself is unverified.

`stream_status` reports: the stream URL, connections in use, any `429` cool-down and its remaining seconds, and the last timepoint seen for each stream since start-up (held in memory).

If Option B is built, it adds `watched_events(company_number?, since_timepoint?, limit)` and `stream_worker_status`.

### Configuration

| Variable | Required | Default | Description |
|---|---|---|---|
| `COMPANIES_HOUSE_STREAM_KEY` | yes | | Stream key from a live application. `_FILE` is supported. Same checks as the REST key (no spaces, no colon) |
| `COMPANIES_HOUSE_STREAM_URL` | no | `https://stream.companieshouse.gov.uk` | `https` only. Plain `http` is allowed for loopback addresses in tests |
| `COMPANIES_HOUSE_STREAM_MAX_CONNECTIONS` | no | `1` | Server-wide cap on open stream connections. Maximum `2`, the documented limit |
| `COMPANIES_HOUSE_STREAM_MIN_INTERVAL` | no | `30` | Seconds between two connections to the same stream |
| `COMPANIES_HOUSE_STREAM_MAX_SECONDS` | no | `45` | Upper bound for `max_seconds` |

Option B adds `COMPANIES_HOUSE_STREAM_WATCH`, `COMPANIES_HOUSE_STREAM_WORKER_STREAMS`, `COMPANIES_HOUSE_STREAM_BUFFER_SIZE` and `COMPANIES_HOUSE_STREAM_STATE_DIR`.

### Limits and failure handling

- **Concurrency:** a semaphore enforces `MAX_CONNECTIONS`. If a call cannot get a slot within 5 s, it returns `rate_limited` instead of opening a connection that would close another one. `MIN_INTERVAL` is enforced the same way and the error gives the seconds left.
- **Error mapping** to the existing error kinds:
  - `401` → `auth`. The message says a key from a test application gets 401 and a live one is needed.
  - `416` → `validation` ("timepoint expired"). The message says to read again without a timepoint and to use `/companies` for the gap.
  - `429` → `rate_limited`. The server starts a 60 s cool-down for all streams, doubling on each repeat up to 10 minutes, and makes no connection attempts while it lasts.
  - `5xx` and network errors → `upstream`. There is no retry inside the call, because a retry costs another reconnect.
- **Disconnect mid-stream:** the call returns what it read, with `stopped: disconnected` and a valid `next_timepoint`. It is not an error.
- **Robustness:** read the response incrementally (an HTTP client or proxy that buffers the response breaks streaming). Ignore blank lines. Cap the line length at 4 MiB, skip oversized or invalid lines and count them. Do not follow redirects. Never log the key. Stream text is written by the public, so the server instructions keep treating it as data.

### Testing

- **Fake stream server** in `tests/support`, alongside `fake_ch.rs`. It covers:
  - chunked lines with heartbeats, a slow live tail, and a disconnect in the middle of a line;
  - `401`, `416`, `429` and `5xx` responses;
  - duplicate and gapped timepoints, and oversized lines;
  - a third connection closing the oldest.
- Time limits are tested with tokio's paused clock.
- **Live smoke test,** `#[ignore]` by default. It needs `COMPANIES_HOUSE_STREAM_KEY` from a live application. It reads up to 5 events from `/filings` in at most 30 s, then resumes once from `next_timepoint`. Run it rarely because of the reconnect limits.

### Open questions (streaming)

1. Will you create a live application and stream key? It is free, and the test key will never work.
2. Which matters more: "what is happening on the register now" (Option A) or "tell me about my companies" (a REST `changes_since` tool or Option B)?
3. If Option B: is an in-memory buffer that empties on every restart acceptable, or can the deployment have a volume?
4. Is a default of 30 s between connections to one stream acceptable?

## 2. Filing service (`/filing`)

### What can be filed (verified)

| Filing | Endpoints | Scope (`{n}` = company number) | Request body and rules |
|---|---|---|---|
| Registered office address (AD01) | `POST`, `PUT` and `GET /transactions/{id}/registered-office-address`, plus `GET …/validation-status` | `https://api.company-information.service.gov.uk/company/{n}/registered-office-address.update` | `premises`, `address_line_1`, `address_line_2`, `locality`, `region`, `postal_code` (required since 15 September 2025), `country` (`England`, `Wales`, `Scotland`, `Northern Ireland`, `Great Britain`, `United Kingdom`, `Not specified`), `accept_appropriate_office_address_statement` (s.86(2) Companies Act 2006), `reference_etag` (the etag of `/company/{n}/registered-office-address`; Companies House rejects the request if the etag is stale) |
| Registered email address (change only) | `…/registered-email-address` (same verbs, plus `validation-status`), and `GET /registered-email-address/company/{n}/eligibility` | `…/company/{n}/registered-email-address.update` | `registered_email_address`, `accept_appropriate_email_address_statement` (s.88A(2)). Returns `403` if the company has no registered email address yet. Eligibility codes: `COMPANY_VALID_FOR_SERVICE`, `INVALID_NO_REGISTERED_EMAIL_ADDRESS_EXISTS`, `INVALID_COMPANY_STATUS`, `INVALID_COMPANY_TYPE`, `COMPANY_NOT_FOUND` |
| Insolvency (for example form 600) | `/transactions/{id}/insolvency/…` | `…/company/*/insolvency.write-full` | Only if Companies House registers our `client_id` as recognised insolvency software **and** the user is a registered insolvency practitioner. **Out of scope** |

Nothing else can be filed through this API.

### Transaction model (verified)

1. `POST /transactions` with `{company_number, description, reference?}` returns `201` and a transaction id. `company_number` must match the company in the token's scope.
2. `POST` the filing resource. While the transaction is open, `PUT` replaces the resource. `GET …/validation-status` returns `{is_valid, errors[]}`.
3. `PUT /transactions/{id}` with `{"status":"closed"}` submits the transaction. Possible answers:
   - `204`: submitted.
   - `202`: payment is needed. Not expected for these filings; stop and report it.
   - `403`: already closed.
   - `422`: invalid, with the errors.

   A closed transaction cannot be changed. `DELETE` removes an open transaction.
4. `GET /transactions/{id}` shows `filings[].status` (`processing`, `accepted` or `rejected`) and `reject_reasons[].english`. Live filings may be checked by hand, which delays the result. Companies House emails the signed-in user on submission and again on the outcome. Polling is allowed within the rate limits.

On 19 August 2026 one user saw the close call return `500` with an empty body until the next day. Apart from that, forum users report that ROA and REA filing works in live.

### OAuth with Companies House

**Endpoints (verified):**
- Hosts: `identity.company-information.service.gov.uk` (live) and `identity-sandbox.company-information.service.gov.uk` (sandbox).
- `GET /oauth2/authorise` with `response_type=code`, `client_id`, `redirect_uri`, `scope` (space-separated) and `state`, which is required.
- `POST /oauth2/token`, form-encoded: `client_id`, `client_secret`, `grant_type` (`authorization_code` or `refresh_token`), `code`, `redirect_uri`.
- `GET /oauth2/verify` returns `expires_in`, `scope` and `user_id`.
- `GET /user/profile`.

The Developer Hub's authentication page says `/oauth/token` and `/oauth/verify` instead. The spec, Companies House's own third-party test harness and forum users all use `/oauth2/…`. Use `/oauth2` and confirm it in the sandbox.

**Scopes:**
- Scopes always use the live domains, even in the sandbox (verified).
- `https://identity.company-information.service.gov.uk/user/profile.read` is required for every filing (verified).
- A forum user (June 2026) found that `profile.read` on its own now fails unless a company scope is requested with it.
- A forum user (2024, unanswered) found that scopes for two different companies in one request fail. This is unverified, but the design assumes **one company per sign-in**.

**Sign-in:**
- Sign-in now goes through GOV.UK One Login with a one-time code (forum reports, 2026).
- When a scope names a company, the user types that company's authentication code into Companies House's own page (verified), so the code never reaches our server.
- The token response contains `access_token`, `expires_in` and `refresh_token`. A refresh token is returned only for the `authorization_code` grant, so refreshing does not rotate it. Token lifetimes are not documented (unverified).

**How it works on a self-hosted server:**
- **Two separate OAuth roles.** Here the server is an OAuth *client* of Companies House; this is separate from its own OAuth server for MCP clients.
- **Setup:** register a web client in the Developer Hub with the redirect URI `{PUBLIC_URL}/filing/callback`, which must match character for character. Use a test application for the sandbox and a live application for live.
- **Callback route:** the service adds this route to its router. It is a cross-site `GET`, so it is checked against `state` and not against cookies or MCP auth.
- **The link:** the tool hands the user the `/oauth2/authorise` URL itself. A forum report says passing out the intermediate URL it redirects to breaks the flow.

**Where tokens live:**
- **Recommended, no storage: one sign-in per filing.**
  1. `start_sign_in` returns a link.
  2. The person opens it, signs in, enters the authentication code and approves.
  3. The callback exchanges the code, then calls `/oauth2/verify` to check that the granted scope matches the company and filing type.
  4. The tokens stay in memory only, tied to that session. They are deleted after submission or after 30 minutes.

  A restart loses unfinished sessions; the person just signs in again. AD01 and REA changes are rare, so signing in for each filing costs little, and it gives a fresh human consent every time.
- **Possible, needs storage: keep refresh tokens.** Encrypt them at rest with a new secret. This only helps with checking the status days later, which the Companies House emails and the `/companies` service already cover: an accepted address appears on the public register. Not recommended.
- **Rejected: give sealed tokens to the MCP client.** They would end up in chat transcripts.

The MCP spec's URL-mode elicitation (2025-11-25) could open the sign-in link for the user. Support for it in rmcp and claude.ai is unverified, so a link in the tool result is the baseline.

### Proposed tools

Read-only tools, always present:
- `filing_status`: environment (sandbox or live), whether writes are enabled, the filings supported, and open sign-in sessions with their companies.
- `start_sign_in(company_number, filing: registered_office_address | registered_email_address)`: returns the sign-in URL, a `session_id` and the expiry time. It **never** takes an authentication code.
- `check_sign_in(session_id)`: pending, signed in (the email from `/user/profile` and the granted scopes), or failed with the Companies House error.
- `get_registered_office_address(session_id)`: the current address and its etag, read with the session's token.
- `check_email_eligibility(session_id)`: the eligibility code in plain words.
- `get_filing(transaction_id)`: transaction status, and for each filing whether it was accepted or rejected, with reasons.

Write tools, present only when `COMPANIES_HOUSE_ALLOW_FILING=true`:
- `prepare_registered_office_address_change(session_id, address fields, accept_statement)`:
  - reads the current address and etag;
  - creates the open transaction and resource, or reuses them if this session already has one;
  - runs `validation-status`;
  - returns the old and new address, the validation result, the `transaction_id` and a `confirmation` digest of the transaction id, payload and etag.

  Nothing is submitted.
- `prepare_registered_email_address_change(session_id, email, accept_statement)`: the same, after the eligibility check.
- `submit_filing(transaction_id, confirmation)`: re-reads the etag and the validation status and refuses if anything changed; otherwise it closes the transaction once. Annotated as destructive.
- `cancel_filing(transaction_id)`: deletes an open transaction.

### Safety design

- **Off by default, twice.** `filing` must be listed in `COMPANIES_HOUSE_SERVICES`, and the write tools do not exist unless `COMPANIES_HOUSE_ALLOW_FILING=true`, following `ICLOUD_ALLOW_SEND`.
- **Sandbox by default.** `COMPANIES_HOUSE_FILING_ENV` defaults to `sandbox`, and `live` must be set explicitly. It sets the identity and API hosts together; forum threads show that mixing live and sandbox hosts is a common failure. Every tool result and the callback page say which environment is in use.
- **Optional allow-list.** With `COMPANIES_HOUSE_FILING_COMPANIES` set, sign-in is refused for any company not on the list.
- **Human confirmation, in layers:**
  - The Companies House consent screen asks the person to approve for each company and filing type.
  - The prepared change is shown in full.
  - `submit_filing` requires the `confirmation` digest.
  - The destructive annotation makes clients ask before calling it.
  - When the client supports MCP form elicitation, it asks for a final yes.
- **Legal statements.** The statements under s.86(2) and s.88A(2) must be passed explicitly. The tool descriptions quote them and tell the model to ask the user, never to assume.
- **Preconditions and idempotency:**
  - `reference_etag` guards against a concurrent address change (Companies House enforces it too).
  - Each session has at most one open transaction per filing.
  - `submit_filing` never retries on its own. After a timeout or `5xx` it reads the transaction to see whether it closed.
  - Submitting an already-closed transaction returns its status rather than an error.
- **Secrets:**
  - Never log the client secret, tokens, `code` or `state`.
  - Remove the callback's query string from access logs; check what mcp-server-core logs today.
  - Log registered email addresses only under `DEBUG_UNSAFE_LOG_CONTENT`.
  - Write one info-level audit line per submission: environment, company number, filing type, transaction id and outcome.

### Configuration

| Variable | Required | Default | Description |
|---|---|---|---|
| `COMPANIES_HOUSE_OAUTH_CLIENT_ID`, `COMPANIES_HOUSE_OAUTH_CLIENT_SECRET` | yes | | Web client from a test (sandbox) or live application. `_FILE` is supported |
| `COMPANIES_HOUSE_FILING_ENV` | no | `sandbox` | `sandbox` or `live`. Sets the identity and API hosts together |
| `COMPANIES_HOUSE_ALLOW_FILING` | no | `false` | Adds the prepare, submit and cancel tools |
| `COMPANIES_HOUSE_FILING_COMPANIES` | no | | Optional allow-list of company numbers |
| `COMPANIES_HOUSE_IDENTITY_URL`, `COMPANIES_HOUSE_FILING_API_URL` | no | set by the environment | Overrides for tests. Plain `http` is allowed only for loopback addresses |

### Sandbox testing

- **Automated tests** use an in-process fake identity service and filing API. They cover: the authorise redirect, token exchange and verify; transactions, ROA and REA resources and `validation-status`; a stale etag; `202`, `403`, `422` and `500` on close; and the mock outcomes below.
- **Manual end-to-end test.** A person has to sign in, so this cannot run in CI.
  1. Create a test company with the test data generator. The existing `live_sandbox` test already does this and receives the company's `auth_code`.
  2. Call `start_sign_in` against `identity-sandbox`, sign in as a sandbox user and enter the authentication code.
  3. Prepare and submit an ROA change, then poll until it is accepted.
  4. Delete the test company.
- **Companies House mock processing (verified):**
  - An ROA change is rejected when the postcode is `CF143UZ`, `BT28BG`, `SW1H9EX` or `EH39FF`.
  - An REA change is rejected when the address ends in `@companieshouse.gov.uk`.
  - Everything else is accepted.

  The mock never updates the test company, so success shows only in the transaction.
- **Risks:**
  - 2025–26 forum threads report that sandbox user accounts cannot be created ("There is no Companies House account that uses this email address") and that creating a web client in a test application returns `403`. Companies House has answered neither.
  - Whether generated test companies have a registered email address, which an REA change requires, is unverified.

### Approval and restrictions

- **ROA and REA:** no approval step is documented. The "Moving to live" guidance lists only configuration changes: hosts, client credentials and API key. Companies House staff describe the platform as free to develop for and use, with limited support.
- **Insolvency:** the scope is granted only if Companies House registers the client as recognised insolvency software, and the person signing in must be a registered insolvency practitioner. In the sandbox, any email containing `ip-test` counts as a practitioner.
- **Scopes enabled on request:** some scopes are enabled per client when you ask Companies House; in October 2026 a user asked for `psc-discrepancy-reports.write-full`. Whether ROA and REA scopes need this is unverified; the sandbox spike will show it.
- **Presenter identity verification:** checks or ACSP registration for anyone filing will apply **no earlier than November 2027**, with at least six months' notice (ICAEW, 13 August 2026). How this will affect API filing is unverified.
- **Rate limits:** no separate rate limit is documented for the OAuth APIs. Assume the 600 requests per 5 minutes (unverified).

### Open questions (filing)

1. Is ROA and REA filing from Claude worth building, given that nothing else can be filed?
2. **Go/no-go spike:** can you create a test web client and sign in at `identity-sandbox` as a sandbox user? This takes about half a day with curl and a browser, before any service code.
3. Is a public, unauthenticated callback route under `PUBLIC_URL` acceptable?
4. Is one sign-in per filing, with tokens held only in memory, acceptable?
5. Do you want an allow-list of company numbers?
6. For live filings, who will sign in, and does that person hold the authentication codes?

## Sources

- Spec list: https://developer-specs.company-information.service.gov.uk/
- Streaming: https://developer-specs.company-information.service.gov.uk/streaming-api/guides/overview, …/streaming-api/guides/authentication, …/streaming-api/reference, …/streaming-api/reference/filing-history/stream, …/streaming-api/reference/company-information/stream
- Streaming swagger: https://developer-specs.company-information.service.gov.uk/api.ch.gov.uk-specifications/swagger-2.0/spec/streaming.json, …/spec/stream.json, …/models/stream/streamEnvelope.json, …/models/stream/filingHistory.json
- Filing: https://developer-specs.company-information.service.gov.uk/manipulate-company-data-api-filing/reference, …/manipulate-company-data-api-filing/guides/overview
- Filing swagger: …/api.ch.gov.uk-specifications/swagger-2.0/spec/filings-public.json, …/spec/transactions.json, …/spec/companyAddress.json, …/spec/registeredEmailAddress.json, …/models/transactions.json, …/models/registeredOfficeAddress.json, …/models/registeredEmailAddress.json, …/models/errors.json
- Identity: https://developer-specs.company-information.service.gov.uk/companies-house-identity-service/reference, …/companies-house-identity-service/guides/ServerWeb, …/account.ch.gov.uk-specifications/swagger-2.0/identity-public.json
- Test data: https://developer-specs.company-information.service.gov.uk/sandbox-test-data-generator-api/reference
- Developer Hub: https://developer.company-information.service.gov.uk/authentication, /api-testing, /developer-guidelines, /how-to-create-an-application
- Test harness: https://raw.githubusercontent.com/companieshouse/third-party-test-harness/master/README.md
- Forum (https://forum.companieshouse.gov.uk/t/…):
  - streaming limits: 12813, 4912, 12502
  - timepoints: 8524, 3008, 12375
  - outage: 12821
  - non-filing events: 12329
  - sandbox and OAuth: 12082, 12187, 11352, 12700, 12742
  - multi-company scopes: 7541
  - REA in live: 12949
  - scope enablement request: 13071
- ICAEW: https://www.icaew.com/insights/viewpoints-on-the-news/2026/aug-2026/companies-house-presenter-verification-deadline-changed
