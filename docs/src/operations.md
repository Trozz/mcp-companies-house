# Operations

This page covers running the server day to day: health checks, logs, metrics and traces, upgrading and rotating the signing key, how the rate limits look from the operator's side, and running more than one instance.

## Health

- `GET /healthz` answers `ok` without authentication. It shows that the server is serving; it does not call Companies House.
- `mcp-companies-house --healthcheck` connects to `127.0.0.1` on the `BIND_ADDR` port, asks for `/healthz` and exits 0 when it gets `200`. It uses TLS unless `INSECURE_HTTP` is set, without checking the certificate, since it only ever connects to the local machine. It gives up after 5 seconds. It reads `BIND_ADDR` and `INSECURE_HTTP` itself, so run it with the same values as the server.
- The container image uses it as its `HEALTHCHECK`: every 30 seconds, a 5 second timeout, 10 seconds to start and 3 retries. `docker ps` then shows the container as `healthy`.
- `mcp-companies-house --version` prints the version, and `--help` a short usage line. Any other argument prints `unknown argument: ...` and exits with code 2.

## Logs

Logs go to stdout. With `LOG_FORMAT=json` (the default) each line is one JSON object, with the event's fields and the span it happened in. `LOG_FORMAT=pretty` prints coloured, multi-line entries for people.

