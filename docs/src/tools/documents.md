# Filed documents

Endpoint `/companies` (or `companies_<tool>` under [`/mcp`](../combined.md)). `download_filing_document` fetches a filed document, such as a set of accounts or a confirmation statement, from the Companies House Document API. This page covers the ids and formats it takes, the checks it makes before downloading, how the document comes back, and why it works only on a server using the live API.

## Tool

| Tool | Kind | What it does | Key parameters |
|---|---|---|---|
| `download_filing_document` | read | Downloads a filed document and returns its metadata, then the document: a PDF as an embedded resource, `xhtml`, `xml` and `json` as text. Checks the formats and size first. With `metadata_only`, returns only the pages, formats and sizes | `document_id`, `format` (`pdf`, the default, `xhtml`, `xml` or `json`), `max_bytes` (1 to 10485760, default 1048576), `metadata_only` |

## Finding a document

Documents belong to filings. List the filings with `list_filing_history`, for example with `categories: ["accounts"]`, and take the `document_id` of the one you want; `get_filing` gives it too. See [Reading a company](company.md#filing-history).

```json
{"document_id": "<document-id>", "metadata_only": true}
```

`document_id` can be given as:

| Form | Example |
|---|---|
| The `document_id` from `list_filing_history` or `get_filing` | `<document-id>` |
| A Document API path | `/document/<document-id>` or `/document/<document-id>/content` |
| A document metadata link | `https://frontend-doc-api.company-information.service.gov.uk/document/<document-id>` |

The id is the part after `document`. It must be letters, digits, `-` and `_`, up to 100 characters; anything else is refused with `validation: document_id must be a document_id from list_filing_history or get_filing, or its document_metadata link`.

## Formats

| `format` | Content type | Notes |
|---|---|---|
| `pdf` | `application/pdf` | The default. Most filings are held only as PDF |
| `xhtml` | `application/xhtml+xml` | Electronically filed accounts are often also held as XHTML (iXBRL) |
| `xml` | `application/xml` | |
| `json` | `application/json` | |

A document is held in one or more of these. `metadata_only: true` lists which, with their sizes, before you download anything.

## Metadata

With `metadata_only: true` the tool makes one request and returns:

| Field | Meaning |
|---|---|
| `document_id` | The id you passed |
| `company_number`, `category`, `pages`, `created_at` | As the Document API gives them |
| `significant_date`, `significant_date_type` | When the Document API gives them |
| `formats` | Each format the document is held in, with `format`, `content_type` and `size_bytes`. A content type outside the four formats above is shown with the content type as its `format` |

## Checks before downloading

Without `metadata_only`, the tool goes through these steps and stops at the first that fails:

1. It reads the metadata. A document the Document API does not know gives `not_found: no document <document-id>; take document_id from list_filing_history or get_filing`.
2. It checks that the document is held in the requested format. If not, nothing is downloaded and the error lists what it is held as, for example `validation: document <document-id> is not held as xml; it is held as: pdf (52344 bytes), xhtml (10230 bytes)`.
3. When the metadata gives a size, it checks that size against `max_bytes`. A larger document is refused before downloading: `validation: document <document-id> is ... bytes as pdf, more than max_bytes (1048576); raise max_bytes (up to 10485760) or choose another format: ...`.
4. It reserves `max_bytes` from the server's [download budget](#the-servers-download-budget).
5. It downloads the document. If the download passes `max_bytes`, for example because the metadata gave no size, it stops at once with `validation: the document is larger than max_bytes (...); raise max_bytes (up to 10485760) or use metadata_only to see its size`.

`max_bytes` defaults to 1048576 (1 MiB) and can be raised to 10485760 (10 MiB). A value outside 1 to 10485760 is refused (`max_bytes must be between 1 and 10485760`).

## The result

The result has two parts:

1. Text: the document's metadata as JSON, as `metadata_only` returns it, plus the `format`, `content_type` and `size_bytes` of what was downloaded. `content_type` is the type Companies House served the document as.
2. An embedded resource holding the document, with its MIME type and, as its URI, the Document API address of the content (`https://document-api.company-information.service.gov.uk/document/<document-id>/content` on the live API).

A PDF comes as a base64 blob. `xhtml`, `xml` and `json` come as text, unless the bytes are not valid UTF-8, in which case they come as a base64 blob too.

The [`read_accounts` prompt](../prompts.md#read-the-latest-accounts) finds the latest accounts with `list_filing_history` and downloads them as `xhtml` first, and as `pdf` when they were filed on paper or `xhtml` is refused.

## The server's download budget

The server downloads and encodes at most 64 MiB of documents at once, across everyone who uses it. Each download reserves its `max_bytes` from that budget before it starts and holds it until its result is built. While the budget is spent, a new download is refused with `rate_limited: the server is downloading too many documents at once; try again in a minute, or ask for a smaller max_bytes`. A smaller `max_bytes` reserves less, so it is more likely to fit. Refusals are counted in the `companies_house_document_downloads_refused_total` metric (see [Operations](../operations.md#metrics)).

## Requests, redirects and retries

- A download makes two requests to the Document API with your key: the metadata, then the content. `metadata_only` makes only the first. Before each, the server checks your key's [rate-limit window](../conventions.md#rate-limit) as it does for every request.
- The Document API answers the content request with a redirect to Companies House's document storage. The server follows that one redirect itself, without your key, and only to an `https` address (or, for tests, plain `http` to a loopback address). A redirect anywhere else is not followed: `upstream: GET ... redirected to a location that is not https; not following it`.
- Each of these requests, and the storage download, is sent once more after a network error or a `5xx` answer.
- If Companies House answers that it cannot provide the document in that format (HTTP 406), the tool returns `validation: Companies House cannot provide that document in the requested format (HTTP 406); check its formats with metadata_only`.

## Live API only

Companies House's Document API has no sandbox. The server therefore uses the live Document API, `https://document-api.company-information.service.gov.uk`, only when `COMPANIES_HOUSE_API_URL` is the live API (the default). With the sandbox, or any other API URL such as a proxy, downloads are off unless `COMPANIES_HOUSE_DOCUMENT_API_URL` is set. The tool is still listed, and answers:

```text
validation: filed documents cannot be downloaded on this server: Companies House's Document API has no sandbox, so documents are only available on a server using the live API
```

`COMPANIES_HOUSE_DOCUMENT_API_URL` follows the same rules as `COMPANIES_HOUSE_API_URL`: `https`, or `http` only to a loopback address, with no credentials, query or fragment. See the [configuration reference](../configuration.md#companies).
