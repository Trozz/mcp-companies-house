# mcp-companies-house

mcp-companies-house is a self-hosted [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server for the UK [Companies House](https://developer.company-information.service.gov.uk) register. It lets MCP clients such as claude.ai, Claude Desktop and Claude Code search and filter the register, read company profiles, officers, owners, filings, charges and insolvency records, download filed accounts and screen a company before you deal with it. Each person who uses it signs in with their own Companies House API key, so one server can serve many people.

One server runs every service. Each service is its own MCP endpoint, and a combined endpoint offers all of them in one connector:

| Endpoint | What it does |
|---|---|
| `/companies` | 26 tools and 8 [prompts](prompts.md) for the register: search and filter companies; profiles, officers and their other appointments, persons with significant control, filing history and filed documents, charges, insolvency, registers, exemptions and disqualifications; SIC code lookup; one-call company reports and due diligence screens |
| `/mcp` | Every enabled service's tools and prompts in one connector, each name prefixed with its service, for example `companies_search_companies`. See [Combined endpoint](combined.md) |

Today `companies` is the only service, so both endpoints offer the same tools. The Tools pages describe each one: [finding companies](tools/finding.md), [reading a company](tools/company.md), [filed documents](tools/documents.md), [people and ownership](tools/people.md) and [due diligence](tools/due-diligence.md). [Using it](usage.md) shows what you can ask.

## Highlights

- A single Rust binary, built static for Linux, also shipped as a `FROM scratch` container image (`ghcr.io/trozz/mcp-companies-house`).
- TLS end to end: the server terminates TLS itself, and every call to Companies House is made over verified HTTPS.
- Each person brings their own free Companies House REST API key. The server holds no key of its own and stores none.
- A built-in OAuth 2.1 authorization server, so each endpoint works as a claude.ai custom connector. No database and no volume are needed.
- Read only: every tool only reads, and the API it uses cannot change anything at Companies House.
- Composite tools and prompts for common jobs. [`company_report`](tools/company.md#company-report) and [`due_diligence_check`](tools/due-diligence.md) each gather in one tool call what would otherwise take several, using up to 7 Companies House requests. Eight prompts walk the model through jobs such as a due diligence screen or a director check.
- Built on [mcp-server-core](https://github.com/Trozz/mcp-server-core-rs), the open-source base it shares with [mcp-icloud](https://github.com/Trozz/mcp-icloud).

## How it talks to Companies House

The server uses the official Companies House APIs. Every request carries the API key of the person using the tool, sent as the HTTP Basic username with a blank password:

| Purpose | API | Host |
|---|---|---|
| Register data, live (default) | [Public Data API](https://developer-specs.company-information.service.gov.uk/companies-house-public-data-api/reference) | `api.company-information.service.gov.uk` |
| Register data, sandbox | Public Data API | `api-sandbox.company-information.service.gov.uk` |
| Filed documents, such as accounts | Document API | `document-api.company-information.service.gov.uk` (live only) |

- `COMPANIES_HOUSE_API_URL` chooses live or sandbox for the whole server. A key belongs to one environment: a key from a live application is rejected by the sandbox, and a key from a test application by the live API. Everyone using a server needs a key for its environment.
- Companies House has no sandbox Document API, so a server on the sandbox cannot download filed documents.
- Because each request uses the caller's own key, each person has their own rate limit of 600 requests per 5 minutes. One person using theirs up does not affect anyone else.
- The Document API answers a download with a redirect to its storage. The server fetches that one redirect without your key, and only over HTTPS. It follows no other redirects.
- SIC code lookups use a copy of the Companies House SIC 2007 condensed list (731 codes) inside the server, so they make no request at all.

## What it cannot do

- **Change anything.** The server cannot file accounts, confirmation statements or changes of officers, address or name. A filing service and a streaming service are only [plans](https://github.com/Trozz/mcp-companies-house/blob/develop/docs/plans/streaming-and-filing.md) and are not built.
- **Vouch for what is filed.** Companies House records what companies file but does not verify it. The server reports the register as it is.
- **Check sanctions, politically exposed persons or credit.** The register holds none of this. `due_diligence_check` lists these among the checks it cannot run, with litigation and trading performance. Where the register itself marks a person with significant control as sanctioned, the tools show it, but that is not a sanctions screen.
- **Find what a person owns.** People can be searched by their officer appointments only, not as persons with significant control. Personal bankruptcy is on the separate Individual Insolvency Register, which the server does not search.
- **List every shareholder.** The register of persons with significant control shows those with more than 25% of the shares or votes, or other significant control. Smaller holders appear, if at all, in confirmation statements and incorporation documents, which the server can download.
- **Reach past the first 1000 matches of a name search.** Companies House serves only the first 1000 matches of a search for companies, officers or disqualified officers by name. Narrow the query to reach the rest.

## Privacy

Your Companies House key is checked once when you sign in, with one search request to Companies House. It is then sealed (encrypted with ChaCha20-Poly1305) into your connection's OAuth tokens and opened again for each request. The server writes no key to disk or to its logs, and your client holds only the sealed form, which it cannot read. A key sent directly as a bearer token is checked on first use, and the result is remembered in memory for up to 10 minutes under the key's SHA-256 hash, never the key itself.

Logs record tool names, outcomes and durations, and for a failed call only the tool and the kind of error. Where they name a person, as when someone signs in, it is as `key:` and a short hash of their key. Whoever runs the server can turn on debug logging, which adds tool arguments such as search terms, or unsafe content logging, which also adds results. Neither ever logs an API key. Companies House itself receives every request made with your key, as with any use of its API. See [Security](security.md).

Next: [Getting started](getting-started.md).
