# Troubleshooting

This page lists the problems people run into with the server, from a server that will not start to tools that answer with errors, with the cause of each and how to fix it. Messages are quoted as the server writes them; `...` stands for the part that varies.

## The server does not start

The server checks the whole configuration first, prints every problem as `configuration error: ...` and exits with code 1. Fix them all and start it again.

| Message | Fix |
|---|---|
| `PUBLIC_URL is required` | Set `PUBLIC_URL` to the external address, such as `https://companies-house.example.com` |
| `PUBLIC_URL must be an https URL (http is only allowed for localhost)` | Use `https`. Plain `http` is accepted only for `localhost`, `127.0.0.1` and `[::1]` |
| `PUBLIC_URL must not contain a path` | Remove the path, such as `/mcp`: the server adds the endpoints itself |
| `PUBLIC_URL must not contain a query or fragment` | Remove everything after `?` or `#` |
| `OAUTH_SIGNING_KEY is required` | Generate one with `openssl rand -hex 32` and keep it |
| `OAUTH_SIGNING_KEY must be at least 32 bytes` | Use `openssl rand -hex 32`, which gives 64 characters |
| `MCP_AUTH_TOKEN cannot be used when people sign in with their own Companies House API key` | Unset `MCP_AUTH_TOKEN`. There is no shared token: clients that can send a header send their own key as the bearer token |
| `MCP_AUTH_TOKEN must not have the shape of a JWT (three dot-separated parts)` | Shown as well as the message above when the value has two dots. Unset `MCP_AUTH_TOKEN` |
| `COMPANIES_HOUSE_SERVICES contains an unknown service: ... (known: companies)` | `companies` is the only service. Unset the variable to run every service |
| `COMPANIES_HOUSE_SERVICES must name at least one service (known: companies)` | The value has no names in it, such as `,`. Name a service or unset the variable |
| `COMPANIES_HOUSE_READ_ONLY contains an unknown service: ... (known: all, companies)` | Use `companies` or `all`, or unset it: it has no effect today |
| `... must be true or false, got "..."` | Use `true` or `false` (or `1`, `0`, `yes`, `no`, `on`, `off`) |
| `COMPANIES_HOUSE_API_URL must use https (http is only allowed for loopback addresses)` | Use the `https` address. The same applies to `COMPANIES_HOUSE_DOCUMENT_API_URL` |
| `COMPANIES_HOUSE_API_URL is not a valid URL: ...`, `... must not contain credentials`, `... must not contain a query or fragment` | Give a plain address such as `https://api-sandbox.company-information.service.gov.uk`, without a user name, password, `?` or `#`. The same applies to `COMPANIES_HOUSE_DOCUMENT_API_URL` |
| `TLS_CERT_PATH and TLS_KEY_PATH must be set together` | Set both, or neither for a self-signed certificate |
| `TRUSTED_PROXIES contains an invalid IP address: ...` | List IP addresses only, comma separated. Host names and ranges such as `10.0.0.0/8` are not accepted |
| `BIND_ADDR has an invalid value` | Use `address:port`, such as `0.0.0.0:8443`. The same message for `METRICS_ADDR` means the same |
| `LOG_FORMAT has an invalid value` | Use `json` or `pretty` |
| `..._FILE: cannot read ...: ...` | The file is missing, or not readable by the user the server runs as (UID 65532 in the container) |

Some problems appear only once the server starts, as a line starting `error:`, and a wrong argument is refused before the configuration is read:

| Message | Fix |
|---|---|
| `error: cannot load TLS certificate ... / key ...: ...` | A certificate file is missing, unreadable by UID 65532 or not PEM. The server never falls back to a self-signed certificate once files are set |
| `error: ...` about the address | The listen address (`BIND_ADDR`) or the metrics address (`METRICS_ADDR`) is in use or not allowed |
| `error: ...` about the filter | `LOG_LEVEL` is not a valid `tracing` filter |
| `unknown argument: ...` | The binary takes only `--version`, `--help` or `--healthcheck`, or nothing. It exits with code 2 |

