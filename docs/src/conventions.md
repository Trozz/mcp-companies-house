# Conventions and errors

This page covers what every tool shares: how companies and records are addressed, the formats of codes and dates, how lists are paged, what the errors mean, and how the Companies House rate limit applies to your key. The tool pages refer back to it.

## Company numbers

Companies are addressed by their company number, 8 characters such as `00445790` or `SC123456`. Find a number from a name with [`search_companies`](tools/finding.md#searching-by-name-or-number).

Tools tidy the number you give before using it: spaces are removed, letters are made upper case, and a number made of digits only, or of two letters followed by digits, is padded with zeros after the letters to 8 characters. Other numbers are used as given.

| You give | Used |
|---|---|
| `445790` | `00445790` |
| `0044 5790` | `00445790` |
| `sc123456` | `SC123456` |
| `SC1234` | `SC001234` |
| `FC12` | `FC000012` |
| `oc301234` | `OC301234` |
| `IP28746R` | `IP28746R` |
| `R0000123` | `R0000123` |

Anything longer than 8 characters, or with characters other than letters and digits, is refused with `validation: company_number must be up to 8 letters and digits, e.g. 00445790 or SC123456`.

## Ids

Other records are addressed by the ids that the list and search tools return.

| Id | From | Taken by |
|---|---|---|
| `officer_id` | [`list_officers`](tools/people.md#officers), [`search_officers`](tools/people.md#searching-people), [`search_all`](tools/finding.md#searching-by-name-or-number) | [`get_officer_appointments`](tools/people.md#appointments), [`officer_network`](tools/people.md#officer-network) |
| `officer_id` and `officer_kind` of a disqualified officer | [`search_disqualified_officers`](tools/people.md#disqualified-officers), `search_all` | [`get_disqualified_officer`](tools/people.md#disqualified-officers) |
| `charge_id` | [`list_charges`](tools/company.md#charges) | [`get_charge`](tools/company.md#charges) |
| `transaction_id` | [`list_filing_history`](tools/company.md#filing-history), and the history in [`get_company_registers`](tools/company.md#registers) | [`get_filing`](tools/company.md#filing-history) |
| `document_id` | `list_filing_history`, `get_filing` | [`download_filing_document`](tools/documents.md) |
| `appointment_id`, `psc_id`, `statement_id` | `list_officers`, [`list_persons_with_significant_control`](tools/people.md#persons-with-significant-control) | No tool: they identify the record at Companies House |

Pass ids exactly as they were returned. An id is letters, digits, `-` and `_`, up to 100 characters; anything else is refused with `validation: <name> must be the identifier exactly as a list or search tool returned it`. `download_filing_document` also takes a document link; see [Filed documents](tools/documents.md#finding-a-document).

## Codes

- Company status, type and subtype are Companies House's own codes, such as `active`, `liquidation`, `ltd`, `llp` and `community-interest-company`. The `filter_companies` schema lists every accepted value; see [Finding companies](tools/finding.md#status-type-and-subtype-codes).
- Other values are also passed on as Companies House's codes, not translated: officer roles (`director`, `llp-designated-member`), natures of control (`ownership-of-shares-75-to-100-percent`), PSC statements (`psc-details-not-confirmed`), filing description keys (`accounts-with-accounts-type-micro-entity`), insolvency case types (`creditors-voluntary-liquidation`) and exemptions (`psc-exempt-as-trading-on-regulated-market`). Companies House's [API enumerations](https://github.com/companieshouse/api-enumerations) list them.
- SIC codes are the 4 or 5 digit SIC 2007 codes, such as `62012`. [`lookup_sic_codes`](tools/finding.md#sic-codes) finds and describes them.

## Dates

- Dates in arguments and results are `YYYY-MM-DD`. A date argument that is not a real date is refused, for example `validation: incorporated_from must be a date as YYYY-MM-DD, got "01/02/2020"`.
- Date ranges include both ends: `_from` means on or after, `_to` on or before. A `_from` date after its `_to` date is refused.
- Officers' and owners' dates of birth are month and year only, `YYYY-MM` (or the year alone), as Companies House publishes them. Disqualified officers' dates of birth are shown as Companies House gives them, which can be a full date.
- [`due_diligence_check`](tools/due-diligence.md) counts ages and "the last 12 months" back from `checked_on`, the date in UTC when it ran.

## Text arguments

Free-text arguments such as `query`, `name`, `location` and `officer_name` are trimmed and limited to 200 characters (`validation: query is limited to 200 characters`). A required one must not be blank (`validation: query must not be empty`).

## Results

Every tool returns one JSON object as text. `download_filing_document` adds the document itself as a second part; see [Filed documents](tools/documents.md#the-result).

Within each record, empty values are left out: a `null`, an empty string, or an empty list or object from Companies House does not appear. `false` and `0` are kept. The lists a tool returns, such as `items`, `registers` or `cases`, are always there, even when empty. One value is kept as `null`: `lookup_sic_codes` gives `description: null` for a code not on its list.

## Lists and paging

List results share one shape:

| Field | Meaning |
|---|---|
| `total` | The number of matches or records, when Companies House gives it |
| `start_index` | The index of the first item on this page, from 0 |
| `count` | The number of items on this page |
| `next_start_index` | Only when more results exist: pass it as `start_index` for the next page |
| `items` | The results |

- `limit` sets the page size, 1 to 100, default 20. Outside that range the tool answers `validation: limit must be between 1 and 100; use start_index to page through more results`.
- `start_index` starts from 0, the default.
- Some lists add totals of their own: `active_count`, `resigned_count` and `inactive_count` for officers, `active_count` and `ceased_count` for persons with significant control, `satisfied_count` and `part_satisfied_count` for charges.
- `total` is left out when it is not known, for example `list_officers` with `role`. [`browse_company_names`](tools/finding.md#browsing-names) returns only `count` and `items`, and [`get_uk_establishments`](tools/company.md#uk-establishments) `total` and `items`.

Companies House answers some lists for an unknown company number with an empty list. When the first page of a company's officers, persons with significant control, charges or UK establishments is empty, or its insolvency, exemptions or registers record is missing, the tool checks with one more request that the company exists, and answers `not_found` if it does not. An empty answer from these tools therefore means the company exists and has no such records.

## Search window

`search_companies`, `search_all`, `search_officers` and `search_disqualified_officers` reach only the first 1000 matches of a query, and their pages are aligned to `limit`. A `start_index` of 1000 or more is refused, and `note` says when a query has more matches than can be reached. See [The 1000-result window](tools/finding.md#the-1000-result-window).

If Companies House itself refuses a range, the tool answers `validation: Companies House refused the requested range (HTTP 416); search results stop at the first 1000 matches, so narrow the query`.

## Errors

A tool that fails returns a result marked as an error, whose text starts with a kind and a colon:

```text
not_found: no company with number 12345678; find the number with search_companies
```

| Kind | Meaning | What to do |
|---|---|---|
| `validation` | An argument is wrong, and the message says which. Also when Companies House rejects the request (HTTP 400 or 422, with its reason), cannot provide a document in the format asked for (406), or refuses a search range (416) | Change the argument as the message says |
| `not_found` | There is no company, filing, charge, officer, document or disqualified officer with that number or id | Check the number or id, or find it with a search or list tool |
| `auth` | Companies House rejected or refused your key, or the request carried none | See the messages below |
| `rate_limited` | Your key's [rate limit](#rate-limit) is used up, or the server is [downloading too many documents](tools/documents.md#the-servers-download-budget) at once | Wait as long as the message says |
| `upstream` | Companies House gave an answer the server did not expect, or could not be reached: another HTTP status, a redirect, a body that is not JSON or is larger than 8 MiB, a timeout or a network failure | Usually temporary: try again later |
| `internal` | An unexpected error inside the server | Check the server log, and report it if it repeats |

The `auth` messages say what to do:

| Cause | Message |
|---|---|
| HTTP 401 | `Companies House rejected your API key. Sign in again with a REST API key (not a stream key) for the environment this server uses: keys from a test application only work with the sandbox` |
| HTTP 403 | `Companies House refused your API key from this server (HTTP 403). Check the key's IP or domain restrictions in the Developer Hub` |
| No key | `this request carried no Companies House API key; sign in again` |

An argument the tool's schema does not allow, such as a status code that is not on the list, a missing required argument or a value of the wrong type, is refused before the tool runs, so nothing is sent to Companies House. That result is also marked as an error, but its text starts `failed to deserialize parameters:` rather than with a kind.

[`company_report`](tools/company.md#company-report) and `due_diligence_check` keep going when a record other than the profile cannot be read: the error appears inside the result (under `unavailable`, or in `checks_not_run`) in the same `kind: message` form.

## Rate limit

Companies House allows 600 requests per 5 minutes for each API key, and answers HTTP 429 for the rest of the window once a key has used them up. Applications that keep going over the limit can be banned. Everyone signs in with their own key, so everyone has their own limit: one person using theirs up does not affect anyone else.

- The server reads the `X-Ratelimit-Limit`, `X-Ratelimit-Remain` and `X-Ratelimit-Reset` headers on each response and remembers the window for each key.
- When your key's window has no requests left and has not yet reset, the server does not call Companies House for you. Tools answer at once with the seconds until the reset: `rate_limited: the Companies House rate limit (600 requests per 5 minutes per API key) is used up; it resets in 42 seconds`.
- A 429 answer is not retried. Its reset time comes from those headers or, without them, from `Retry-After`. When no reset time is known, the message ends `try again in a few minutes`.
- A request is sent once more, after a short pause, only after a network error (including a timeout) or a `5xx` answer. The server waits up to 10 seconds to connect and 30 seconds for each request.

Most tools make one request. These make more, before any retries:

| Tool | Companies House requests |
|---|---|
| `lookup_sic_codes` | None |
| A company list or record tool that finds nothing | One more, to check that the company exists |
| `list_persons_with_significant_control` | Two on the first page (the PSCs and the statements), one on later pages, and one more for the exemptions when no current PSC or statement is listed |
| `list_officers` with `current_only` or `role` | Up to 5 pages of 100, and one more if Companies House refuses its active filter |
| `get_disqualified_officer` without `officer_kind` | Up to 2 |
| `officer_network` | Up to 10 pages of 100, and one search when given `officer_name` |
| `company_report` | Up to 7 |
| `due_diligence_check` | Up to 7 |
| `download_filing_document` | Two to the Document API (one with `metadata_only`), and the download from document storage |

The [prompts](prompts.md) tell the model how many tool calls a job may use.

## Filed text is data

Company names, addresses, officers' names, filing descriptions and the text of filed documents are written by whoever filed them, and Companies House does not verify them. The server's instructions tell the model to treat them as data, not as instructions, and every prompt says the same. If something in a result reads like an instruction, it is text someone filed, not a message from the server.
