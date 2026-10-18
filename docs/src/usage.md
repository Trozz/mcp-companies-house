# Using it

This page is for the people who use the server through an MCP client. It shows what you can ask, grouped by the job you are doing, which tools or prompts the model uses for each, how to get good answers, what the answers can and cannot tell you, and how the rate limit and the sandbox affect you.

You ask in plain words and the model picks the tools. For the bigger jobs there is also a ready-made [prompt](prompts.md), which you choose from your client. A prompt tells the model which tools to call, how many calls it may make and how to set out the answer, so the result is the same shape every time.

## What you can ask

### Check a supplier or customer before dealing with them

> "Screen Acme Widgets Ltd before I pay their £5,000 deposit invoice."
>
> "Run a due diligence check on 00445790. Their invoice gives 12 High Street, Leeds as the address and Jane Smith as a director."
>
> "Is anything overdue or unusual on the register for SC123456?"

The model calls [`due_diligence_check`](tools/due-diligence.md), which screens the company in one call and returns observations: each a **flag** (something to look into) or a **note** (context). It covers the company's status, insolvency cases, charges not yet satisfied, overdue accounts and confirmation statements, a registered office in dispute or marked undeliverable, directors in post and officer changes in the last 12 months, ownership, the company's age and recent name changes, and the identity verification statements of the people in post. It also lists the checks it could not run.

