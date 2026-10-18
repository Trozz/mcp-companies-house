# People and ownership

Endpoint `/companies` (or `companies_<tool>` under [`/mcp`](../combined.md)). These tools read who runs and who controls a company, where else its officers are appointed, and the register of disqualified directors.

## Tools

| Tool | Kind | What it does | Key parameters |
|---|---|---|---|
| `list_officers` | read | A company's officers (directors, secretaries, LLP members and others), current and resigned: role, appointment and resignation dates, occupation, nationality, country of residence, month and year of birth, correspondence address, identity verification and `officer_id`, with `active_count` and `resigned_count` | `company_number`, `order_by` (`appointed_on`, `resigned_on` or `surname`; default Companies House's own order), `current_only`, `role` (`directors`, `secretaries` or `llp_members`), `limit` (1 to 100, default 20), `start_index` (default 0) |
| `list_persons_with_significant_control` | read | The people, companies or legal persons who own or control a company, and the beneficial owners of overseas entities: kind, natures of control, notified and ceased dates, nationality, month and year of birth, address. The first page also lists PSC statements | `company_number`, `limit` (1 to 100, default 20), `start_index` (default 0) |
| `get_officer_appointments` | read | Every company appointment of one officer, current and past, with the company's number, name and status, the role and its dates | `officer_id`, `active_only`, `limit` (1 to 100, default 20), `start_index` (default 0) |
| `officer_network` | read | Every company one officer is or was appointed to, split into current and past, with a count of the companies by status. Reads up to 1000 appointments | `officer_id`, or `officer_name` |
| `search_officers` | read | Officers by name, with `officer_id`, number of appointments, month and year of birth and address | `query`, `limit` (1 to 100, default 20), `start_index` (default 0) |
| `search_disqualified_officers` | read | People and companies disqualified from acting as company directors, by name, with `officer_id` and `officer_kind` | `query`, `limit` (1 to 100, default 20), `start_index` (default 0) |
| `get_disqualified_officer` | read | One disqualified officer's record: each disqualification's type, dates and reason, the companies where the misconduct happened and the court, and any permissions to act | `officer_id`, `officer_kind` (`natural` or `corporate`; default: try `natural`, then `corporate`) |

## People on the register

