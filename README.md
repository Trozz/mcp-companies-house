# mcp-companies-house

A self-hosted [Model Context Protocol](https://modelcontextprotocol.io) server for the UK [Companies House](https://developer.company-information.service.gov.uk) register. MCP clients such as claude.ai, Claude Desktop and Claude Code can search and filter the register, read company profiles, officers, owners, filings, charges and insolvency records, and put together a due-diligence overview of a company.

| Service | Path | What it does |
|---|---|---|
| Companies | `/companies` | Search and filter companies; read profiles, officers and their appointments, persons with significant control, filing history, charges, insolvency and disqualifications; look up SIC codes; one-call company reports |

- Single static Rust binary, shipped as a `FROM scratch` container image.
- TLS end to end: the server terminates TLS itself, and every call to Companies House is verified.
- Each person signs in with their own Companies House API key, so one server serves many people. The server holds no Companies House key.
- Built-in OAuth 2.1 authorization server so each service works as a claude.ai remote connector. No database, no volume.
- Read only: the public data API cannot change anything at Companies House.

The core (OAuth, TLS, routing, observability) started as a copy of `mcp-server-core` from [mcp-icloud](https://github.com/Trozz/mcp-icloud) and lives in `crates/mcp-server-core`, where it gained per-person sign-in.

## How it works

The server calls the Companies House [Public Data API](https://developer-specs.company-information.service.gov.uk/companies-house-public-data-api/reference) with the REST API key of the person using it, sent as the HTTP Basic username with a blank password:

| Environment | Host | Data |
|---|---|---|
| Live (default) | `api.company-information.service.gov.uk` | The real register |
| Sandbox | `api-sandbox.company-information.service.gov.uk` | Test companies you create with the test data generator; search answers are mostly empty |

Keys belong to one environment: a sandbox key is rejected by the live API and the other way round. `COMPANIES_HOUSE_API_URL` sets the environment for the whole server, so everyone needs a key for that environment.

MCP clients reach each service over HTTPS at its path (`https://<host>/companies`). Each person brings their own Companies House REST API key, in one of two ways:

- **Sign in (OAuth, for claude.ai and Claude Desktop).** The server's sign-in page asks for the key and checks it with Companies House. It is then encrypted into that connection's OAuth tokens; the server stores nothing.
- **Key as the bearer token (Claude Code and other clients that can send a header).** The client sends `Authorization: Bearer <key>`. The server checks the key with Companies House the first time it sees it.

Every Companies House request then uses the caller's own key, so each person has their own rate limit and sees only what their key may see.

`COMPANIES_HOUSE_SERVICES` chooses which services run (default: all). A service that is not enabled is not mounted and does not read its configuration.

## Company tools (`/companies`)

### Finding companies

| Tool | What it does |
|---|---|
| `search_companies` | Search by company name or number, best match first. Optional `active_only`. Paged with `limit` (1 to 100, default 20) and `start_index` |
| `filter_companies` | The Companies House advanced search: `name_includes`, `name_excludes`, `status`, `company_type`, `subtype`, `sic_codes`, `location` (registered office), `incorporated_from`/`incorporated_to`, `dissolved_from`/`dissolved_to`. Filters combine with AND, values within a filter with OR. `total` counts every match, so `limit: 1` counts cheaply |
| `search_dissolved_companies` | Search dissolved companies by name, or by a name they had before (`match_on: previous_names`) |
| `check_company_name_availability` | Active companies whose name Companies House treats as the same as a proposed name; `available` is true when there are none. Sensitive words and "too like" rules are not checked |
| `browse_company_names` | Companies in register (alphabetical) order around a name |
| `lookup_sic_codes` | Find SIC 2007 codes by words ("bakery", "software development") or digits, or describe codes. Works offline from the Companies House condensed list |

### Reading a company

| Tool | What it does |
|---|---|
| `get_company` | Profile: status, type, dates, registered office, SIC codes with descriptions (`nature_of_business`), previous names, accounts and confirmation statement due dates (and whether overdue), and `records_available` (for example `officers`, `charges`, `insolvency`) |
| `company_report` | One call for due diligence: profile, current officers, current persons with significant control and PSC statements, recent filings, charges not yet satisfied and insolvency cases. Up to 7 Companies House requests (charges and insolvency only when the company has them); sections that fail are listed under `unavailable` instead of failing the call |
| `list_officers` | Directors, secretaries, LLP members and others, current and resigned, with `officer_id` |
| `list_persons_with_significant_control` | Owners and controllers (and beneficial owners of overseas entities) with natures of control; the first page also has PSC statements |
| `list_filing_history` | Filings newest first, optionally by `categories` (for example `accounts`, `officers`), with the Companies House description key and `document_id` |
| `list_charges`, `get_charge` | Mortgages and other secured lending: status, lender, what is charged, dates; `get_charge` gives the full record |
| `get_insolvency` | Insolvency cases with their dates and practitioners |

### People

| Tool | What it does |
|---|---|
| `search_officers` | Officers by name, with `officer_id` and number of appointments |
| `get_officer_appointments` | Every appointment of one officer across companies, optionally only current ones |
| `search_disqualified_officers`, `get_disqualified_officer` | People and companies disqualified from being directors, and the details of each disqualification |

### Conventions

- Companies are addressed by company number. Tools ignore spaces, upper-case letters and pad short numbers with zeros (`445790` becomes `00445790`, `SC1234` becomes `SC001234`). Officers, charges and disqualified officers are addressed by the ids the list and search tools return.
- Status, type and subtype are the exact Companies House codes (`active`, `liquidation`, `ltd`, `llp`, `community-interest-company`, ...). The tool schema lists every accepted value, so clients cannot send a code the API does not know.
- Dates are `YYYY-MM-DD`. SIC codes are the 4 or 5 digit SIC 2007 codes, for example `62012`; `lookup_sic_codes` finds them.
- Officers' and owners' dates of birth are shown as month and year only, as Companies House publishes them.
- Filing descriptions are Companies House description keys (for example `accounts-with-accounts-type-micro-entity`) with their values, not the wording shown on the Companies House website.
- List results share one shape: `total`, `start_index`, `count`, `items` and, when more results exist, `next_start_index` to pass as the next `start_index`.
- `search_companies`, `search_officers` and `search_disqualified_officers` reach only the first 1000 matches (a Companies House limit) and serve pages aligned to `limit`: a `start_index` that is not a multiple of `limit` returns the rest of that page, and `note` says when matches exist beyond the first 1000.
- Errors are tool results whose text starts with a kind: `validation`, `not_found`, `auth`, `rate_limited`, `upstream` or `internal`.

### Rate limit

Companies House allows 600 requests per 5 minutes per key and answers `429` for the rest of the window once it is used up; applications that keep exceeding it can be banned. The server tracks the `X-Ratelimit-*` headers separately for each person's key: when a key's window is used up, that person's tools return `rate_limited` with the seconds until it resets, without calling Companies House again until then. Other people are not affected. A request is retried once only after a network error or a `5xx`.

### SIC codes

`lookup_sic_codes` and the `nature_of_business` descriptions use Companies House's [SIC 2007 condensed list](https://www.gov.uk/government/publications/standard-industrial-classification-of-economic-activities-sic) (731 codes, including the Companies House codes `74990` Non-trading company and `99999` Dormant Company), stored in `crates/mcp-companies-house-data/data/sic2007.tsv`. Contains public sector information licensed under the Open Government Licence v3.0.

## Quick start

### 1. Run with Docker

```bash
docker run -d --name mcp-companies-house -p 8443:8443 \
  -e PUBLIC_URL=https://companies-house.example.com \
  -e OAUTH_SIGNING_KEY="$(openssl rand -hex 32)" \
  -v /path/to/certs:/certs:ro \
  -e TLS_CERT_PATH=/certs/fullchain.pem \
  -e TLS_KEY_PATH=/certs/privkey.pem \
  ghcr.io/trozz/mcp-companies-house:latest
```

This serves the live register. To serve the sandbox instead, add `-e COMPANIES_HOUSE_API_URL=https://api-sandbox.company-information.service.gov.uk`; people then need sandbox keys. Keep `OAUTH_SIGNING_KEY` stable: changing it signs everyone out. Store it in a secret store, not in shell history.

### 2. Or run the binary

Download the archive for your platform from [Releases](https://github.com/Trozz/mcp-companies-house/releases), verify the checksum, extract it and run:

```bash
shasum -a 256 -c mcp-companies-house-<version>-<target>.tar.gz.sha256
tar -xzf mcp-companies-house-<version>-<target>.tar.gz
cd mcp-companies-house-<version>-<target>
PUBLIC_URL=... OAUTH_SIGNING_KEY=... ./mcp-companies-house
```

`./mcp-companies-house --version` prints the version. `./mcp-companies-house --healthcheck` connects to `127.0.0.1` and exits 0 when the local server answers `/healthz` (used by the image `HEALTHCHECK`). `BIND_ADDR` must therefore listen on loopback; the default `0.0.0.0` does.

## Connecting clients

Each service is a separate MCP server, so add one connector per service you want.

### Get your API key

Everyone who uses the server needs their own key:

1. Register at the [Companies House Developer Hub](https://developer.company-information.service.gov.uk/get-started/) and create an application. Choose a **live** application for a server on the live register, or **test** for a sandbox server.
2. In the application, create a **REST** API key. (Stream keys and web OAuth clients are for other APIs and do not work here.)
3. If you restrict the key to IP addresses, include the address the server calls Companies House from.

### claude.ai, Claude Desktop and Claude mobile (remote connector)

1. In claude.ai open **Settings**, then **Connectors**, then **Add custom connector**.
2. Enter `https://<your PUBLIC_URL host>/companies`.
3. A sign-in page from this server opens, naming the service. Enter your Companies House REST API key.

claude.ai connects from Anthropic's servers, so the server must be reachable from the internet with a **publicly trusted** certificate (directly through `TLS_CERT_PATH`/`TLS_KEY_PATH`, or at a reverse proxy or tunnel in front of it).

### Claude Code

Send your key as the bearer token:

```bash
claude mcp add --transport http companies-house https://companies-house.example.com/companies \
  --header "Authorization: Bearer $COMPANIES_HOUSE_API_KEY"
```

`$COMPANIES_HOUSE_API_KEY` here is your own key on your own machine. Without `--header`, Claude Code uses the sign-in page instead (run `/mcp` to authenticate).

## Configuration

Every secret can also be read from a file by appending `_FILE` to its name, for example `OAUTH_SIGNING_KEY_FILE=/run/secrets/oauth-signing-key`. A directly set value wins over the file.

The server has no Companies House key of its own, and has no admin password or static token.

### Server

| Variable | Required | Default | Description |
|---|---|---|---|
| `COMPANIES_HOUSE_SERVICES` | no | all (`companies`) | Comma separated services to run. An unknown name or an empty list stops startup |
| `PUBLIC_URL` | yes | | External base URL, for example `https://companies-house.example.com`. `http` is accepted only for `localhost` |
| `OAUTH_SIGNING_KEY` | yes | | At least 32 bytes. Signs OAuth tokens and encrypts the keys inside them. Rotating it signs everyone out |
| `TRUSTED_PROXIES` | no | | Comma separated proxy IPs whose `X-Forwarded-For` is trusted for sign-in rate limiting |
| `ALLOWED_ORIGINS` | no | | Extra allowed `Origin` values, comma separated |
| `BIND_ADDR` | no | `0.0.0.0:8443` | Listen address. Must listen on loopback (`0.0.0.0` does) so `--healthcheck` can connect to `127.0.0.1` |
| `TLS_CERT_PATH`, `TLS_KEY_PATH` | no | | PEM certificate chain and key. Reloaded automatically when the files change |
| `INSECURE_HTTP` | no | `false` | Serve plain HTTP without TLS |
| `LOG_LEVEL` | no | `info` | Log filter, for example `info,mcp_companies_house_data::api=debug` |
| `LOG_FORMAT` | no | `json` | `json` or `pretty` |
| `DEBUG` | no | off | Debug logging: tool arguments, error messages, Companies House paths, statuses and timings. Never logs API keys |
| `DEBUG_UNSAFE_LOG_CONTENT` | no | off | Also logs full tool results and Companies House response bodies. API keys are never logged |
| `METRICS_ADDR` | no | | Enables a Prometheus listener, for example `0.0.0.0:9090` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | no | | Enables OpenTelemetry trace export over OTLP/HTTP |

### Companies

| Variable | Required | Default | Description |
|---|---|---|---|
| `COMPANIES_HOUSE_API_URL` | no | `https://api.company-information.service.gov.uk` | API base URL, and so the environment everyone's keys must belong to. Use `https://api-sandbox.company-information.service.gov.uk` for the sandbox. Must be `https`, except a loopback address for testing |

## TLS

The server picks its certificate at startup:

1. `INSECURE_HTTP=true`: plain HTTP. Use only when TLS is terminated elsewhere on a network you trust.
2. `TLS_CERT_PATH` and `TLS_KEY_PATH` both set: load them. If either file is missing or invalid the server refuses to start; it never falls back silently. Setting only one of them is an error.
3. Neither set: an in-memory self-signed certificate is generated for the `PUBLIC_URL` host, `localhost`, `127.0.0.1` and `::1`. It is never written to disk and changes on every restart. The log shows its SHA-256 fingerprint.

The self-signed certificate is fine for local use, Claude Code, or encrypting the hop from a reverse proxy or tunnel that skips verification or pins the fingerprint. **It does not work for claude.ai directly**, which requires a publicly trusted certificate.

### A stable local certificate with mkcert

```bash
brew install mkcert
mkcert -install
mkcert -cert-file cert.pem -key-file key.pem localhost 127.0.0.1 ::1
TLS_CERT_PATH=$PWD/cert.pem TLS_KEY_PATH=$PWD/key.pem ./mcp-companies-house
```

## Observability

- Logs go to stdout as JSON by default. Every tool call is a span and is logged with its outcome and duration; OAuth requests are logged the same way. Companies House requests inside a tool call are logged at debug level.
- Failed tool calls are logged at `warn` with the tool and the error kind only. `DEBUG=1` adds tool arguments (search terms included), error messages, Companies House paths, statuses and timings. API keys and the `Authorization` header are never logged.
- `DEBUG_UNSAFE_LOG_CONTENT=1` also writes tool results and Companies House response bodies into the logs. The server prints a warning at startup.
- `METRICS_ADDR` starts a separate Prometheus listener (plain HTTP, keep it on an internal network): `mcp_tool_calls_total`, `mcp_tool_duration_seconds`, `companies_house_requests_total`, `companies_house_request_duration_seconds`, `companies_house_rate_limited_total`, `oauth_requests_total`, `oauth_login_failures_total`, `bearer_credential_checks_total`, `http_requests_total`.
- `OTEL_EXPORTER_OTLP_ENDPOINT` (with the standard `OTEL_EXPORTER_OTLP_HEADERS`) exports traces over OTLP/HTTP. Nothing is exported unless it is set.

## Security notes

- **Who can use it.** Anyone with a valid Companies House REST key for the server's environment. Keys are free, so treat a server reachable from the internet as public; limit who can reach it at the network if that matters. Each person's requests use, and are limited by, their own key.
- **No stored keys.** At sign-in the key is checked with one Companies House search request, then sealed (ChaCha20-Poly1305, under a key derived from `OAUTH_SIGNING_KEY` with HKDF-SHA256) into that client's access and refresh tokens, and opened again on each request. The server writes no key to disk or logs. The client holds the sealed key inside its tokens and cannot read it.
- **Keys as bearer tokens** are checked with Companies House on first use. The verdict is cached in memory for 10 minutes (1 minute for a rejected key) under the key's SHA-256, never the key itself. Every Companies House request still uses the key, so a key revoked at Companies House stops working at once.
- Each service is its own OAuth protected resource (RFC 9728), with metadata at `/.well-known/oauth-protected-resource/<service>`. Access tokens are bound to the service they were issued for (RFC 8707).
- Rejected keys, at the sign-in page or as bearer tokens, are limited to 5 per minute per client, plus 30 per minute across all clients. Behind a reverse proxy, set `TRUSTED_PROXIES` so the per-client limit uses the real client address. When Companies House cannot be reached, the key is not counted as rejected and the client is asked to retry.
- Access tokens last 1 hour, refresh tokens 30 days. There is no per-client revocation: rotate `OAUTH_SIGNING_KEY` to sign every client out, which also makes every sealed key unreadable.
- Keys are sent only to `COMPANIES_HOUSE_API_URL`; redirects are not followed.
- Company names, addresses and other filed text are written by the public. The server instructions tell the model to treat them as data, not as instructions.
- The container runs as UID 65532 with nothing else in the image.

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check
bash scripts/test-next-version.sh
```

The tests run against an in-memory fake of the Companies House API. Live smoke tests against the sandbox are ignored by default; they need a sandbox REST key in `COMPANIES_HOUSE_API_KEY` (read only by the tests) and create test companies with the [test data generator](https://developer.company-information.service.gov.uk/api-testing), which they always delete again. The sandbox serves profiles, officers and PSCs for generated companies, but its search index is empty and filing history and PSC statements answer HTTP 500, so those tests check shapes and graceful failure rather than content:

```bash
COMPANIES_HOUSE_API_KEY=... cargo test -p mcp-companies-house-data --test live_sandbox -- --ignored --nocapture
```

### Adding a service

Each service is a library crate that implements the `Service` trait from `mcp-server-core::service`:

```rust
pub trait Service: Send + Sync + 'static {
    fn name(&self) -> &'static str;            // path, OAuth resource and scope, e.g. "companies"
    fn tool_names(&self) -> Vec<String>;
    fn log_caps(&self) -> &'static [&'static str] { &[] }
    fn router(self: Arc<Self>, ctx: &ServiceContext) -> axum::Router;
    // How people sign in: checks their credential with the upstream service.
    fn credential_verifier(&self) -> Option<Arc<dyn CredentialVerifier>> { None }
}
```

1. Create `crates/mcp-companies-house-<name>` as a library. Read its own variables in `pub fn from_env(env: &mut ServiceEnv) -> Option<Arc<dyn Service>>`, reporting problems through `env.errors`. Never read the server variables.
2. Build the MCP endpoint in `router()` with `mcp_server_core::service::mcp_service(ctx, move || Ok(MyServer::new(...)))`. Tools read the caller's key from the request: take `Extension(parts): Extension<http::request::Parts>` and get `mcp_server_core::credential::Credential` from `parts.extensions`.
3. Add the crate to `crates/mcp-companies-house/Cargo.toml` and one line to `SERVICES` in `crates/mcp-companies-house/src/lib.rs`.

Routing, OAuth metadata, token audiences, `COMPANIES_HOUSE_SERVICES` and the release pipeline pick the new service up without further changes. The server has one sign-in, taken from the first enabled service that defines `credential_verifier`, so a service that needs a different credential (for example a stream key) needs a core change first.

### Planned services

`docs/plans/streaming-and-filing.md` proposes a `/streaming` service (Streaming API, needs a stream key from a live application) and a `/filing` service (registered office and registered email address changes through the Companies House filing API and OAuth). Neither is built yet.

### The core

`crates/mcp-server-core` was copied from `Trozz/mcp-icloud` at commit `d98557d` and has since gained per-person sign-in (`src/credential.rs`; `OAUTH_ADMIN_PASSWORD` and `MCP_AUTH_TOKEN` removed). Port fixes between the two by hand.

## Releases

- Work happens on `feature/*` and `bugfix/*` branches merged into `develop` by pull request.
- Every merge into `develop` that touches the server publishes a **pre-release** `vX.Y.Z-dev.N` (GitHub pre-release, image tags `X.Y.Z-dev.N` and `develop`).
- A pull request from `develop` into `main` publishes a **release** `vX.Y.Z` when merged (image tags `X.Y.Z`, `X.Y`, `X`, `latest`).
- The version bump comes from the `develop` to `main` pull request: a `major`, `minor` or `patch` label, otherwise `[major]`, `[minor]` or `[patch]` in its title, otherwise patch. Pre-releases on `develop` follow the currently open release pull request.
- Only merge commits are allowed. Squash and rebase merging are disabled so `develop` and `main` never diverge.
- Pull requests into `main` must come from `develop` in this repository.

### Repository setup (once)

```bash
git push origin main:develop
gh label create major --color B60205 --description "Release: major version bump"
gh label create minor --color FBCA04 --description "Release: minor version bump"
gh label create patch --color 0E8A16 --description "Release: patch version bump"
```

Under **Settings > Actions > General > Workflow permissions**, the workflows must be allowed to grant `contents: write`, `packages: write`, `id-token: write` and `attestations: write` (the jobs request them explicitly through `permissions` blocks).

After the first image is published, set the `mcp-companies-house` package visibility to public under **Packages** on GitHub if you want anonymous pulls.

## License

MIT