## claude.ai cannot connect

Adding the connector fails, or the sign-in page never opens.

| Cause | How to tell | Fix |
|---|---|---|
| The certificate is not publicly trusted | The startup log warns `... serving HTTPS with an in-memory self-signed certificate (not trusted by claude.ai)`, or `curl https://companies-house.example.com/healthz` from another machine fails without `-k` | Use a tunnel, a reverse proxy or certificate files from a public authority. See [TLS and public access](tls.md) |
| The server cannot be reached from the internet | The same `curl` cannot connect | Open the port, or use a tunnel. claude.ai connects from Anthropic's servers, not your machine |
| `PUBLIC_URL` is not the address claude.ai uses | `curl -s https://companies-house.example.com/.well-known/oauth-protected-resource/mcp` shows a different `resource` from the connector URL, or the sign-in ends with `invalid_target` and `resource is not a service on this server` | Set `PUBLIC_URL` to exactly the scheme, host and port in the connector URL. Metadata, resource URLs and token audiences are built from it |
| A proxy rewrites the `Host` header | Requests get `403 Forbidden: Host header is not allowed` | Pass `Host` through unchanged (`proxy_set_header Host $host;` in nginx) |
| A proxy buffers responses | Connecting hangs or times out | Turn buffering off (`proxy_buffering off;` in nginx) |
| The tunnel cannot reach the server | The tunnel's address answers with a tunnel error page | Point the tunnel's public hostname to `https://mcp-companies-house:8443` and turn on **No TLS Verify**, since the server's certificate is self-signed |
| `/mcp` is turned off | `/mcp` answers `404` | Unset `COMPANIES_HOUSE_COMBINED`, or connect to `/companies` |

## The sign-in page refuses

The page shows why. `429` and `503` answers carry `Retry-After: 30`.

| Message | Cause | Fix |
|---|---|---|
| `That is not a Companies House API key.` | The text cannot be a key: keys are 8 to 64 letters, digits, dashes and underscores. It was not sent to Companies House | Paste the key itself, from the application's page in the Developer Hub |
| `That Companies House API key was not accepted.` | Companies House rejected the key (HTTP 401): it is mistyped, revoked, a stream key, or for the other environment | Use a REST key from a **live** application on a live server, or from a **test** application on a sandbox server |
| `Companies House refused your API key from this server (HTTP 403). Check the key's IP or domain restrictions in the Developer Hub` | The key is restricted to addresses that do not include the server's | Add the address the server calls Companies House from, or remove the restriction |
| `Your Companies House API key could not be checked right now: this key's Companies House rate limit is used up; try again in ... seconds` | Your key's 600 requests per 5 minutes are used up | Wait as long as it says |
| `Your Companies House API key could not be checked right now: Companies House did not answer as expected (upstream)` | Companies House failed or could not be reached from the server | Try again later. If it persists, check that the server can reach the address in `COMPANIES_HOUSE_API_URL` over HTTPS |
| `Your Companies House API key could not be checked right now: the check timed out` | Companies House took more than 15 seconds | Try again |
| `Your Companies House API key could not be checked right now: the server is checking too many credentials at once` | 16 checks were already running and none finished within 5 seconds | Try again shortly |
| `Too many attempts from this address: credentials were rejected, or several are being checked at once. Wait a little and try again.` | 5 keys rejected or still being checked from your address in the last minute, 25 from your network, or many rejections across the server including one of yours | Wait a minute. Behind a proxy or tunnel, set `TRUSTED_PROXIES`, or everyone shares the proxy's address and its limit |
| `This server is refusing new addresses while it is under heavy load. Try again shortly.` | So many addresses are failing that the server refuses new ones (HTTP 503) | Wait. If it lasts, the server is under attack: limit `/authorize` at the network or a proxy |
| `The login form expired or was modified. Start the connection again.` | The page was open for more than 10 minutes, or was changed | Start connecting again from the client |
| `unknown client` | The client's registration is from before `OAUTH_SIGNING_KEY` or `PUBLIC_URL` changed | Remove the connector and add it again |
| `redirect_uri is not registered for this client` | The client sent a different return address from the one it registered | Remove the connector and add it again; if it persists, the client is at fault |

