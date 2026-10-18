# mcp-companies-house

A self-hosted [Model Context Protocol](https://modelcontextprotocol.io) server for the UK [Companies House](https://developer.company-information.service.gov.uk) register. MCP clients such as claude.ai, Claude Desktop and Claude Code can search and filter the register, read company profiles, officers, owners, filings, charges and insolvency records, download filed accounts, and screen a company before you deal with it.

**Documentation: [trozz.github.io/mcp-companies-house](https://trozz.github.io/mcp-companies-house/)**

| Endpoint | What it does |
|---|---|
| `/companies` | 26 read-only tools and 8 prompts for the register: search and filter companies; profiles, officers and their other appointments, persons with significant control, filing history and filed documents, charges, insolvency, registers, exemptions and disqualifications; SIC code lookup; one-call company reports and due diligence screens |
| `/mcp` | Every enabled service's tools and prompts in one connector, named `<service>_<tool>`, for example `companies_search_companies` |

- Each person signs in with their own Companies House API key, so one server serves many people. The server holds no key of its own and stores none.
- A single static Rust binary, also shipped as a `FROM scratch` container image.
- TLS end to end, and a built-in OAuth 2.1 server so the server works as a claude.ai custom connector. No database.
- Read only: the public data API cannot change anything at Companies House.

## Quick start

1. Get a free REST API key: register at the [Companies House Developer Hub](https://developer.company-information.service.gov.uk/get-started/), create a **live** application and add a **REST** key to it. Everyone who uses the server needs their own.
2. Run the server:

   ```bash
   docker run -d --name mcp-companies-house -p 8443:8443 \
     -e PUBLIC_URL=https://companies-house.example.com \
     -e OAUTH_SIGNING_KEY="$(openssl rand -hex 32)" \
     -v /path/to/certs:/certs:ro \
     -e TLS_CERT_PATH=/certs/fullchain.pem \
     -e TLS_KEY_PATH=/certs/privkey.pem \
     ghcr.io/trozz/mcp-companies-house:latest
   ```

   Keep `OAUTH_SIGNING_KEY` stable (changing it signs everyone out) and in a secret store. For the sandbox, add `-e COMPANIES_HOUSE_API_URL=https://api-sandbox.company-information.service.gov.uk` and use keys from a test application.
3. Connect a client:
   - **claude.ai, Claude Desktop and mobile**: add a custom connector for `https://companies-house.example.com/mcp` and enter your API key on the sign-in page. claude.ai needs a publicly trusted certificate.
   - **Claude Code**: `claude mcp add --transport http companies-house https://companies-house.example.com/mcp --header "Authorization: Bearer $COMPANIES_HOUSE_API_KEY"`

See [getting started](https://trozz.github.io/mcp-companies-house/getting-started.html), [connecting clients](https://trozz.github.io/mcp-companies-house/connecting-clients.html), [TLS and public access](https://trozz.github.io/mcp-companies-house/tls.html) and the [configuration reference](https://trozz.github.io/mcp-companies-house/configuration.html).

## Using it

Ask in plain words; the model picks the tools. Give a company number when you have one (`00445790`, `SC123456`), and say which company you mean when several share a name.

| You want to | Ask, for example | What the model uses |
|---|---|---|
| Check a supplier or customer | "Screen ACME WIDGETS LIMITED before I pay their £5,000 deposit invoice" | `due_diligence_check`, `company_report`, disqualified officer searches |
| Get the facts on a company | "Give me an overview of 00445790 and what it has filed since January" | `company_report`, `list_filing_history` |
| Look into a director | "What other companies has Jane Anne Smith, born April 1980, been a director of?" | `search_officers`, `officer_network`, `search_disqualified_officers` |
| Trace who owns a company | "Who ultimately controls 00445790?" | `list_persons_with_significant_control`, followed up the chain |
| Read the latest accounts | "What were the net assets and cash in the latest accounts of 00445790?" | `list_filing_history`, `download_filing_document` |
| Find companies | "Active craft breweries in Leeds incorporated since 2022" | `lookup_sic_codes`, `filter_companies` |
| Check a name before registering | "Is Bright Lane Bakery Ltd available as a company name?" | `check_company_name_availability`, `browse_company_names` |
| Watch filing deadlines | "Which of these 12 companies have accounts overdue or due this month?" | `get_company` for each |

Each of these jobs is also a ready-made **prompt** that clients show as a slash command or in a menu: `overview`, `due_diligence`, `director_check`, `ownership_chain`, `read_accounts`, `find_companies`, `name_check` and `watchlist` (on `/mcp`, `companies_due_diligence` and so on). A prompt tells the model which tools to call, how many requests to spend and how to set out the answer.

Good to know:

- The register is public and unverified: companies file it themselves. Nothing here is a credit, sanctions, politically exposed person or identity check, and the due diligence tools never say a company is safe.
- Each key may make 600 Companies House requests per 5 minutes. When yours is used up, tools say how long to wait; other people are not affected.
- Officers' dates of birth are month and year only, as Companies House publishes them. Names of people are not unique.
- The sandbox serves only test companies you generate, and has no filed documents.

The documentation describes [what you can ask](https://trozz.github.io/mcp-companies-house/usage.html), [every prompt](https://trozz.github.io/mcp-companies-house/prompts.html) and every tool: [finding companies](https://trozz.github.io/mcp-companies-house/tools/finding.html), [reading a company](https://trozz.github.io/mcp-companies-house/tools/company.html), [filed documents](https://trozz.github.io/mcp-companies-house/tools/documents.html), [people and ownership](https://trozz.github.io/mcp-companies-house/tools/people.html) and [due diligence](https://trozz.github.io/mcp-companies-house/tools/due-diligence.html). [Security](https://trozz.github.io/mcp-companies-house/security.html) explains how keys are handled, and [operations](https://trozz.github.io/mcp-companies-house/operations.html) covers logs, metrics and limits.

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check
bash scripts/test-next-version.sh
bash scripts/build-docs.sh
```

The tests run against an in-memory fake of the Companies House API. See [Contributing](https://trozz.github.io/mcp-companies-house/contributing.html) for the live sandbox tests, the documentation tools and adding a tool, prompt or service, and [Releases](https://trozz.github.io/mcp-companies-house/releases.html) for how versions are published. The server is built on [mcp-server-core](https://github.com/Trozz/mcp-server-core-rs), which it shares with [mcp-icloud](https://github.com/Trozz/mcp-icloud).

## License

MIT. The SIC 2007 list in `crates/mcp-companies-house-data/data/sic2007.tsv` contains public sector information licensed under the Open Government Licence v3.0.
