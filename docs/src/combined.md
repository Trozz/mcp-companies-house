# Combined endpoint

The `/mcp` endpoint offers the tools and prompts of every service the server runs as one MCP server, so a single connector gives a client everything. This page covers the prefixed tool and prompt names, how prompts name tools there, how signing in to `/mcp` differs from signing in to a service's own endpoint, how to turn it off and how to choose between `/mcp` and `/companies`.

Today `companies` is the only service, so `/mcp` offers the same 26 tools and 8 prompts as `/companies`, each with `companies_` in front of its name. `/companies` keeps working unchanged alongside it. A streaming service and a filing service are [proposed](https://github.com/Trozz/mcp-companies-house/blob/develop/docs/plans/streaming-and-filing.md) but not built; any service the server runs appears on `/mcp` with its own prefix.

## Tool names

On `/mcp` every tool name is `<service>_<tool>`:

| Service | Own endpoint | Names on `/mcp`, for example |
|---|---|---|
| Companies | `/companies` | `companies_search_companies`, `companies_get_company`, `companies_due_diligence_check` |

- Descriptions, parameters and annotations are the same as on `/companies`, so the Tools pages describe every tool: [finding companies](tools/finding.md), [reading a company](tools/company.md), [filed documents](tools/documents.md), [people and ownership](tools/people.md) and [due diligence](tools/due-diligence.md).
- A call to `companies_get_company` runs `get_company` exactly as `/companies` would, with the same key, the same rate limit and the same result.
- A name without a known service prefix is refused with `tool not found`.
- The server's own text names tools without the prefix: tool descriptions, error messages such as `not_found: no company with number 12345678; find the number with search_companies`, and the service's instructions. The instructions on `/mcp` therefore start with this note, followed by the service's own instructions under `companies:`:

  ```text
  This server combines several services. Each tool and prompt name starts with its service: companies_. The notes below, tool descriptions and error messages name tools without that prefix.
  ```

- The longest name on `/mcp` is `companies_list_persons_with_significant_control`, 47 characters. Some clients limit the full name, including a prefix of their own, to 64 characters; see [Tool names and their length](connecting-clients.md#tool-names-and-their-length).

## Prompts

Every prompt is offered on `/mcp` with the same prefix as the tools:

| On `/companies` | On `/mcp` | Described in |
|---|---|---|
| `overview` | `companies_overview` | [Company overview](prompts.md#company-overview) |
| `due_diligence` | `companies_due_diligence` | [Due diligence screen](prompts.md#due-diligence-screen) |
| `director_check` | `companies_director_check` | [Director check](prompts.md#director-check) |
| `ownership_chain` | `companies_ownership_chain` | [Ownership chain](prompts.md#ownership-chain) |
| `read_accounts` | `companies_read_accounts` | [Read the latest accounts](prompts.md#read-the-latest-accounts) |
| `find_companies` | `companies_find_companies` | [Find companies](prompts.md#find-companies) |
| `name_check` | `companies_name_check` | [Company name check](prompts.md#company-name-check) |
| `watchlist` | `companies_watchlist` | [Check a list of companies](prompts.md#check-a-list-of-companies) |

A prompt tells the model which tools to call, so its text must name them as the endpoint lists them. On `/mcp` the server gives every tool name in the prompt's text the same prefix:

| Endpoint | Prompt | Its text says |
|---|---|---|
| `/companies` | `due_diligence` | `Call due_diligence_check ...` |
| `/mcp` | `companies_due_diligence` | `Call companies_due_diligence_check ...` |

Only whole tool names are changed. `get_company` inside `get_company_registers` is not a separate name, so it is left alone, and a name that already has the prefix is not prefixed twice. Arguments, limits and the fences around pasted text are the same on both endpoints (see [Arguments](prompts.md#arguments)).

## Sign-in

`/mcp` is an OAuth protected resource of its own, separate from `/companies`:

| Endpoint | Resource | Scope | Metadata |
|---|---|---|---|
| `/companies` | `https://companies-house.example.com/companies` | `companies` | `/.well-known/oauth-protected-resource/companies` |
| `/mcp` | `https://companies-house.example.com/mcp` | `mcp` | `/.well-known/oauth-protected-resource/mcp` |

- Access tokens are bound to the endpoint they were issued for. A token for `/mcp` is refused on `/companies`, and a token for `/companies` is refused on `/mcp`, with `401` and a `WWW-Authenticate` header naming the endpoint's metadata. A `/mcp` token reaches the tools of every service.
- The sign-in page says the client is asking for access to every service on the server, through `/mcp`, instead of naming one service.
- A client must name `/mcp` as the resource it wants when it signs in (the `resource` parameter, which claude.ai sends). A client that names no resource is signed in for `/companies`, never for `/mcp`, even though `companies` is the only service. Point such a client at `/companies`.
- People still sign in with their own Companies House API key, and the key is checked and sealed into the tokens in the same way. See [Security](security.md#keys-are-never-stored).
- A key sent directly as the bearer token is not bound to an endpoint: it works on `/mcp` and on `/companies`.
- Each endpoint keeps its own MCP sessions. The limit of 50 sessions per key applies to `/mcp` and `/companies` separately (see [Sessions](security.md#sessions)).

## Turning it off

`/mcp` is served whenever at least one service runs, and the startup log shows `combined endpoint enabled` with its URL and the number of tools. Set `COMPANIES_HOUSE_COMBINED=false` to turn it off. Then:

- `/mcp` and `/.well-known/oauth-protected-resource/mcp` answer `404`.
- A sign-in that names `/mcp` as its resource is sent back to the client with the error `invalid_target` and `resource is not a service on this server`.
- `/companies` is not affected.

## Choosing between /mcp and /companies

| | `/mcp` | `/companies` |
|---|---|---|
| Tools and prompts | Every service's, prefixed: `companies_get_company` | This service's, unprefixed: `get_company` |
| Clients that sign in without naming a resource | Not supported | Supported |
| Clients that limit tool name length | Names are 10 characters longer | Shorter names |
| When more services exist | One connector for all of them | One connector per service, which you can turn on and off separately in the client |

- Today both offer the same tools, so the choice is about names and clients. `/mcp` is the simplest if your client sends `resource`, as claude.ai does.
- Add one or the other, not both. With both, every tool and prompt appears twice and you sign in twice.

Next: [Configuration reference](configuration.md).