Some requests never show the page: the server sends the browser straight back to the client with an error, such as `invalid_target` (`resource is not a service on this server`) when the connector URL does not match `PUBLIC_URL`, or names `/mcp` while `/mcp` is turned off.

See [Security](security.md#limits-on-key-checks) for how the limits work.

## A connector stops working with 401

| Cause | Fix |
|---|---|
| The sign-in is 90 days old. Every sign-in ends then, however often it was refreshed | Connect again. The connector stays; it asks you to sign in |
| The client did not refresh for 30 days, so its refresh token expired | Connect again |
| `OAUTH_SIGNING_KEY` or `PUBLIC_URL` changed, which signs everyone out | Connect again; if the sign-in page says `unknown client`, remove the connector and add it again |
| The connector URL is not the endpoint it signed in to: a token for `/companies` is refused on `/mcp`, and the other way round | Remove the connector and add it again with the endpoint you want |

## A bearer key is refused

For clients that send the key in the `Authorization` header, such as Claude Code with `--header`.

| Answer | Cause | Fix |
|---|---|---|
| `401` on every request | Companies House rejected the key, or the value is not shaped like a key (for example with quotes or a `Bearer` prefix inside the variable). A rejection is remembered for 1 minute | Check the key, then replace it in the client, for example `claude mcp remove ch` and add it again |
| `503` with `Your Companies House API key could not be checked right now: ...` | Companies House could not be reached, your key's rate limit is used up, or the server is busy | Wait as the message says. `Retry-After` is 30 seconds |
| `429` with `Too many attempts from this address: credentials or tokens were rejected, or several are being checked at once. Try again shortly.` | Too many rejected keys from your address or network | Wait a minute and fix the key |

## Tools answer auth

| Message | Cause | Fix |
|---|---|---|
| `auth: Companies House rejected your API key. Sign in again with a REST API key (not a stream key) for the environment this server uses: keys from a test application only work with the sandbox` | The key was revoked or deleted since you signed in, or the server was switched between live and sandbox since then: sign-ins stay valid, but keys belong to one environment | Sign in again with a REST key for the server's environment |
| `auth: Companies House refused your API key from this server (HTTP 403). Check the key's IP or domain restrictions in the Developer Hub` | The key's IP or domain restrictions do not include the server's address, for example after the server moved | Update the restrictions in the Developer Hub |
| `auth: this request carried no Companies House API key; sign in again` | The request reached the tool without a key | Sign in again |

## Tools answer rate_limited

| Message | Cause | Fix |
|---|---|---|
| `rate_limited: the Companies House rate limit (600 requests per 5 minutes per API key) is used up; it resets in ... seconds` | Your own key's allowance is used up. Only you are affected | Wait as long as it says. Spread large jobs out; see [Rate limit](conventions.md#rate-limit) for what each tool costs |
| `rate_limited: the Companies House rate limit (600 requests per 5 minutes per API key) is used up; try again in a few minutes` | As above, but Companies House gave no reset time | Wait a few minutes |
| `rate_limited: the server is downloading too many documents at once; try again in a minute, or ask for a smaller max_bytes` | The server's 64 MiB budget for document downloads is in use by others | Try again shortly, or ask for a smaller `max_bytes` |

Companies House can ban applications that keep going over the limit, so do not retry in a loop.

## Documents are refused

| Message | Cause | Fix |
|---|---|---|
| `validation: filed documents cannot be downloaded on this server: Companies House's Document API has no sandbox, so documents are only available on a server using the live API` | The server uses the sandbox, or an API address other than the live one, and `COMPANIES_HOUSE_DOCUMENT_API_URL` is not set | Expected on the sandbox. With a proxy in front of the live API, set `COMPANIES_HOUSE_DOCUMENT_API_URL` |
| `validation: document ... is ... bytes as ..., more than max_bytes (...); raise max_bytes (up to 10485760) or choose another format: ...` | The document is larger than `max_bytes`, which defaults to 1 MiB | Ask for a larger `max_bytes`, up to 10 MiB, or another format from the list |
| `validation: the document is larger than max_bytes (...); raise max_bytes (up to 10485760) or use metadata_only to see its size` | The same, found while downloading because Companies House gave no size | As above |
| `validation: document ... is not held as ...; it is held as: ...` | Companies House does not hold the document in that format | Ask for one of the formats listed |
| `not_found: no document ...; take document_id from list_filing_history or get_filing` | The document id is wrong or unknown | Take `document_id` from `list_filing_history` or `get_filing` (see [Filed documents](tools/documents.md#finding-a-document)) |
| `upstream: Companies House error: GET ... redirected to a location that is not https; not following it` | The Document API redirected the download to an address that is not `https`, which the server does not follow | Try again; report it if it repeats |

Documents larger than 10 MiB cannot be downloaded through the server. See [Filed documents](tools/documents.md).

## Upstream errors

`upstream` means Companies House gave an answer the server did not expect, or could not be reached. The server has already sent the request a second time after a network error or a `5xx`.

| Message | Cause | Fix |
|---|---|---|
| `upstream: Companies House error: GET /company/.../filing-history returned HTTP 500` | On the sandbox, filing history and PSC statements usually answer HTTP 500, and officers sometimes do too | Expected on the sandbox. [`company_report`](tools/company.md#company-report) and [`due_diligence_check`](tools/due-diligence.md#checks_run-and-checks_not_run) list what they could not read and carry on. See [On the sandbox](usage.md#on-the-sandbox) |
| `upstream: Companies House error: GET ... returned HTTP ...` on the live API | Companies House is failing | Try again later |
| `upstream: Companies House error: GET ... timed out: ...` or `... failed: ...` | Companies House did not answer within 30 seconds, or could not be reached | Check the server's outbound network and DNS; try again |
| `upstream: Companies House error: GET ... returned invalid JSON: ...` or `... returned more than 8388608 bytes` | An unexpected answer, often from a proxy in front of the API | Check `COMPANIES_HOUSE_API_URL` and any proxy |

The sandbox also has an almost empty search index, so searches there find little: look companies up by number.

## Sessions are refused

| Answer | Cause | Fix |
|---|---|---|
| `503 This server has too many open sessions. Try again shortly.` | The endpoint holds 10,000 sessions and every key holds about the same share | Wait; idle sessions are cleared after 6 minutes. A key that already has sessions on that endpoint is never refused: its new session replaces its own least recently used one |
| `404 Session not found` | The session ended: it was idle, the server restarted, the key opened more than 50 sessions on that endpoint, the request reached another instance, or the session belongs to another key | Nothing to fix for a single request: the client starts a new session. If it happens all the time with several instances, route each session to one instance (see [Running several instances](operations.md#running-several-instances)) |

## Requests are refused with 403

| Message | Cause | Fix |
|---|---|---|
| `Forbidden: Host header is not allowed` | The request's `Host` is not the `PUBLIC_URL` host or a loopback name | Fix `PUBLIC_URL`, or make the proxy keep the original `Host` |
| `Forbidden: Origin header is not allowed` | A browser-based client on another site sent its `Origin` | Add that origin to `ALLOWED_ORIGINS` if you trust it |

## More detail

Set `DEBUG=true`, or `LOG_LEVEL=info,mcp_companies_house_data=debug` for the tools only, and reproduce the problem. Debug logs include tool arguments such as search terms, but never API keys. Avoid `DEBUG_UNSAFE_LOG_CONTENT` unless you need the response bodies (see [Operations](operations.md#debug)).

When reporting a problem on [GitHub](https://github.com/Trozz/mcp-companies-house), include the version (`mcp-companies-house --version`) and the relevant log lines, and check that they contain nothing private.