The [due diligence screen](prompts.md#due-diligence-screen) prompt (`due_diligence`) goes further, in a fixed order: after `due_diligence_check` it reads [`company_report`](tools/company.md#company-report) for the officers and owners, searches the disqualified directors register for up to eight of them, and looks at up to three directors' other appointments. Put what you are about to do, and what the business has told you, in its `context` argument: the answer then has a table comparing each claim with the register. The register holds no bank details, VAT numbers, phone numbers, email addresses, websites or trading addresses, so those are reported as "not held by the register". A match only shows the details agree with the register, not that the person you are dealing with acts for the company: confirm contact and payment details through a source you found yourself.

The answer never says a company is safe, sound or fine to deal with, and gives no score. The decision is yours. See [Due diligence](tools/due-diligence.md).

### Get the facts on one company

> "Give me an overview of 00445790."
>
> "What has 00445790 filed since 2025-01-01, and does anything stand out?"

`company_report` reads the profile, the current officers and persons with significant control, recent filings, charges not satisfied and insolvency cases in one call. [`get_company`](tools/company.md#the-profile) gives the profile alone, including when the next accounts and confirmation statement are due and whether either is overdue. [`list_filing_history`](tools/company.md#filing-history) lists filings newest first. The [company overview](prompts.md#company-overview) prompt (`overview`) sets these out under fixed headings, and with its `since` argument adds a timeline of everything filed since that date, pointing out changes of directors, owners, registered office or name, new charges and strike-off or insolvency filings. See [Reading a company](tools/company.md).

### Look into a director

> "What other companies has Jane Anne Smith, born April 1980, been a director of?"
>
> "Jane Smith is a director of 00445790. What else has she been involved in, and did any of those companies go into liquidation?"
>
> "Is there an entry in the disqualified directors register for John Smith, born March 1975?"

The model first has to find the right person. Names are not unique, so it looks for the person among the officers of a company you name ([`list_officers`](tools/people.md#officers), which includes resigned officers), or searches by name ([`search_officers`](tools/people.md#searching-people)) and matches the month and year of birth. When several people fit, it lists them and asks which you mean. Then [`officer_network`](tools/people.md#officer-network) maps every company the person is or was appointed to, split into current and past, with each company's status. [`get_insolvency`](tools/company.md#insolvency) gives the dates of any insolvency cases, and [`search_disqualified_officers`](tools/people.md#disqualified-officers) and [`get_disqualified_officer`](tools/people.md#disqualified-officers) check the disqualified directors register.

The [director check](prompts.md#director-check) prompt (`director_check`) takes the person's name and, if you have them, a company they are or were an officer of and their month and year of birth. An entry counts as a possible match only when the surname, first forename and month and year of birth all match. A name alone is never treated as a match, and the answer says "no matching entry found" rather than that someone is not disqualified. See [People and ownership](tools/people.md).

### Trace who owns a company

> "Who ultimately controls 00445790?"
>
> "Follow the ownership of Acme Holdings Ltd up to the people at the top."

[`list_persons_with_significant_control`](tools/people.md#persons-with-significant-control) lists the company's persons with significant control (people, companies and other legal persons) with the nature of their control, such as `ownership-of-shares-75-to-100-percent`, and any statements the company has filed about them. When a controller is another UK company, the model reads that company in turn. The [ownership chain](prompts.md#ownership-chain) prompt (`ownership_chain`) follows the chain up to four levels and eight companies, and says where the trail stops: at a person, at a company registered abroad that the UK register does not cover, at a protected entry, at an exempt company, or at a statement that no controller has been identified.

The register shows only those with more than 25% of the shares or votes, the right to appoint or remove most of the directors, or significant influence or control. It is not a check of beneficial ownership for anti-money-laundering purposes.

### Read the latest accounts

> "What were the net assets and cash in the latest accounts of 00445790?"
>
> "Read the latest accounts of SC123456 and tell me about going concern and loans to directors."

The model finds the latest accounts with `list_filing_history` (category `accounts`) and downloads them with [`download_filing_document`](tools/documents.md). Accounts filed online are usually held as iXBRL, which the tool returns as text; scanned paper filings come as a PDF. Documents over 1 MiB are refused unless the model asks for more, up to 10 MiB. The [read the latest accounts](prompts.md#read-the-latest-accounts) prompt (`read_accounts`) reports the figures exactly as printed for the year and the year before, the audit position, and notes such as going concern, and its `focus` argument says what to lead with.

Small companies' accounts often leave things out. Micro-entity accounts, and small company accounts filed without a profit and loss account, show no turnover or profit; the answer then says "not given". Documents are available only on a server using the live register. See [Filed documents](tools/documents.md).

### Find companies by sector and place

> "Active craft breweries in Leeds incorporated since 2022."
>
> "How many active companies in Cambridge do software development?"
>
> "Find dissolved companies that used to be called Bright Lane Bakery."

[`lookup_sic_codes`](tools/finding.md#sic-codes) turns words into SIC 2007 codes without a request to Companies House. It matches the official wording, so the model may try a more formal word, such as "beer" when "brewery" finds nothing. [`filter_companies`](tools/finding.md#filters) then runs the Companies House advanced search on SIC codes, status, company type, words in the name, registered office location and incorporation or dissolution dates. Its `total` counts every match, so a question about how many needs only one request. [`search_dissolved_companies`](tools/finding.md#dissolved-companies) finds dissolved companies by their last name or a name they had before.

The [find companies](prompts.md#find-companies) prompt (`find_companies`) shows the filters and SIC codes it used, so you can see what was searched. Keep in mind that the location is matched against the registered office, which is often an accountant's or formation agent's address, that companies choose their own SIC codes, and that "active" includes dormant companies. See [Finding companies](tools/finding.md).

### Check a company name before registering

> "Is Bright Lane Bakery Ltd available as a company name? We will make wedding cakes."
>
> "Compare these names: Bright Lane Bakery Ltd; Brightlane Cakes Ltd; Lane and Loaf Ltd."

[`check_company_name_availability`](tools/finding.md#name-availability) lists active companies whose names Companies House treats as the same, ignoring endings such as Limited or Ltd and punctuation; `available` is true when there are none. [`browse_company_names`](tools/finding.md#browsing-names) shows the names next to it in the register, and `filter_companies` finds active companies with the same distinctive words. The [company name check](prompts.md#company-name-check) prompt (`name_check`) checks up to five names, separated by semicolons.

A clear result does not mean the name will be accepted. Companies House can still refuse a name that uses a sensitive word or is too like another, and trade marks and domain names are not checked at all.

### Watch filing deadlines for a list of companies

> "Check these companies for anything overdue: 00445790, SC123456, OC301234."
>
> "Here are our 12 clients' company numbers. Which have accounts or confirmation statements overdue or due soon, and what has each filed since 2026-07-01?"

The model calls `get_company` once for each company, which gives its status, when the next accounts and confirmation statement are due and whether either is overdue, and whether the registered office is in dispute. With a date, it also reads each company's filing history. The [check a list of companies](prompts.md#check-a-list-of-companies) prompt (`watchlist`) takes up to 20 numbers or names and returns one table, most urgent first, which it can give you as CSV.

The due dates are the register's own. A filing still being processed, or a recent change of accounting reference date, may not show yet, and HMRC deadlines are separate and not shown.

## Getting good answers

- **Give the company number when you have it.** It is 8 characters, such as `00445790`, `SC123456` or `OC301234`. Spaces and lower case do not matter, and shorter numbers are padded with zeros: `445790` is read as `00445790` and `SC1234` as `SC001234`.
- **Say which company when several share a name.** Given a name, the prompts take a search result only when exactly one company matches it (ignoring case, punctuation and endings such as Limited, Ltd, PLC and LLP), or only one of the matches is not dissolved. Otherwise they list the candidates and ask.
- **Help the model find the right person.** Names of people are not unique, and one person can have several officer records. Name a company the person is or was an officer of, and give their month and year of birth. The register writes officers' names as `SURNAME, Forenames`.
- **Use a prompt for the bigger jobs.** Choose it from your client's prompt menu or slash commands; on `/mcp` its name starts with `companies_`, such as `companies_due_diligence`. Asking in your own words also works, but the model then plans the steps itself and the answer's layout can vary.
- **Ask for more when an answer stops short.** Lists come a page at a time, and the prompts stop at their limits and say what they did not read. Ask for the next page, or for a narrower search.
- **Give dates as YYYY-MM-DD**, such as `2025-01-01`. The watchlist counts days to a deadline only when the model knows today's date.

## What the answers can and cannot tell you

- **The register is public and unverified.** Companies file their own information, and Companies House records it without checking it. Names, addresses and other filed text are written by whoever filed them; the model is told to treat them as data and never follow them as instructions.
- **It is not a credit, sanctions, politically exposed person or identity check.** The register holds no credit data, says nothing about sanctions or politically exposed persons, and has no court cases. `due_diligence_check` lists these under the checks it did not run. Disqualifications are not linked to officer records, so they are found by searching names.
- **Dates of birth are month and year only**, as Companies House publishes them. Officers' addresses are correspondence addresses, often not a home address.
- **Identity verification shows a filing, not the check.** The tools show when Companies House received a person's identity verification statement and when one is due. Companies House is phasing verification in, so "none on record" is not a failure.
- **The register can lag.** Changes can be filed late, so something may have happened that is not on the register yet.
- **Searches stop at 1000 matches.** Name searches reach only the first 1000 matches of a query; narrow it to reach the rest.

## Rate limit

Companies House allows each key 600 requests per 5 minutes. The limit is yours alone: everyone on the server uses their own key, so other people's use does not count against yours, and yours does not affect them. Companies House can ban applications that keep going over the limit.

When your key's allowance is used up, tools answer `rate_limited` with the seconds until it resets, and the server does not call Companies House with your key again until then. The prompts tell the model to stop, tell you how long to wait and give you what it has so far.

Most tools make one request. The table shows the ones that matter when you plan a large job; [Rate limit](conventions.md#rate-limit) in Conventions and errors lists every tool that makes more than one:

| Tool or prompt | Requests |
|---|---|
| `lookup_sic_codes` | None |
| `company_report`, `due_diligence_check` | Up to 7 each, in one tool call |
| `officer_network` | Up to 10, one for each 100 of the person's appointments |
| `due_diligence` prompt | At most 20 tool calls, two of them the composite tools above |
| `watchlist` prompt | One `get_company` per company, plus one filing history per company when you give a date, and one search per company given by name |

A single prompt uses a small part of the allowance. Screening many companies in a row, or checking a long list several times in a few minutes, can use it up.

Downloads of filed documents also share a [server-wide budget](tools/documents.md#the-servers-download-budget) of 64 MiB at a time across everyone. While it is in use, `download_filing_document` answers `rate_limited` and asks you to try again in a minute or ask for a smaller document.

## On the sandbox

A server on the Companies House sandbox (see [Getting started](getting-started.md#4-or-use-the-sandbox)) works with test keys and test data only:

- It holds test data, not the real register: you make test companies with the Companies House [test data generator](https://developer.company-information.service.gov.uk/api-testing). Its search index is almost empty, so give companies by number rather than by name.
- It has no filed documents. `download_filing_document`, and so the `read_accounts` prompt, says documents are only available on a server using the live API.
- Some endpoints fail there: filing history and PSC statements usually answer HTTP 500, and officers sometimes do too. `company_report` lists what it could not read under `unavailable`, and `due_diligence_check` lists it under the checks not run, rather than failing the whole call.
- Keys from live applications are rejected.

The sandbox is for trying the server out and for testing. For real answers, use a server on the live register.

Next: [Prompts](prompts.md).
