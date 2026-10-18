# Configuration reference

Everything is configured with environment variables. This page lists every variable the server reads, with whether it is required, its default and what it does, and the settings that stop the server from starting. The server checks the whole configuration at startup. If anything is wrong, it prints every problem it found, each as `configuration error: ...`, and exits, so it never runs half configured.

Only two variables are required: `PUBLIC_URL` and `OAUTH_SIGNING_KEY`. There is no Companies House key in the configuration, no admin password and no shared token: everyone signs in with their own Companies House API key.

## General rules

- An empty value counts as unset.
- Every variable can also be read from a file by appending `_FILE` to its name, for example `OAUTH_SIGNING_KEY_FILE=/run/secrets/oauth_signing_key`. Use this for secrets with Docker or Kubernetes secrets.
  - A value set directly wins over the file.
  - A trailing newline in the file is ignored.
  - A file that cannot be read stops startup with `<NAME>_FILE: cannot read <path>: <reason>`. In the container the file must be readable by UID 65532.
  - The `OTEL_*` variables are the exception: the OpenTelemetry exporter reads them itself, so set them directly.
- Booleans accept `true`, `false`, `1`, `0`, `yes`, `no`, `on` and `off`, in any mix of upper and lower case. Anything else stops startup with `<NAME> must be true or false, got "<value>"`.
- Lists are comma separated. Spaces around entries and empty entries are ignored.

## Server

