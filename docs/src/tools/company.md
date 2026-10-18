# Reading a company

Endpoint `/companies` (or `companies_<tool>` under [`/mcp`](../combined.md)). These tools read one company's records by its company number: its profile, an overview in one call, its filing history, charges, insolvency, statutory registers, exemptions and UK establishments.

## Tools

| Tool | Kind | What it does | Key parameters |
|---|---|---|---|
| `get_company` | read | The company's profile: name, status and status detail, type, jurisdiction, incorporation and cessation dates, registered office, SIC codes with descriptions (`nature_of_business`), previous names, accounts and confirmation statement due dates and whether they are overdue, and `records_available` | `company_number` |
| `company_report` | read | An overview in one call: profile, current officers, current persons with significant control and PSC statements, recent filings, charges not yet satisfied and insolvency cases. Uses up to 7 requests | `company_number`, `filings` (0 to 50, default 10) |
| `list_filing_history` | read | Filings, newest first: date, form type, category, description key and values, `transaction_id` and `document_id` | `company_number`, `categories`, `limit` (1 to 100, default 20), `start_index` (default 0) |
| `get_filing` | read | One filing record, with its page count. This is the record, not the document | `company_number`, `transaction_id` |
| `list_charges` | read | Charges (mortgages and other secured lending): status, who is entitled to them, what they cover, and created, delivered and satisfied dates, with `satisfied_count` and `part_satisfied_count` | `company_number`, `limit` (1 to 100, default 20), `start_index` (default 0) |
| `get_charge` | read | One charge in full: classification, particulars, amount or obligations secured, persons entitled, the filings made about it and any insolvency cases | `company_number`, `charge_id` |
| `get_insolvency` | read | Insolvency status and each case with its dates and insolvency practitioners | `company_number` |
| `get_company_registers` | read | Where the company keeps its statutory registers, with the history of each move | `company_number` |
| `get_exemptions` | read | Exemptions from disclosure, with the periods they apply for and whether they still apply | `company_number` |
| `get_uk_establishments` | read | The UK establishments (branches) of an overseas company | `company_number` |

