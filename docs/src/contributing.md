# Contributing

This page is for working on the code: the commands that build and check it, how the tests are laid out around a fake Companies House, the optional tests against the Companies House sandbox, how to add a tool, a prompt or a service, how to change the shared core, and how to build this documentation. Branches, version numbers and releases are on [Releases](releases.md).

## Build and test

`rust-toolchain.toml` pins Rust 1.99.1, with rustfmt and clippy. These are the checks CI runs:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check
bash scripts/test-next-version.sh
bash scripts/build-docs.sh
```

| Command | What it checks |
|---|---|
| `cargo test --workspace` | The unit tests and the integration tests of both crates. None of them calls Companies House: they use an in-memory fake or a closed local port, and the live sandbox tests are ignored unless you ask for them |
| `cargo clippy ...` | Lints, with every warning treated as an error |
| `cargo fmt --all --check` | Formatting |
| `cargo deny check` | The rules in `deny.toml`: security advisories, the allowed licences, banned crates (OpenSSL, native-tls and aws-lc) and crates.io as the only source of crates. It needs cargo-deny installed |
| `bash scripts/test-next-version.sh` | The version rules in `scripts/next-version.sh`, in a throwaway git repository |
| `bash scripts/build-docs.sh` | Builds this documentation and checks every internal link and anchor. See [This documentation](#this-documentation) for the tools it needs |

CI also runs shellcheck on `scripts/*.sh`, checks the source-branch script, and builds the container image for `linux/amd64` and `linux/arm64` without pushing it. It runs clippy and the tests with `--locked`, so `Cargo.lock` must be up to date.

## Tests

| Where | What it covers |
|---|---|
| `#[cfg(test)]` modules in `src/` | Configuration, error kinds and messages, company number normalisation, URLs, rate-limit headers, the key check's verdicts, the SIC search, the shaping of results, prompt rendering, and the server's configuration errors |
| `crates/mcp-companies-house-data/tests/support/` | The fake Companies House (`fake_ch.rs`, with canned records in `fake_records.rs`) and helpers for calling tools (`tools.rs`) |
| `tests/api_client.rs` | `ChClient` against the fake: Basic auth, the retry, error mapping and rate-limit tracking |
| `tests/tools_read.rs`, `tools_search_extras.rs`, `tools_people.rs`, `tools_records.rs`, `tools_documents.rs`, `tools_report.rs` | The tools, called directly, against the fake |
| `tests/http_e2e.rs` (data crate) | The service over HTTP, behind the core's bearer guard and combined endpoint, against the fake |
| `tests/live_sandbox.rs` | Smoke tests against the real sandbox, ignored by default. See [Live sandbox tests](#live-sandbox-tests) |
| `crates/mcp-companies-house/tests/http_e2e.rs` | The whole server built from environment variables, as `main` builds it: OAuth tokens bound to their path, the key as the bearer token, prefixes and prompts on `/mcp`, icons and OAuth metadata. `COMPANIES_HOUSE_API_URL` points at a closed local port, so nothing reaches the network |

### The fake API

`FakeCh::start(FakeState::register())` starts the fake on a random port of `127.0.0.1`, with six made-up companies and their records. [Architecture](architecture.md#the-fake-companies-house) describes what it serves. The helpers in `support/tools.rs` point a client at it:

| Helper | What it gives |
|---|---|
| `server(&fake)` | A `CompaniesServer` whose client calls the fake, for the API and the Document API, with no delay before the retry |
| `client(&fake)` | Just the `ChClient` |
| `auth()` | Request details carrying the test key, as the bearer guard would add it |
| `auth_with(key)`, `no_auth()` | The same with another key, or with none |
| `params(json!(...))` | Tool arguments from JSON |
| `json(&result)`, `error_text(&result)`, `is_error(&result)` | A successful result as JSON, or a tool error's text |

A tool test calls the tool method directly and checks the result:

```rust
#[tokio::test]
async fn get_company_describes_its_sic_codes() {
    let fake = FakeCh::start(FakeState::register()).await;
    let v = json(
        &server(&fake)
            .get_company(params(json!({"company_number": "2"})), auth())
            .await
            .unwrap(),
    );
    assert_eq!(v["sic_codes"], json!(["10710", "47240"]));
}
```

- `fake.requests()` lists every request with its path, query, `Authorization` and `Accept` headers.
- `fake.inject(Injected::status(500))` queues a canned response for the next request that passes the fake's key check, and `Injected::json(...)` and `.header(...)` build one with a body or headers.
- `fake.state()` gives the fake's state, to make a path always fail (`fail`), slow every response (`delay`) or change the rate-limit counters.
- A tool that reads a new kind of resource needs a fixture: add it to `fixtures()` in `fake_records.rs`, keyed by its request path and shaped like the live API's answer.

The test key is the placeholder `test-api-key`. `.gitguardian.yaml` marks it, and its Basic header, as test-only. Never put a real key in a test, a fixture or these pages.

### Live sandbox tests

`tests/live_sandbox.rs` runs against the Companies House sandbox. The tests are ignored by default and need a REST key from a **test** application in `COMPANIES_HOUSE_API_KEY`. Only these tests read that variable; the server never does.

```bash
COMPANIES_HOUSE_API_KEY=<your-sandbox-api-key> \
  cargo test -p mcp-companies-house-data --test live_sandbox -- --ignored --nocapture
```

- They run only against the sandbox. `COMPANIES_HOUSE_API_URL` may point them elsewhere, but only at a host whose name contains `sandbox`; any other address fails the test before anything is created.
- Three of the tests create a test company with the [test data generator](https://developer.company-information.service.gov.uk/api-testing) and always delete it again with its authentication code.

| Test | What it does |
|---|---|
| `get_company_round_trip_with_a_generated_company` | Reads a new company by its full and its short number, deletes it, then checks it is `not_found` |
| `company_report_on_a_generated_company` | Checks that `company_report` answers, with each section either present or listed under `unavailable` |
| `due_diligence_and_records_on_a_generated_company` | Runs `due_diligence_check`, the registers, exemptions and UK establishments tools and current officers, then `officer_network` and `get_filing` when the sandbox returned an officer and a filing, and checks that `download_filing_document` says there is no sandbox Document API |
| `searches_answer_in_the_expected_shape` | Checks the shape of `search_companies` and `filter_companies` results |
| `the_sign_in_check_accepts_a_real_key_and_rejects_a_wrong_one` | Runs the sign-in check with your key and with a key of zeros |
| `a_wrong_key_is_reported_as_auth` | Checks that a wrong key gives an `auth` error. It needs network access but no key |

What the sandbox can and cannot do shapes these tests:

- It serves the profiles, officers and PSCs of generated companies, but their filing history and PSC statements usually answer HTTP 500, and officers sometimes fail too.
- Its search index is almost empty, so the search tests check the shape of the answer, not its content.
- It has no Document API, so no filed document can be downloaded.

The tests therefore check shapes and graceful failure, and print what the sandbox answered for anything it does not serve reliably.

## Adding a tool

Tools live in `crates/mcp-companies-house-data/src/tools/`, one module per group. Each module has its own `#[tool_router]` block, and `CompaniesServer::new` in `tools/mod.rs` adds them together; a new module needs its router added there.

1. **Parameters.** Write a struct that derives `Deserialize` and `JsonSchema` (with `#[schemars(crate = "rmcp::schemars")]`). Each field's doc comment becomes its description in the tool's input schema, which is what the model reads. Use the enums in `codes.rs` for Companies House codes, so the schema lists every accepted value.
2. **Description and annotations.** Put the tool's description in its `#[tool(description = "...", annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true))]` attribute. Every tool must be marked `read_only_hint = true`: the core treats a tool without it as one that changes something, so it would be hidden and refused on a read-only service and its calls recorded in the audit log. `open_world_hint` is `false` only for a tool that calls nothing outside the server, as `lookup_sic_codes` does.
3. **Body.** Take `Parameters(p): Parameters<...>` and `Extension(parts): Extension<Parts>`, and wrap the body in `run("tool_name", async { ... })`. `run` adds the tracing span and the metrics, and turns a `ChError` into a tool error that starts with its kind. Get the caller with `self.caller(&parts)?`, check every argument with the helpers in `tools/mod.rs` (`normalise_company_number`, `id_arg`, `text_arg`, `required_text`, `limit_arg`, `date_arg`, `date_range`) before making any request, and shape the response with the functions in `view/`.
4. **Instructions.** If the model needs to know when to use the tool, mention it in `INSTRUCTIONS` in `tools/mod.rs`, which the server sends when a client connects.
5. **Tests.** Test the tool against the fake. Then update the places that pin the list of tools: the sorted names in `builds_from_env` in `src/service.rs`, and the count of 26 in `crates/mcp-companies-house/src/lib.rs`, `crates/mcp-companies-house/tests/http_e2e.rs` and `crates/mcp-companies-house-data/tests/http_e2e.rs`.
6. **Documentation.** Describe the tool on its page under `docs/src/tools/`, and update the tool count where the documentation and the README give it.

Existing tests then check the new tool too:

- every tool is listed as read-only, and every tool except `lookup_sic_codes` as open world (a new tool that works offline must be added to that check);
- every name on `/mcp` is unique, at most 64 characters with its prefix, and made of letters, digits, `_` and `-`;
- `/mcp` lists every tool with exactly the service's description, schema and annotations;
- every tool a prompt names exists (`prompts_name_only_tools_that_exist`).

## Adding a prompt

1. Write the template as `crates/mcp-companies-house-data/prompts/<name>.txt`. `{argument}` is replaced with the argument's value, and `[[if argument: ...]]` keeps its text only when that argument is given. Name tools by their plain names, such as `get_company`; on `/mcp` the core adds the prefix.
2. Add a `PromptSpec` to `PROMPTS` in `src/prompts.rs`, with its name, title, description, arguments (each with a name, a description and whether it is required) and `template: include_str!("../prompts/<name>.txt")`.
3. Put text that may come from someone else inside a fence tag, such as `<context>` and `</context>`. A value cannot open or close the tags listed in `FENCES` in `prompts.rs`, so add any new tag there.
4. Add the prompt's name, in order, to the list in `crates/mcp-companies-house/tests/http_e2e.rs`, and raise the count of 8 there.
5. Describe it in [Prompts](prompts.md), and in [Using it](usage.md) if it suits a common job.

The prompt tests then check every prompt:

- It renders both with all its arguments and with only the required ones, each argument appears only when it is given, no `[[`, `]]`, `{` or `}` is left over, and there is never more than one blank line in a row. A template therefore cannot contain a literal brace.
- Any word in a template that starts like a tool name (`get_`, `list_`, `search_`, `filter_`, `browse_`, `check_`, `lookup_` or `download_`, or that ends in `_check`) must be the name of a real tool.
- A missing required argument, an unknown argument and a value over 2000 characters are refused when the prompt is rendered.

## Adding a service

Each service is a library crate that implements the `Service` trait from [mcp-server-core](https://github.com/Trozz/mcp-server-core-rs) (`mcp_server_core::service::Service`): a name, which is its path, OAuth resource and scope, and its rmcp handler, plus optionally its tool names for the startup log, its own router and how people sign in. [Architecture](architecture.md#the-service-trait) shows how the companies service implements it.

1. Create `crates/mcp-companies-house-<name>` as a library. Read its own variables in `pub fn from_env(env: &mut ServiceEnv) -> Option<Arc<dyn Service>>`, reporting problems through `env.errors`. Never read the server variables; the `reads_no_server_variables` test in the data service's `config.rs` shows how to check that.
2. Return `DynHandler::new(MyServer::new(...))` from `handler()`. Any rmcp `ServerHandler` fits. The default `router()` serves it at `/<name>`, and the combined endpoint serves it at `/mcp`. Mark tools that only read with `read_only_hint = true`. Tools read the caller's key from the request: take `Extension(parts): Extension<http::request::Parts>` and get `mcp_server_core::credential::Credential` from `parts.extensions`.
3. Add the crate to `crates/mcp-companies-house/Cargo.toml` and one line to `SERVICES` in `crates/mcp-companies-house/src/lib.rs`.
4. Document its tools on a new page under `docs/src/`, listed in `docs/src/SUMMARY.md`, and its variables in the [configuration reference](configuration.md).

Service names are lowercase letters, digits and `-`. `mcp` is reserved, and a name cannot contain `_`, which the combined endpoint uses to end the prefix.

Routing, OAuth metadata, token audiences, `COMPANIES_HOUSE_SERVICES` and the release pipeline pick the new service up without further changes. The server has one sign-in: exactly one enabled service may define `credential_verifier`, and a second one stops the server at startup. A service that uses the same Companies House REST key should not define its own. A service that needs a different credential, such as a stream key, needs a change to the core first.

### Planned services

[`docs/plans/streaming-and-filing.md`](https://github.com/Trozz/mcp-companies-house/blob/develop/docs/plans/streaming-and-filing.md) proposes two more services. Neither is built yet.

| Service | What it would do | Status in the plan |
|---|---|---|
| `/streaming` | Read the Companies House Streaming API: a feed of changes across the whole register. It needs a stream key from a live application | Recommended first, as one bounded `read_stream` tool that keeps no state |
| `/filing` | File a registered office address change and a change of registered email address through the Companies House filing API, with each person signing in at Companies House itself | A second phase, after a short check that sandbox sign-in works |

The plan also proposes that new services run only when `COMPANIES_HOUSE_SERVICES` names them, so the default stays `companies`, and notes that a streaming service would need either a server-wide stream key or a change to the core's single sign-in.

## The shared core

OAuth, sign-in, sessions, limits, TLS, routing, logging and metrics come from [mcp-server-core](https://github.com/Trozz/mcp-server-core-rs), which mcp-icloud uses too. It is published on crates.io, with API documentation on [docs.rs](https://docs.rs/mcp-server-core), and the workspace `Cargo.toml` requires version 0.1.

- **Changing it.** Fix the core in its own repository and release it, then raise the version in the workspace `Cargo.toml` here and update `Cargo.lock`.
- **Only crates.io.** `deny.toml` allows crates from crates.io only, so a git dependency on the core fails `cargo deny check`. A path dependency on a local checkout, such as `mcp-server-core = { path = "../mcp-server-core-rs" }`, works while you develop both, but cannot be merged, because CI has no such checkout.
- **rmcp.** The workspace pins rmcp to an exact version (`=3.5.1`), within the core's own requirement (`~3.5.1`), so the tools and the core share one rmcp. A core release that needs a newer rmcp means raising this pin with it.

## Pull requests

- Branch from `develop` as `feature/<name>` or `bugfix/<name>`, and open the pull request against `develop`.
- Write conventional commit messages (`feat:`, `fix:`, `perf:`, `refactor:`, `docs:`, `test:`, `chore:`). Release notes are built from them; see [Releases](releases.md#release-notes).
- CI runs on every pull request into `develop` or `main`: fmt, clippy, tests, cargo-deny, the Docker build, the documentation build and the script checks.

## This documentation

This site is built with [mdBook](https://rust-lang.github.io/mdBook/) from `docs/`: `docs/book.toml`, and the pages under `docs/src/`, listed in `docs/src/SUMMARY.md`. Diagrams are Mermaid, drawn by the mdbook-mermaid preprocessor. The plans in `docs/plans/` are not part of the site.

`scripts/install-docs-tools.sh` installs the pinned tools, mdbook 0.5.4, mdbook-mermaid 0.17.1 and lychee 0.24.2, into the directory you give it. It downloads the Linux x86_64 builds, as CI does, and checks their SHA-256 checksums. On another system, install the same versions another way.

```bash
bash scripts/install-docs-tools.sh "$HOME/.local/bin"
export PATH="$HOME/.local/bin:$PATH"
bash scripts/build-docs.sh
mdbook serve docs --open
```

- `scripts/build-docs.sh` installs Mermaid's files into `docs/`, builds the book into `docs/book` (both are ignored by git), then checks every internal link and anchor with lychee, offline. A broken link fails the build, and so fails CI.
- A page listed in `SUMMARY.md` that does not exist also fails the build: `book.toml` sets `create-missing = false`.
- Link between pages with relative `.md` links, such as `[Security](security.md)`. An anchor is the heading in lower case, with punctuation dropped and spaces turned into hyphens, so `## Rate limit` is `#rate-limit`.
- No test compares these pages with the code. A change to a tool, a prompt, a variable or a limit should update its page in the same pull request.

The site is published from `main`; see [Releases](releases.md#the-documentation-site).