| Variable | Required | Default | Example | Description |
|---|---|---|---|---|
| `PUBLIC_URL` | yes | | `https://companies-house.example.com` | The external address clients use. Must be `https`; `http` is accepted only for `localhost`, `127.0.0.1` and `[::1]`. A port is allowed; a path, query or fragment is not (a trailing `/` is fine). OAuth metadata, resource URLs, token audiences and the self-signed certificate's name are built from it, so it must match the address clients use exactly |
| `OAUTH_SIGNING_KEY` | yes | | output of `openssl rand -hex 32` | At least 32 bytes. Signs every OAuth token and client registration, and is the source of the key that seals people's API keys into their tokens. Changing it signs everyone out. Keep it secret: see [Security](security.md#if-oauth_signing_key-leaks) |
| `BIND_ADDR` | no | `0.0.0.0:8443` | `127.0.0.1:8443` | Listen address, as `address:port`. It must listen on loopback (`0.0.0.0` does) so `--healthcheck` can reach `127.0.0.1` |
| `TLS_CERT_PATH`, `TLS_KEY_PATH` | no | | `/certs/fullchain.pem`, `/certs/privkey.pem` | PEM certificate chain and private key. Set both or neither. The server checks them for changes every 30 seconds and reloads them. Without them it serves a self-signed certificate. See [TLS and public access](tls.md) |
| `INSECURE_HTTP` | no | `false` | `true` | Serve plain HTTP without TLS. Only when TLS is terminated in front of the server on a network you trust. It wins over `TLS_CERT_PATH` and `TLS_KEY_PATH` |
| `TRUSTED_PROXIES` | no | | `172.30.0.10,10.0.0.3` | IP addresses of reverse proxies or tunnels whose `X-Forwarded-For` header is believed for the limits on key checks, at sign-in and for bearer keys. Addresses only, not host names or ranges. See [Security](security.md#limits-on-key-checks) |
| `ALLOWED_ORIGINS` | no | | `https://app.example.com` | Extra browser `Origin` values accepted on `/companies` and `/mcp`. The `PUBLIC_URL` origin is always accepted, and requests without an `Origin` header are not affected |
| `COMPANIES_HOUSE_SERVICES` | no | every service (`companies`) | `companies` | Services to run. An unknown name, or a value with no names in it such as `,`, stops startup. A service that is not selected is not mounted and does not read its variables. `companies` is the only service today |
| `COMPANIES_HOUSE_COMBINED` | no | `true` | `false` | Serve the [combined endpoint](combined.md) `/mcp`. With `false`, `/mcp` answers `404` |
| `COMPANIES_HOUSE_READ_ONLY` | no | none | `all` | Services, or `all`, whose tools that change anything are hidden and refused. Accepts `companies` and `all`; an unknown name stops startup. No effect today: see [Variables with no effect today](#variables-with-no-effect-today) |
| `AUDIT_LOG_PATH` | no | | `/audit/audit.jsonl` | File to append one JSON line to for every call of a tool that changes anything. No effect today: see [Variables with no effect today](#variables-with-no-effect-today) |
| `LOG_LEVEL` | no | `info` | `info,mcp_companies_house_data::api=debug` | Log filter in `tracing` syntax. See [Operations](operations.md#levels) for the targets |
| `LOG_FORMAT` | no | `json` | `pretty` | `json` (one object per line) or `pretty` (for people reading a terminal) |
| `DEBUG` | no | `false` | `true` | Debug logging for the server's own code: tool arguments such as search terms, full error messages, and the path, status and time of each Companies House request. Never API keys |
| `DEBUG_UNSAFE_LOG_CONTENT` | no | `false` | `true` | Everything `DEBUG` logs, plus every tool result and every Companies House response body. The server prints a warning at startup. Never API keys. See [Operations](operations.md#debug) |
| `METRICS_ADDR` | no | | `0.0.0.0:9090` | Start a separate plain-HTTP Prometheus listener on this `address:port`. Keep it on an internal network. See [Operations](operations.md#metrics) |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | no | | `http://otel-collector:4318` | Export traces over OTLP/HTTP to this collector. The exporter's standard variables, such as `OTEL_EXPORTER_OTLP_HEADERS`, apply too. Nothing is exported unless it is set |

## Companies

| Variable | Required | Default | Example | Description |
|---|---|---|---|---|
| `COMPANIES_HOUSE_API_URL` | no | `https://api.company-information.service.gov.uk` | `https://api-sandbox.company-information.service.gov.uk` | The Companies House API, and so the environment everyone's keys must belong to: keys from a live application work only with the live API, and keys from a test application only with the sandbox. Must be `https`, or `http` only to a loopback address for testing, with no user name, password, query or fragment. A path is kept, for a proxy in front of the API |
| `COMPANIES_HOUSE_DOCUMENT_API_URL` | no | the live Document API (`https://document-api.company-information.service.gov.uk`) when `COMPANIES_HOUSE_API_URL` is the live API; none otherwise | `https://proxy.example.com/documents` | Where `download_filing_document` fetches filed documents. Companies House has no sandbox Document API, so with the sandbox, or any other API URL such as a proxy, downloads are off unless this is set. Same rules as `COMPANIES_HOUSE_API_URL`. See [Filed documents](tools/documents.md#live-api-only) |

The startup line `companies house settings` shows the API and the Document API in use (`none` when downloads are off).

## Settings that stop startup

Besides a missing required variable and a value that cannot be read, the server refuses to start with:

- **`MCP_AUTH_TOKEN` set to anything.** Other servers built on the same core accept a shared bearer token for clients that cannot sign in. This one does not, because everyone brings their own key: `MCP_AUTH_TOKEN cannot be used when people sign in with their own Companies House API key`. Unset it; clients that can send a header send their own key as the bearer token instead (see [Connecting clients](connecting-clients.md#claude-code)).
- **A `PUBLIC_URL` that is not `https`**, other than `http` for `localhost`, `127.0.0.1` or `[::1]`, or one with a path, query or fragment.
- **An `OAUTH_SIGNING_KEY` shorter than 32 bytes.**
- **Only one of `TLS_CERT_PATH` and `TLS_KEY_PATH`**, unless `INSECURE_HTTP` is on.
- **An unknown service** in `COMPANIES_HOUSE_SERVICES` or `COMPANIES_HOUSE_READ_ONLY`, or a `COMPANIES_HOUSE_SERVICES` value with no names in it.
- **A Companies House URL** that is not `https` (other than `http` to a loopback address) or contains a user name, password, query or fragment.
- **A value that does not parse**: a boolean that is not one of the words above, a `TRUSTED_PROXIES` entry that is not an IP address, a `BIND_ADDR` or `METRICS_ADDR` that is not `address:port`, or a `LOG_FORMAT` other than `json` or `pretty`.

After the configuration is read, the server can still stop with an `error: ...` line, for example when the certificate files cannot be loaded or the listen address is in use. [Troubleshooting](troubleshooting.md#the-server-does-not-start) lists the messages and their fixes.

## Variables with no effect today

| Variable | Why |
|---|---|
| `COMPANIES_HOUSE_READ_ONLY` | It hides and refuses tools that change anything. Every tool on this server only reads, so there is nothing to hide. The value is still checked, so an unknown service name stops startup |
| `AUDIT_LOG_PATH` | Audit lines record calls of tools that change anything. There are none, so nothing is written and the file is never created |
| `OAUTH_ADMIN_PASSWORD` | Not read. There is no admin password: people sign in with their own key |
| `COMPANIES_HOUSE_API_KEY` | Not read by the server. Only the live sandbox tests use it (see [Contributing](contributing.md)) |

## A complete example

A server on the live register with its own certificate, metrics and secrets in files:

```bash
PUBLIC_URL=https://companies-house.example.com
OAUTH_SIGNING_KEY_FILE=/run/secrets/oauth_signing_key
TLS_CERT_PATH=/certs/fullchain.pem
TLS_KEY_PATH=/certs/privkey.pem
METRICS_ADDR=0.0.0.0:9090
LOG_FORMAT=json
```

The same server on the sandbox, behind a tunnel or reverse proxy at `172.30.0.10` that holds the public certificate:

```bash
PUBLIC_URL=https://companies-house.example.com
OAUTH_SIGNING_KEY_FILE=/run/secrets/oauth_signing_key
COMPANIES_HOUSE_API_URL=https://api-sandbox.company-information.service.gov.uk
TRUSTED_PROXIES=172.30.0.10
```

Without certificate files the server uses its self-signed certificate, which the tunnel or proxy must accept (see [TLS and public access](tls.md)).

Next: [TLS and public access](tls.md).