- Names are not unique, and one person can have more than one `officer_id`. `search_officers` can list the same person more than once. Tell people apart by their month and year of birth, their address and the companies they are appointed to.
- Officers' and owners' dates of birth are month and year only, as `YYYY-MM` (or the year alone when no month is given), as Companies House publishes them. Disqualified officers' dates of birth are shown as Companies House gives them, which can be a full date.
- Officers are addressed by the `officer_id` that `list_officers`, `search_officers` and [`search_all`](finding.md#searching-by-name-or-number) return. Disqualified officers have their own `officer_id` and `officer_kind` from `search_disqualified_officers`; the two kinds of id are not linked.

## Officers

Each officer from `list_officers` has:

| Field | Meaning |
|---|---|
| `name` | As Companies House writes it, usually `SURNAME, Forenames` |
| `officer_role` | Such as `director`, `secretary`, `corporate-director` or `llp-designated-member` |
| `appointed_on`, `appointed_before`, `resigned_on` | The appointment's dates; `resigned_on` only once the officer has resigned |
| `occupation`, `nationality`, `country_of_residence` | As filed |
| `date_of_birth` | Month and year, `YYYY-MM` |
| `address` | The correspondence address, on one line |
| `former_names` | Earlier names, when filed |
| `identification`, `principal_office_address`, `contact_details`, `responsibilities` | When Companies House gives them |
| `identity_verification` | See [Identity verification](#identity-verification) |
| `officer_id` | For `get_officer_appointments` and `officer_network` |
| `appointment_id` | This appointment's id at Companies House. No tool takes it |

The list has `total`, `active_count`, `resigned_count` and, when Companies House gives it, `inactive_count`.

### Officers in post

`current_only: true` lists only the officers in post, that is, not resigned.

- The server asks Companies House for its own filter of active officers (`filter=active`, which Companies House's published specification does not describe). It reads pages of 100 from `start_index` until your page is full, the list ends, or it has read 5 pages (500 officers), and checks every row itself as well.
- If Companies House refuses the filter, the server reads the list without it; if it ignores the filter, resigned officers are dropped by the server. In both cases `total` is left out, because the number in post is then not known.
- Companies House lists the officers of a closed company, such as a dissolved one, as inactive rather than resigned. A closed company therefore has no officers in post: `current_only` gives an empty list with `total: 0` and the note `the company is closed, so none of its officers is in post; list them without current_only`.
- `officers_read` says how many officer records were read from `start_index`, and `next_start_index` continues from there.

### One kind of officer

`role` limits the list to one kind:

| `role` | Officers |
|---|---|
| `directors` | Directors, corporate directors and nominee directors |
| `secretaries` | Secretaries and corporate secretaries |
| `llp_members` | LLP members and designated members, people or corporate |

Companies House cannot filter by kind, so the server reads the list from `start_index`, 100 at a time and up to 500 officers, until the page is full. `officers_read` says how many it read and `next_start_index` continues from where it stopped. When it reaches 500 officers without filling the page and more remain, `note` says so (`read ... officers without filling the page; pass next_start_index to read on`). `total` is left out. `role` and `current_only` can be combined.

### Identity verification

Companies House is phasing in identity verification for directors and persons with significant control. `identity_verification` on an officer, an appointment or a person with significant control is Companies House's record for that role:

| Field | Meaning |
|---|---|
| `appointment_verification_start_on` | When Companies House received the person's identity verification statement for this role |
| `appointment_verification_end_on` | When it stops applying, if it does |
| `appointment_verification_statement_due_on` | When a statement is due |

For checks made by an authorised agent, it also says who made them. When `identity_verification` is absent, no statement is on record yet: that does not mean verification failed. The register shows that a statement was filed, not the check itself. `due_diligence_check` counts the statements of the people in post and flags any past their due date; see [Due diligence](due-diligence.md#identity-verification).

## Persons with significant control

Each person with significant control (PSC) has:

| Field | Meaning |
|---|---|
| `name` | As filed |
| `kind` | Companies House's kind without the `-person-with-significant-control` ending: `individual`, `corporate-entity`, `legal-person` or `super-secure`. Beneficial owners of overseas entities keep their full kind, such as `individual-beneficial-owner` |
| `natures_of_control` | Companies House codes, such as `ownership-of-shares-75-to-100-percent` |
| `notified_on`, `ceased_on` | The notified date, and the ceased date once they are no longer a PSC |
| `nationality`, `country_of_residence`, `date_of_birth` | For people; `date_of_birth` as `YYYY-MM` |
| `address` | The address, on one line |
| `principal_office_address`, `identification`, `description` | When Companies House gives them |
| `is_sanctioned` | Whether the register marks the owner as sanctioned, when Companies House gives it |
| `identity_verification` | As for [officers](#identity-verification) |
| `psc_id` | The PSC's id at Companies House. No tool takes it |

A `super-secure` PSC is one whose details are protected from the public register. The list has `total`, `active_count` and `ceased_count`.

### Statements

The first page (`start_index: 0`) also has `statements`: what the company has filed about its PSCs, for example that it knows of no person with significant control, or that a PSC's details are not confirmed. Each statement has `statement` (a code such as `psc-details-not-confirmed`), `notified_on`, `ceased_on`, `linked_psc_name`, `restrictions_notice_withdrawal_reason`, `statement_id` and the `psc_id` of the PSC it concerns.

- Up to 100 statements are read. When the company has more, `statements_total` gives the number and `statements_note` says only the newest 100 are listed.
- If the statements cannot be read, the PSCs are still returned, and `statements_unavailable` gives the error.
- Later pages leave the statements out.

### When no one is listed

A UK company or LLP should have filed at least one current PSC, a current PSC statement or a PSC exemption. When the first page holds the whole list, the statements were read, and neither lists anyone or anything current, the tool reads the company's exemptions (one more request) and explains in `note`:

| Finding | `note` |
|---|---|
| A current exemption | `no persons with significant control are listed because the company is exempt from the PSC rules:` and the exemption, such as `psc-exempt-as-trading-on-regulated-market` |
| No exemption | `Companies House records no person with significant control, no current PSC statement and no PSC exemption for this company. A UK company or LLP should have filed one of these, so this may be a gap in the register; some kinds of company, such as overseas companies, do not report PSCs` |
| The exemptions could not be read | That no PSC or current statement is listed, and that whether the company is exempt could not be checked, with the error |

When the company lists only former PSCs, the note speaks of current ones. `get_exemptions` shows the exemptions in full; see [Reading a company](company.md#exemptions).

## Appointments

`get_officer_appointments` lists one officer's appointments across companies, by `officer_id`. `active_only: true` asks Companies House for only the appointments the officer still holds. The result has the officer's `officer_id`, `name`, `date_of_birth` and `is_corporate_officer`, the paging fields, and `items`, each with:

- `company_number`, `company_name` and `company_status`;
- `officer_role`, `appointed_on`, `appointed_before` and `resigned_on`;
- `occupation`, `nationality`, `country_of_residence`, the `name` and `address` used on that appointment, `identification` and `identity_verification` when given.

An unknown id gives `not_found: no officer with id ...; take officer_id from list_officers or search_officers`.

## Officer network

`officer_network` reads an officer's whole appointment list, 100 at a time, up to 1000 appointments (10 requests), and sets it out:

| Field | Meaning |
|---|---|
| `officer_id`, `name`, `date_of_birth`, `is_corporate_officer` | The officer |
| `total_appointments`, `appointments_read` | How many appointments Companies House holds, and how many were read |
| `companies` | The number of different companies among them |
| `company_statuses` | The number of those companies with each status, such as `{"active": 2, "liquidation": 1}` |
| `current_count`, `current` | Appointments still held: not resigned, at a company that is not `dissolved`, `converted-closed`, `closed` or `removed` |
| `past_count`, `past` | The rest: resigned, or at a company that is dissolved or closed |
| `note` | Only when not every appointment was read, for example `only the first 1000 of 1250 appointments were read; use get_officer_appointments with start_index 1000 for the rest` |

An appointment at a company in liquidation or administration still counts as current, because the company is not dissolved or closed. Each appointment has the fields of `get_officer_appointments`.

### By name

Pass `officer_id` when you have it. `officer_name` is used only without `officer_id`: the server searches officers with it once (the first 100 results) and takes the one officer whose name is the same. Names are compared as forenames then surname, ignoring case, full stops and the titles Mr, Mrs, Ms, Miss, Mx, Dr, Sir, Dame, Lord, Lady and Prof. Companies House's `SURNAME, Forenames` order is read the right way round.

| `officer_name` | Compared with | Same name |
|---|---|---|
| `Mrs Jane Anne Smith` | `SMITH, Jane Anne` | yes |
| `Dr. Jane SMITH` | `jane smith` | yes |
| `Jane Smith` | `Jane Anne Smith` | no |
| `James THOMAS` | `Thomas JAMES` | no |

- One officer with the name: their network is returned.
- Several: refused with `validation: N officers are named ... (names are not unique, and one person can have several officer ids); pass the officer_id of one:` and up to 10 candidates, each with `officer_id`, name, month and year of birth, number of appointments and address.
- None with exactly that name, but the search found others: refused with `validation: no officer is named exactly ...; pass officer_id instead. Closest matches:` and up to 10 candidates.
- Nothing found: `not_found: no officer is named ...; check the name with search_officers`.

## Searching people

`search_officers` finds officers by name. Each result has `name`, `officer_id`, `appointment_count`, `date_of_birth` (month and year), `address` and `description`. It reaches only the first 1000 matches; see [the 1000-result window](finding.md#the-1000-result-window). `search_all` searches officers together with companies and disqualified officers.

## Disqualified officers

`search_disqualified_officers` finds people and companies disqualified from acting as directors. Each result has `name`, `officer_id`, `officer_kind` (`natural` for a person, `corporate` for a company), `date_of_birth`, `address` and `description`. The 1000-result window applies here too.

`get_disqualified_officer` takes the `officer_id` and `officer_kind` from the search. Without `officer_kind` it tries `natural`, then `corporate`, which can take two requests. It returns `officer_id`, `officer_kind` and the record as Companies House holds it, without empty values: each disqualification's type, dates and reason, the companies where the misconduct happened and the court, and any permissions to act. An unknown id gives `not_found: no disqualified officer with id ...; take officer_id from search_disqualified_officers`.

Disqualifications are not linked to officer ids, so to check an officer, search their name and compare the date of birth as well. A matching name alone does not show that it is the same person. The [`director_check`](../prompts.md#director-check) and [`due_diligence`](../prompts.md#due-diligence-screen) prompts follow these steps.