Company numbers are normalised before use: `445790` becomes `00445790` and `SC1234` becomes `SC001234` (see [Conventions and errors](../conventions.md#company-numbers)). An unknown number gives `not_found: no company with number ...; find the number with search_companies`, or a similar message naming the record.

Other tools read the same company:

- Officers and persons with significant control: [People and ownership](people.md).
- Filed documents such as accounts, by the `document_id` from `list_filing_history`: [Filed documents](documents.md).
- A screen that turns these records into flags and notes: [Due diligence](due-diligence.md).

## The profile

`get_company` returns the profile Companies House holds, with a few changes:

- `etag` and `links` are removed; `links` becomes `records_available` (below). `has_been_liquidated`, `has_charges`, `has_insolvency_history` and `is_community_interest_company` are removed too, as Companies House has deprecated them in favour of `links` and `subtype`.
- Empty values are left out, so a missing field means Companies House holds no value for it.
- Fields Companies House adds later are passed through unchanged.

Two fields are added:

| Field | Meaning |
|---|---|
| `records_available` | The other records Companies House holds for the company, such as `officers`, `filing_history`, `charges` or `insolvency`. `company_report` and [`due_diligence_check`](due-diligence.md) read charges and insolvency only when they are listed here |
| `nature_of_business` | Each of the company's SIC codes with its description from the built-in [SIC 2007 list](finding.md#sic-codes). A code not on the list is shown without a description |

The deadlines are in `accounts` (for example `next_accounts` with `due_on` and `overdue`) and `confirmation_statement` (`next_due` and `overdue`), as Companies House gives them. The profile also carries `previous_company_names`, `registered_office_is_in_dispute` and `undeliverable_registered_office_address` when Companies House gives them. The [`watchlist` prompt](../prompts.md#check-a-list-of-companies) reads these for a list of companies.

## Company report

`company_report` gathers an overview of a company for due diligence in one call:

| Section | Contents |
|---|---|
| `company` | The profile, as `get_company` returns it |
| `officers` | `active_count`, `resigned_count`, and `active`: the officers not resigned, among the first 100 officer records |
| `persons_with_significant_control` | `active_count`, `ceased_count`, `active` (the current persons with significant control among the first 100), `statements` (the current PSC statements, when there are any), and `statements_unavailable` when the statements could not be read |
| `recent_filings` | The newest `filings` filings (default 10), as `list_filing_history` shows them. Left out with `filings: 0` |
| `charges` | `total`, `satisfied_count`, `part_satisfied_count` and `not_satisfied`: the charges among the first 100 that are not satisfied. `{"total": 0}` when the profile lists no charges |
| `insolvency` | `status` and `cases`, as `get_insolvency` returns them. `{"cases": []}` when the profile lists no insolvency |
| `unavailable` | Only when a section could not be read: each such section with its error, such as `"officers": "upstream: ..."` |

- The officers, persons with significant control and charges sections have `truncated: true` when the company has more than 100 such records. The `list_` tools give the full lists.
- The profile is read first. If the company does not exist, the call fails with `not_found`. After that, a section that fails does not fail the call: it is listed under `unavailable` and the rest is returned.
- The call uses up to 7 Companies House requests: the profile, officers, persons with significant control, PSC statements, filing history (not with `filings: 0`), charges (only when `records_available` lists them) and insolvency (only when it lists it).

`company_report` sets out records. `due_diligence_check` reads mostly the same records and says what a reviewer should look into; see [Due diligence](due-diligence.md#how-it-differs-from-company_report).

## Filing history

`list_filing_history` lists filings newest first. Each filing has:

| Field | Meaning |
|---|---|
| `date` | The filing's date |
| `type` | The form type, such as `AA` or `CS01` |
| `category`, `subcategory` | Companies House's grouping of the filing |
| `description` | The Companies House description key, such as `accounts-with-accounts-type-micro-entity`. This is a key, not the wording shown on the Companies House website |
| `description_values` | The values the key refers to, such as `made_up_date` |
| `action_date`, `pages`, `paper_filed`, `resolutions`, `annotations`, `associated_filings` | When Companies House gives them |
| `transaction_id` | The filing's id, for `get_filing` |
| `document_id` | The filed document's id, for [`download_filing_document`](documents.md) |

`categories` limits the list to some categories, for example `["accounts"]` or `["officers", "persons-with-significant-control"]`. Categories include `accounts`, `address`, `annual-return`, `capital`, `change-of-name`, `confirmation-statement`, `incorporation`, `insolvency`, `liquidation`, `miscellaneous`, `mortgage` (charges), `officers`, `persons-with-significant-control` and `resolution`.

Category names are letters and hyphens, up to 40 characters each; case is ignored, a name given twice is sent once, and up to 20 can be given. Anything else is refused: `validation: categories must be Companies House category names such as accounts or officers; got ...`.

`get_filing` returns one filing with the same fields, by the `transaction_id` from `list_filing_history`. It is the record of the filing; pass its `document_id` to `download_filing_document` for the document itself.

## Charges

`list_charges` lists the charges registered against a company. Each has:

- `charge_id` (for `get_charge`), `charge_code` and `charge_number`;
- `status`, such as `outstanding`, `part-satisfied`, `satisfied` or `fully-satisfied`;
- `classification`, the charge's description;
- `created_on`, `delivered_on` and `satisfied_on`;
- `persons_entitled`, the names of those entitled to the charge (usually the lender), and `more_than_four_persons_entitled` when Companies House gives it;
- `particulars`, what is charged, such as a description and whether it contains a fixed or floating charge.

The list has `total`, `satisfied_count` and `part_satisfied_count` for all the company's charges. A company without charges gives an empty list with `total: 0`.

`get_charge` returns one charge in full, as Companies House holds it with `charge_id` first: classification, particulars, amount or obligations secured, persons entitled, the filings made about the charge and any insolvency cases.

## Insolvency

`get_insolvency` returns the company's insolvency `status` and its `cases`. Each case has:

- `type`, such as `creditors-voluntary-liquidation` or `in-administration`;
- `number`;
- `dates`, from the kind of date to the date, such as `{"wound-up-on": "2025-02-01"}`;
- `practitioners`, each with `name`, `role`, `appointed_on`, `ceased_to_act_on` and `address`;
- `notes`, when there are any.

A company without an insolvency record gives `{"cases": [], "note": "Companies House holds no insolvency record for this company"}`.

## Registers

A company keeps statutory registers: directors, secretaries, persons with significant control, members, and directors' residential addresses. `get_company_registers` says where each is kept. Each entry in `registers` has:

| Field | Meaning |
|---|---|
| `register` | Which register, as Companies House names it, such as `directors` or `members` |
| `location` | Where it is kept now |
| `since` | When it moved there |
| `history` | Every move, newest first, each with `location`, `moved_on` and the `transaction_id` of the filing that recorded it |

`location` is one of:

| Location | Meaning |
|---|---|
| `public-register` | The register is held at Companies House, on the public register |
| `registered-office` | At the company's registered office |
| `single-alternative-inspection-location` | At a single alternative inspection location |
| `unspecified-location` | No location is given |

Most companies have told Companies House nothing about their registers, which means they keep them at their registered office. Such a company gives `{"registers": [], "note": "Companies House holds no register information for this company, which usually means it keeps its registers at its registered office"}`.

## Exemptions

`get_exemptions` lists a company's exemptions from disclosure, for example from the persons with significant control rules because its shares trade on a regulated market. Each entry in `exemptions` has:

- `exemption`, the Companies House code, such as `psc-exempt-as-trading-on-regulated-market`;
- `periods`, each with `exempt_from` and, when it ended, `exempt_to`;
- `current`, `true` when a period has no end date, so the exemption still applies.

Most companies have none, and get `{"exemptions": [], "note": "Companies House holds no exemptions for this company"}`. `list_persons_with_significant_control` and `due_diligence_check` read exemptions themselves to explain a company that lists no owners; see [People and ownership](people.md#when-no-one-is-listed).

## UK establishments

The UK establishments of an overseas company (its number starts with `FC`) are its branches, each with its own company number starting with `BR`. `get_uk_establishments` returns `total` and `items`, each with `company_number`, `company_name`, `company_status` and `locality`. Other companies have none, and get an empty list.
