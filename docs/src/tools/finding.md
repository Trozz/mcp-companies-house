# Finding companies

Endpoint `/companies` (or `companies_<tool>` under [`/mcp`](../combined.md)). These tools find companies by name, number, filters or kind of business, and find the SIC codes to filter by. They return the company numbers and ids that the other tools take.

## Tools

| Tool | Kind | What it does | Key parameters |
|---|---|---|---|
| `search_companies` | read | Search the register by company name or number, best match first. Each match has its number, name, status, type, incorporation and cessation dates and registered office address | `query`, `active_only`, `limit` (1 to 100, default 20), `start_index` (default 0) |
| `search_all` | read | One search across companies, officers and disqualified officers, best match first. Each result has a `kind` and the id the other tools take | `query`, `limit` (1 to 100, default 20), `start_index` (default 0) |
| `filter_companies` | read | The Companies House advanced search: words in the name, status, type, subtype, SIC codes, registered office location, and incorporation or dissolution date ranges. `total` counts every match | the [filters](#filters), `limit` (1 to 100, default 20), `start_index` (default 0) |
| `search_dissolved_companies` | read | Search dissolved companies by their last name, or by a name they had before | `query`, `match_on` (`best_match`, the default, or `previous_names`), `limit` (1 to 100, default 20), `start_index` (default 0) |
| `check_company_name_availability` | read | Whether a proposed name is already taken: the active companies whose names Companies House treats as the same | `name` |
| `browse_company_names` | read | Companies in register (alphabetical) order around a name, with number, name, status and type | `name`, `limit` (1 to 100, default 20) |
| `lookup_sic_codes` | read | Find SIC 2007 codes by words or by the first digits of a code, or describe codes you have. Does not call Companies House | `query`, `codes`, `limit` (1 to 100, default 20) |

Text arguments such as `query` and `name` are trimmed and limited to 200 characters. Lists share one shape and are paged with `start_index`; see [Conventions and errors](../conventions.md#lists-and-paging).

## Searching by name or number

`search_companies` takes a name (`rolls royce`) or a number (`00445790`). Each match has `company_number`, `company_name`, `company_status`, `company_type`, `date_of_creation`, `date_of_cessation` when the company has ended, a one-line `address` and, when Companies House sends one, a `snippet`. Without `active_only`, dissolved companies are listed too; `active_only: true` lists only companies that are currently active. Use [`get_company`](company.md#the-profile) for the full record.

`search_all` runs one Companies House search over the whole register. Each result has `kind`:

| `kind` | Fields | Take to |
|---|---|---|
| `company` | as `search_companies` | `get_company` and the other company tools |
| `officer` | `name`, `officer_id`, `appointment_count`, `date_of_birth` (month and year), `address`, `description` | [`get_officer_appointments`](people.md#appointments), [`officer_network`](people.md#officer-network) |
| `disqualified_officer` | `name`, `officer_id`, `officer_kind`, `date_of_birth`, `address`, `description` | [`get_disqualified_officer`](people.md#disqualified-officers) |

The three kinds share one ranked list, so a common name can fill the page with one kind. `search_companies`, `search_officers` and `search_disqualified_officers` give more results of one kind. The people searches are described in [People and ownership](people.md).

## Filters

`filter_companies` uses the Companies House advanced search. Every filter is optional, but give at least one; without one the tool answers `validation: give at least one filter: name_includes, name_excludes, status, company_type, subtype, sic_codes, location or a date`.

| Filter | Matches |
|---|---|
| `name_includes` | Words the company name must contain |
| `name_excludes` | Words the company name must not contain |
| `status` | A list of [status codes](#status-type-and-subtype-codes), such as `["active"]` |
| `company_type` | A list of [type codes](#status-type-and-subtype-codes), such as `["ltd", "llp"]` |
| `subtype` | A list of [subtype codes](#status-type-and-subtype-codes) |
| `sic_codes` | SIC 2007 codes of 4 or 5 digits, up to 50, such as `["62012", "62020"]`. Find them with `lookup_sic_codes` |
| `location` | Text matched against the registered office address: a town, county or postcode |
| `incorporated_from`, `incorporated_to` | Incorporated on or after, and on or before, a date as `YYYY-MM-DD` |
| `dissolved_from`, `dissolved_to` | Dissolved on or after, and on or before, a date as `YYYY-MM-DD` |

- Filters combine with AND: a company must match every filter you give.
- Values within one filter combine with OR: `status: ["active", "liquidation"]` matches either, and a company matches `sic_codes` if it has any of the codes.
- A value given twice is sent once. A `_from` date after its `_to` date is refused (`incorporated_from must not be after incorporated_to`), and so is a date that is not a real `YYYY-MM-DD` date.
- Each result has `company_number`, `company_name`, `company_status`, `company_type`, `company_subtype`, `date_of_creation`, `date_of_cessation`, a one-line `address` and `sic_codes`.
- When nothing matches, the result is an empty list with `total: 0`, not an error.

### Counting

`total` is the number of companies that match all the filters, not the number on the page. To count companies without listing them, ask for one:

```json
{"status": ["active"], "sic_codes": ["11050"], "location": "Leeds", "incorporated_from": "2022-01-01", "limit": 1}
```

`11050` is Manufacture of beer. The `total` of this call is the number of active companies with that code, a registered office address matching `Leeds` and an incorporation date on or after 1 January 2022, for one request.

### Status, type and subtype codes

`status`, `company_type` and `subtype` take the codes exactly as Companies House spells them in its [API enumerations](https://github.com/companieshouse/api-enumerations/blob/master/constants.yml). The tool's schema lists every accepted value, so a client can offer them as a choice. A value outside the list, or one in the wrong case (`Ltd` for `ltd`), is refused before the tool runs, so nothing is sent to Companies House; see [Conventions and errors](../conventions.md#errors).

| Filter | Accepted codes |
|---|---|
| `status` | `active`, `dissolved`, `open`, `closed`, `converted-closed`, `receivership`, `administration`, `liquidation`, `insolvency-proceedings`, `voluntary-arrangement`, `registered`, `removed` |
| `company_type` | `private-unlimited`, `ltd`, `plc`, `old-public-company`, `private-limited-guarant-nsc-limited-exemption`, `limited-partnership`, `private-limited-guarant-nsc`, `converted-or-closed`, `private-unlimited-nsc`, `private-limited-shares-section-30-exemption`, `protected-cell-company`, `assurance-company`, `oversea-company`, `eeig`, `icvc-securities`, `icvc-warrant`, `icvc-umbrella`, `registered-society-non-jurisdictional`, `industrial-and-provident-society`, `northern-ireland`, `northern-ireland-other`, `royal-charter`, `investment-company-with-variable-capital`, `unregistered-company`, `llp`, `other`, `european-public-limited-liability-company-se`, `uk-establishment`, `scottish-partnership`, `charitable-incorporated-organisation`, `scottish-charitable-incorporated-organisation`, `further-education-or-sixth-form-college-corporation`, `registered-overseas-entity` |
| `subtype` | `community-interest-company`, `private-fund-limited-partnership` |

## Dissolved companies

`search_dissolved_companies` searches only companies that have been dissolved. With `match_on: best_match` (the default) it matches the name the company had when it ended; with `match_on: previous_names` it matches the names it had before, which helps when you know a company by an old name.

Each result has `company_number`, `company_name`, `company_status`, `date_of_creation`, `date_of_cessation`, the last registered office as `address`, `previous_company_names` (each with `name`, `effective_from` and `ceased_on`) and, for a previous-name search, `matched_previous_name`. `total` counts every match.

## Name availability

`check_company_name_availability` asks Companies House for the active companies whose names it treats as the same as the proposed name, for example ignoring `Limited` or `Ltd` and punctuation. It returns:

| Field | Meaning |
|---|---|
| `name` | The name checked |
| `available` | `true` when no active company has a name treated as the same |
| `conflicting_companies` | Those companies (up to 100), with the fields of `search_companies` |
| `note` | What the check does not cover |

It does not check:

- names that are not the same but are too like an existing name, which Companies House can still refuse;
- sensitive words, which Companies House can also refuse;
- companies that are not active: only active companies are compared.

`available: true` therefore does not mean Companies House will accept the name. `browse_company_names` shows the names around it in register order. The [`name_check` prompt](../prompts.md#company-name-check) runs both, together with `filter_companies`, for up to five names.

## Browsing names

`browse_company_names` lists companies in register (alphabetical) order around a name, for example to see similarly named companies or every company starting with a word. It returns `count` and `items`, each with `company_number`, `company_name`, `company_status` and `company_type`. It lists at most 100 and has no `start_index`.

## SIC codes

`lookup_sic_codes` works from a copy of Companies House's [SIC 2007 condensed list](https://www.gov.uk/government/publications/standard-industrial-classification-of-economic-activities-sic) built into the server: 731 codes, including the Companies House codes `74990` (Non-trading company) and `99999` (Dormant Company). It makes no Companies House request, so it does not count against your rate limit. The same list gives the descriptions in `nature_of_business` on `get_company`.

Give `query`, `codes` or both:

- `query` with words, such as `software development` or `letting property`, returns `matches`: each with `code`, `description` and `section` (the SIC 2007 section letter, A to U), best first.
- `query` with digits, such as `620`, returns the codes that start with them.
- `codes`, such as `["62012", "99999"]`, returns each code with its `description`. A code not on the list has `description: null` and a note saying so. Codes are 4 or 5 digits, up to 50.

Word matching uses the official wording:

- Every word must match the start of a word in a description. Words of three letters or fewer match only whole words (`tax` does not find `taxi`); longer words also match by their first few letters, so `accountant` finds `accounting`.
- Small words such as `and`, `of` and `the` are ignored.
- Some everyday words are mapped to the official ones: `bakery` looks for `bread`, `dentist` for `dental`, `vet` for `veterinary`, `garage` for `motor`, `landlord` for `letting` and `renting`, `online` for `internet`, among others.
- When no code matches every word, the codes that match some of the words are returned, those that match the most first.

If nothing matches, try a broader or more formal word. Use the codes with `filter_companies`.

Contains public sector information licensed under the Open Government Licence v3.0.

## The 1000-result window

`search_companies`, `search_all`, `search_officers` and `search_disqualified_officers` use the Companies House `/search` endpoints, which serve only the first 1000 matches of a query.

- `start_index` must be below 1000. A `start_index` of 1000 or more is refused before any request: `validation: Companies House search returns only the first 1000 matches, so start_index must be below 1000; narrow the query instead`.
- When a query has more than 1000 matches, the result has a `note` saying that only the first 1000 can be reached, and `next_start_index` is left out once it would reach 1000. Narrow the query, or use `filter_companies` for companies.
- These endpoints serve pages that start at a multiple of the page size. A `start_index` that is not a multiple of `limit` returns the rest of its page, so `count` can be smaller than `limit`; `next_start_index` continues from there. Passing `next_start_index` back as `start_index` walks the results in order.
- `limit: 1` reads a page of two and returns the result asked for, because Companies House skips the first match when asked for pages of one.
- Near result 1000 the page is shortened so that it ends at result 1000, which Companies House would otherwise refuse.

`filter_companies` and `search_dissolved_companies` use other Companies House searches and pass `limit` and `start_index` through as given. `check_company_name_availability` reads one page of up to 100.
