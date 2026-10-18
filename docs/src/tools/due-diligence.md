# Due diligence

Endpoint `/companies` (or `companies_<tool>` under [`/mcp`](../combined.md)). `due_diligence_check` screens one company against the public register and returns observations for a reviewer to examine. This page lists every check it runs, what makes an observation a flag or a note, what it cannot check, and how it differs from [`company_report`](company.md#company-report).

It never concludes that a company is sound. It says what it found, what it checked and what it could not check; the decision stays with you.

## Tool

| Tool | Kind | What it does | Key parameters |
|---|---|---|---|
| `due_diligence_check` | read | Screens a company: status, insolvency, charges, overdue accounts and confirmation statements, registered office problems, officers in post and recent changes, ownership, company age, recent name changes and identity verification statements. Each observation is a flag or a note. Lists the checks run and those it cannot run. Uses up to 7 requests | `company_number` |

## The result

| Field | Contents |
|---|---|
| `company` | `company_number`, `company_name`, `company_status`, `company_status_detail`, `type`, `date_of_creation`, `date_of_cessation`, `jurisdiction`, `registered_office_address` and `nature_of_business`, when the profile has them |
| `checked_on` | The date of the check, `YYYY-MM-DD` (UTC). Ages and "in the last 12 months" are counted back from it |
| `observations` | What the checks found, each with `check`, `level` (`flag` or `note`) and `detail` |
| `checks_run` | The checks that ran |
| `checks_not_run` | The checks that did not run or cannot be run, each with `check` and `reason` |
| `disclaimer` | What the result is not; see [The disclaimer](#the-disclaimer) |

An observation looks like this:

```json
{"check": "accounts", "level": "flag", "detail": "accounts are overdue: they were due by 2026-09-30"}
```

- A **flag** is something a reviewer should look into.
- A **note** is context: worth knowing, but not on its own something to look into.

Neither is a verdict. A company with no observations has simply raised none in the checks that ran. The register records what companies file but does not verify it.

## Profile checks

These read the company's profile, the same one [`get_company`](company.md#the-profile) returns.

| Check | Flag | Note |
|---|---|---|
| `company_status` | The status is not `active`, `registered` or `open` (the statuses of a company, entity or branch that is still live). Any `company_status_detail`, such as `active-proposal-to-strike-off`, is also a flag | |
| `accounts` | The accounts are overdue, with the date they were due by | The latest accounts were filed as dormant, with the date they were made up to |
| `confirmation_statement` | The confirmation statement is overdue, with the date it was due by | |
| `registered_office` | The registered office address is in dispute, or Companies House has marked it as undeliverable (a flag for each) | |
| `company_age` | Incorporated less than 12 months ago | Incorporated 12 to 23 months ago |
| `name_changes` | | The company changed its name in the last 12 months, with each old name and the date it ended |

## Insolvency

`insolvency` reads the insolvency record when the profile's `records_available` lists `insolvency`. Each case is a flag with its type and dates, such as `insolvency case: creditors-voluntary-liquidation (wound-up-on 2025-02-01)`. A record with a status but no cases is a flag with the status.

## Charges

`charges` reads the first 100 charges when the profile lists `charges`. Charges not yet satisfied are common for a trading company, so they are a note, not a flag: `charges not satisfied: 2, held by Big Bank PLC, Other Bank; see list_charges`.

- The number comes from Companies House's totals (all charges less the satisfied ones), so charges beyond the first 100 are counted too. Part-satisfied charges count as not satisfied.
- The lenders named are those entitled to the charges not satisfied among the first 100. When there are more than 100 charges, the note says the lenders are from the first 100.

## Officers

`officers` reads the first 100 officer records and judges them by the company's type. The officers who run a company, and the check on them, depend on the type:

| Company type | Officers in charge | Observation |
|---|---|---|
| `ltd`, `plc`, `private-unlimited`, `private-unlimited-nsc`, `private-limited-guarant-nsc`, `private-limited-guarant-nsc-limited-exemption`, `private-limited-shares-section-30-exemption`, `old-public-company` | Directors | Flag when no director is in post. Flag when every director in post is a company: a UK company must have at least one director who is a person |
| `llp` | Members and designated members, people or corporate | Flag when none is in post |
| `oversea-company` | Directors or equivalent officers: roles containing `director`, or starting `member-of-` or `person-authorised` | Flag when none is in post |
| `eeig` | Managers | Flag when none is in post |
| `registered-overseas-entity` | Managing officers | Flag when none is in post and a current PSC statement says that not every beneficial owner is identified, or that their information is not provided. Otherwise managing officers are not required, and none is no observation |
| `limited-partnership`, `uk-establishment` | None on the register: a limited partnership's partners and a UK establishment's officers (who are its overseas parent's) have no officer records | Not checked |
| Any other type | Not known to the check | Note when no officer at all is in post, saying that whether this type must have any was not checked |

Two notes can be added for any type with officer records:

- Officer changes in the last 12 months: how many of the records read were appointed and how many resigned.
- More than 100 officer records: only the first 100 were checked. When none of the officers in charge is among those 100, that is a note pointing to [`list_officers`](people.md#officers-in-post) with `current_only`, not a flag.

## Ownership

`ownership` reads the persons with significant control (the first 100) and the PSC statements, and only the current ones of each count.

| Finding | Level |
|---|---|
| A PSC statement that says the company has not identified, or not confirmed, who controls it: its code contains `not-identified`, `not-confirmed`, `not-yet-completed`, `failed-to`, `restrictions-notice`, `no-response`, `unidentified`, `not-provided` or `awaiting-confirmation` | Flag, with the code |
| Any other PSC statement | Note, with the code |
| A PSC that is a company or legal person: control passes through other entities, so check who controls them | Note, with their names |
| A PSC the register marks as sanctioned (`is_sanctioned`) | Flag, with their names |
| A super-secure PSC, whose details are protected from the public register | Note |
| No current PSC and no current statement, for a kind of company the PSC rules cover, with a current exemption | Note, naming the exemption |
| The same, without an exemption, or when the exemptions could not be read | Flag: this may be a gap in the register |
| The same, for a kind of company this check does not expect to report PSCs | Note |

The kinds of company the check expects to report PSCs are UK companies (the types in the first row of the officers table), `llp`, `european-public-limited-liability-company-se`, `united-kingdom-societas`, `eeig`, `scottish-partnership`, `registered-overseas-entity` (for its beneficial owners), and `limited-partnership` only when its number starts with `SL` (a Scottish limited partnership).

The "no current PSC and no current statement" rows apply only when the whole list fits in the first 100 and the statements were read. Exemptions are read only for a kind of company the PSC rules cover with no current PSC or statement, as `list_persons_with_significant_control` does (see [People and ownership](people.md#when-no-one-is-listed)). If the statements could not be read, those rows are skipped and `checks_not_run` lists `ownership_statements` with the reason.

## Identity verification

`identity_verification` looks at the people in post:

- the officers in charge (as in the officers table) who are people, not companies;
- the persons with significant control whose kind is `individual`.

For each person it reads the [identity verification record](people.md#identity-verification):

| Record | Counts as |
|---|---|
| `appointment_verification_start_on` on or before `checked_on`, and `appointment_verification_end_on` absent or after `checked_on` | A statement on record |
| No statement on record, and `appointment_verification_statement_due_on` before `checked_on` | Overdue |
| Anything else, including no record | None on record yet |

- Overdue statements are a flag, naming each person with their due date.
- A note gives the tally, for example `1 of 2 officers in charge who are people, 0 of 1 individual PSCs`. Companies House is phasing verification in, so none on record is not a failure, and the register shows that a statement was filed, not the check itself.
- Company types without officers in charge in the officers table (limited partnerships, UK establishments and unknown types) have no officer tally.

## checks_run and checks_not_run

`checks_run` lists the checks that ran, in this order: `company_status`, `accounts`, `confirmation_statement`, `registered_office`, `company_age`, `name_changes`, `insolvency`, `charges`, `officers`, `ownership`, `identity_verification`. `insolvency` and `charges` run even when the profile lists no such record; they then have nothing to find.

A check whose records could not be read does not run. It is listed in `checks_not_run` with the reason, `could not be read:` and the error, and the other checks still run. Parts of checks are listed the same way:

| Entry | When |
|---|---|
| `insolvency`, `charges`, `officers`, `ownership` | That record could not be read |
| `ownership_statements` | The PSC statements could not be read |
| `identity_verification_officers`, `identity_verification_pscs` | The officers, or the persons with significant control, could not be read |
| `identity_verification` | Neither could be read |

`checks_not_run` always ends with what the register cannot show:

| Check | Reason |
|---|---|
| `sanctions` | The register says nothing about sanctions; screen the company, its officers and owners against the UK sanctions list separately |
| `politically_exposed_persons` | The register does not record politically exposed persons |
| `credit_and_finances` | The register holds filed accounts but no credit data; read the accounts with `download_filing_document` or ask a credit reference agency |
| `director_disqualifications` | Disqualifications are not linked to officer ids; search each officer's name with `search_disqualified_officers` |
| `litigation_trading_and_other_ownership` | Court cases, trading performance and ownership outside the PSC rules are not on the register |

The `is_sanctioned` flag above is only what the register itself marks. It is not a sanctions screen.

## The disclaimer

Every result ends with this text:

> These observations come from the public Companies House register, which records what companies file but does not verify it. This is not a verification, credit check, sanctions or politically exposed person screening, or a clearance decision, and having few or no observations does not mean the company is sound.

## Requests

A check uses up to 7 Companies House requests:

1. the profile;
2. the first 100 officers;
3. the first 100 persons with significant control;
4. up to 100 PSC statements;
5. the first 100 charges, only when the profile lists charges;
6. the insolvency record, only when the profile lists insolvency;
7. the exemptions, only when no current owner or statement is recorded for a kind of company the PSC rules cover.

The profile is read first. A company that does not exist gives `not_found: no company with number ...; find the number with search_companies` after one request.

## How it differs from company_report

| | `company_report` | `due_diligence_check` |
|---|---|---|
| Purpose | Sets out the records for you to read | Judges the records and says what to look into |
| Returns | Profile, officers in post, current PSCs and statements, recent filings, charges not satisfied, insolvency cases | Observations (flags and notes), the checks run and not run, and the disclaimer |
| Filing history | The most recent filings (default 10) | Not read |
| Exemptions | Not read | Read when no current owner or statement is recorded |
| Requests | Up to 7 | Up to 7 |

Both read the first 100 officers, persons with significant control and charges, and read charges and insolvency only when the profile lists them. The [`due_diligence` prompt](../prompts.md#due-diligence-screen) calls both, then searches the disqualified directors register for the people in post and looks at their other appointments. See [Reading a company](company.md#company-report) for `company_report`.

## Never a clearance

`due_diligence_check` does not rate, score or approve a company. Few or no observations do not mean the company is sound: the register is unverified, most checks read only the first 100 records of each kind, and sanctions, politically exposed persons, credit, disqualifications, litigation and ownership outside the PSC rules are outside it. Use it to decide what to look into, not whether to proceed.