[Getting started](getting-started.md#5-check-it-is-running) lists the startup lines. While it runs, the server logs:

| Message | Level | Fields | When |
|---|---|---|---|
| `tool call` | info | `tool`, `outcome` (`ok` or `error`), `elapsed_ms` | Every tool call |
| `tool failed` | warn | `tool`, `kind` | A tool call that failed, with the [kind of error](conventions.md#errors) only |
| `rate limit used up; not calling Companies House` | warn | `endpoint`, `reset_in_secs` | A request refused because the key's rate-limit window is used up |
| `oauth authorization granted` | info | `subject` | Someone signed in on the sign-in page |
| `oauth login failed: credential rejected` | warn | `ip` | A key rejected on the sign-in page |
| `oauth login: credential could not be checked` | warn | `ip`, `reason` | A key that could not be checked on the sign-in page |
| `oauth client registered` | info | `client_name` | A client registered itself |
| `authorization code replay rejected` | warn | | An authorization code used a second time |
| `bearer credential rejected` | warn | `ip` | A key sent as the bearer token was rejected |
| `bearer credential could not be checked` | warn | `reason` | A bearer key that could not be checked |
| `tls certificate reloaded` | info | | New certificate files were loaded |
| `tls certificate reload failed, keeping previous certificate` | warn | `error` | Changed certificate files could not be loaded |
| `shutdown signal received` | info | | `SIGTERM` or Ctrl-C |

Every HTTP request is a span named `request` with its `method` and `path`, without the query string. Every tool call is a span named `tool` with the tool's name.

### Levels

`LOG_LEVEL` takes a `tracing` filter, such as `info` or `info,mcp_companies_house_data::api=debug`. The targets are:

| Target | What it covers |
|---|---|
| `mcp_companies_house` | The server binary |
| `mcp_companies_house_data` | The tools, and the Companies House client in `mcp_companies_house_data::api` |
| `mcp_server_core` | Sign-in, tokens, sessions, TLS and the HTTP layer |
| `audit` | Audit lines for tools that change anything; none today |

Whatever `LOG_LEVEL` says:

- `audit` stays on at `info`.
- The MCP library's Streamable HTTP server and transport targets are held at `warn` at most, because they log MCP session ids at lower levels. A few of their warnings and errors, such as a session that could not be closed, can still name one.
- Directives scoped to spans (those containing `[`) are dropped, because they could turn a held target back up.

`LOG_LEVEL=info,mcp_companies_house_data::api=debug` logs the Companies House requests with their endpoint label, path, status and time, without turning on the rest of `DEBUG`. Prefer targets like this to a bare `debug` or `trace`, which also turns on the debug output of the other libraries the server uses.

### DEBUG

- **`DEBUG=true`** raises `mcp_companies_house`, `mcp_companies_house_data` and `mcp_server_core` to debug. That adds each tool's arguments (search terms and people's names included), the full message of each failed call, each Companies House request's path, status and time, and retries. It never logs API keys.
- **`DEBUG_UNSAFE_LOG_CONTENT=true`** turns on everything `DEBUG` does, and also logs every tool result and every Companies House response body. The server warns at startup: `DEBUG_UNSAFE_LOG_CONTENT is enabled: message content WILL be written to logs`. The register is public, but the logs then show what each person looked up and read. Use it briefly, and delete the logs afterwards. It never logs API keys either.

### What is never logged

The server's own code never logs:

- API keys, in plain or sealed form, the `Authorization` header or OAuth tokens, at any level.
- MCP session ids.
- Tool arguments, such as search terms, except at debug level, which `DEBUG` turns on.
- Tool results and Companies House response bodies, except with `DEBUG_UNSAFE_LOG_CONTENT`.
- The address of a Companies House request that failed on the network, which can carry search terms: such errors are logged without it.

Without `DEBUG`, a failed tool call is logged with the tool and the kind of error only. A few warnings include the path of the Companies House request that failed, which can contain a company number; for example `psc statements unavailable`, when the register of PSC statements could not be read.

### How people are named

Logs name a person as `key:` and 16 hex digits: a keyed hash (HMAC-SHA256) of their API key, under a key derived from `OAUTH_SIGNING_KEY`.

- The same key always gets the same name while `OAUTH_SIGNING_KEY` stays the same, so you can follow one person's sign-ins. Rotating the signing key gives everyone a new name.
- The name cannot be turned back into the key, or used to guess it, without the signing key.
- It appears in `oauth authorization granted` and as the subject (`sub`) of that person's tokens. Tool call lines do not name the caller, and the lines about rejected keys name the client's address, not a person.

## Metrics

`METRICS_ADDR` (for example `0.0.0.0:9090`) starts a separate plain-HTTP Prometheus listener, and the server logs `prometheus metrics listener started`. It has no authentication: keep it on an internal network. Durations are in seconds.

| Metric | Type | Labels | What it counts |
|---|---|---|---|
| `mcp_tool_calls_total` | counter | `tool`, `outcome` (`ok`, `error`) | Tool calls |
| `mcp_tool_duration_seconds` | histogram | `tool` | How long tool calls take |
| `companies_house_requests_total` | counter | `endpoint`, `status_class` (`2xx`, `3xx`, `4xx`, `5xx`, or `error` for no answer) | Requests to Companies House, each retry included. `endpoint` is a fixed label such as `company_profile`, `key_check` (the key check at sign-in or for a new bearer key), `document_metadata`, `document_content` or `document_storage`, never a company number |
| `companies_house_request_duration_seconds` | histogram | `endpoint` | How long Companies House requests take |
| `companies_house_rate_limited_total` | counter | `source` | Requests refused for a used-up rate limit: `local` when the server refused without calling Companies House, `upstream` when Companies House answered `429` |
| `companies_house_document_downloads_refused_total` | counter | | Document downloads refused because the server's 64 MiB [download budget](tools/documents.md#the-servers-download-budget) was in use |
| `oauth_requests_total` | counter | `endpoint` (`authorize`, `token`, `register`), `outcome` | OAuth requests. `authorize`: `ok`, `rate_limited` or `unavailable`. `token`: `ok` or the OAuth error code, such as `invalid_grant`. `register`: `ok` or `invalid` |
| `oauth_login_failures_total` | counter | | Keys rejected on the sign-in page |
| `bearer_credential_checks_total` | counter | `outcome` (`valid`, `invalid`, `unavailable`, `rate_limited`) | Checks of keys sent as bearer tokens. Answers from the [verdict cache](security.md#keys-as-bearer-tokens) are not counted |
| `http_requests_total` | counter | `route`, `status` | HTTP requests by route and status code |

Worth watching:

- `companies_house_requests_total` with `status_class` `5xx` or `error`: Companies House is failing or cannot be reached.
- `oauth_login_failures_total` and `bearer_credential_checks_total{outcome="invalid"}`: mistyped keys, or someone guessing.
- `oauth_requests_total{endpoint="authorize",outcome="rate_limited"}`: the sign-in limits are refusing people. If it keeps happening behind a proxy or tunnel, check `TRUSTED_PROXIES`.
- `companies_house_rate_limited_total`: people using up their own Companies House allowance.
- `companies_house_document_downloads_refused_total`: many large downloads at once.

## Traces

`OTEL_EXPORTER_OTLP_ENDPOINT` exports traces over OTLP/HTTP. Give the collector's base address, such as `http://otel-collector:4318`; the exporter sends traces to `/v1/traces` below it. The exporter reads the other standard variables itself, such as `OTEL_EXPORTER_OTLP_HEADERS` for an API key your collector needs, so set them directly rather than with `_FILE`. Traces carry the service name `mcp-companies-house` and the `request` and `tool` spans described under [Logs](#logs). Nothing is exported unless the variable is set.

## Upgrading

- **Docker:** pull the new image and recreate the container, for example `docker compose pull && docker compose up -d`. The container keeps no state.
- **Binary:** replace the file and restart.
- On `SIGTERM` or Ctrl-C the server stops accepting connections and lets open ones finish for up to 10 seconds.
- Sign-ins survive restarts and upgrades as long as `OAUTH_SIGNING_KEY` and `PUBLIC_URL` stay the same, unless the release notes say otherwise. Changing `PUBLIC_URL` signs everyone out, because tokens and client registrations name it.
- Open MCP sessions do not survive a restart: they live in memory. A client's next request gets `404 Session not found`, and the client starts a new session.
- The bearer key verdicts, the rate-limit windows and the sign-in limits start empty again.
- [Releases](releases.md) explains the versions and image tags, and every release lists its changes.

## Rotating `OAUTH_SIGNING_KEY`

1. Generate a new key with `openssl rand -hex 32` and put it where the server reads it, for example the file named by `OAUTH_SIGNING_KEY_FILE`.
2. Restart the server.

Then:

- Every OAuth sign-in ends: access and refresh tokens, authorization codes and sign-in forms still open all stop working, and people connect again.
- Client registrations end too. A connector that keeps its old registration gets `unknown client` from the sign-in page; remove it and add it again.
- Keys sent as bearer tokens, as Claude Code does with `--header`, keep working: they are not tokens.
- Everyone's name in the logs changes.
- With several instances, change the key on all of them together: a token signed by one is refused by another with a different key.

Rotate it at once if it may have leaked; see [Security](security.md#if-oauth_signing_key-leaks).

## Rate limits

### The Companies House rate limit

Companies House allows each API key 600 requests per 5 minutes. As an operator you see it like this:

- Each person's key has its own window. The server reads the `X-Ratelimit-*` headers on each response and remembers the latest window of each key, under the key's hash, for up to 10,000 keys at once.
- When a key's window is used up, that key's tools answer `rate_limited` at once with the seconds until the reset, without calling Companies House. The server logs `rate limit used up; not calling Companies House` and counts `companies_house_rate_limited_total{source="local"}`. A `429` from Companies House counts as `source="upstream"`.
- Other people are not affected.
- The check when someone signs in, or sends a new bearer key, uses one request of that person's allowance.
- A request is sent once more after a network error or a `5xx`. A `429` is never retried.

Companies House can ban applications that keep going over the limit. Each key belongs to the application of the person who created it. See [Rate limit](conventions.md#rate-limit) for what people see and how many requests each tool makes.

### Request rate limits

Apart from the limits on key checks (see [Security](security.md#limits-on-key-checks)), the server does not limit how many requests a client sends. To add limits, do it at a reverse proxy in front of it:

- Limit `/companies` and `/mcp` per bearer token (the `Authorization` header), not per client address. claude.ai reaches the server from a few shared addresses, so a limit per address there would let one busy person block everyone.
- Limit by address only on `/authorize`, the sign-in page a person's browser opens.
- Do not limit `/token` or `/register` per address either, for the same reason. They carry no `Authorization` header, so a per-token limit does not apply to them.

With nginx:

```nginx
limit_req_zone $http_authorization zone=per_token:10m rate=20r/s;
limit_req_zone $binary_remote_addr zone=sign_in:10m rate=10r/m;

server {
    # listen, server_name and certificates as in TLS and public access

    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_buffering off;

    location /authorize { limit_req zone=sign_in burst=10;   proxy_pass https://127.0.0.1:8443; }
    location /          { limit_req zone=per_token burst=40; proxy_pass https://127.0.0.1:8443; }
}
```

Set `TRUSTED_PROXIES` to the address the server sees nginx connect from: `127.0.0.1` when nginx and the server binary run on the same machine. The rates are examples; choose your own.

### Document downloads

Filed document downloads share a budget of 64 MiB at a time across everyone, described in [Filed documents](tools/documents.md#the-servers-download-budget). Downloads refused while it is spent are counted in `companies_house_document_downloads_refused_total`. Each instance has its own budget.

## Running several instances

Several instances can share the work, with one condition: each MCP session must stay on one instance.

- **Sign-in works across instances.** Tokens are signed JWTs that carry the sealed key, and nothing else is stored. Any instance with the same `OAUTH_SIGNING_KEY` and `PUBLIC_URL` accepts tokens issued by another. Keep their clocks in sync, because tokens carry the times they were issued and the time of the sign-in.
- **Sessions do not.** MCP sessions live in the memory of the instance that opened them. A request that reaches another instance gets `404 Session not found`, and the client has to start again. Route every request with a given `Mcp-Session-Id` header to the instance that issued it (sticky sessions), or run one instance.
- **Limits are per instance.** Each instance keeps its own sign-in limits, so with three instances a client can make three times the attempts. Each also keeps its own memory of used authorization codes, bearer key verdicts, session limits, rate-limit windows and download budget. An instance that has not yet seen a key's used-up window calls Companies House once, gets the `429` and then refuses locally like the others.

Restarting one instance ends only its own sessions.

## Backups and resources

- There is nothing to back up: no database, no files, no volume. Keep the configuration and `OAUTH_SIGNING_KEY` somewhere safe.
- Memory holds the open sessions, the sign-in limit tables, the bearer key verdicts, each key's rate-limit window, Companies House responses being read (up to 8 MiB each) and the documents being downloaded (up to 64 MiB at once).
- The container needs no writable filesystem.

Next: [Troubleshooting](troubleshooting.md).
