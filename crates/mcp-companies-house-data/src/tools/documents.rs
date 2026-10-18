//! Filed documents, from Companies House's Document API.

use super::{CompaniesServer, run};
use crate::api::MAX_DOCUMENT_BYTES;
use crate::error::{self, ChError};
use crate::view::records::{DOCUMENT_FORMATS, document_metadata, format_name};
use crate::view::segment_after;
use axum::http::request::Parts;
use base64::Engine as _;
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ResourceContents};
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

/// Largest document downloaded when `max_bytes` is not given.
pub const DEFAULT_DOCUMENT_BYTES: u32 = 1024 * 1024;

/// A format `download_filing_document` can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormat {
    Pdf,
    Xhtml,
    Xml,
    Json,
}

impl DocumentFormat {
    fn name(self) -> &'static str {
        match self {
            DocumentFormat::Pdf => "pdf",
            DocumentFormat::Xhtml => "xhtml",
            DocumentFormat::Xml => "xml",
            DocumentFormat::Json => "json",
        }
    }

    fn content_type(self) -> &'static str {
        DOCUMENT_FORMATS
            .iter()
            .find(|(f, _)| *f == self.name())
            .map(|(_, t)| *t)
            .expect("every format has a content type")
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct DownloadDocumentParams {
    /// document_id from list_filing_history or get_filing. A document_metadata link (a URL or a /document/{id} path) also works.
    pub document_id: String,
    /// pdf (default), xhtml, xml or json. Most filings are only held as pdf; electronically filed accounts are often also xhtml (iXBRL). metadata_only lists what a document is held as.
    pub format: Option<DocumentFormat>,
    /// Largest document to download, in bytes, up to 10485760 (10 MiB). Default: 1048576 (1 MiB). A larger document is refused before it is downloaded.
    pub max_bytes: Option<u32>,
    /// true: return only the document's metadata (pages, formats and their sizes), without downloading it.
    pub metadata_only: Option<bool>,
}

/// The document id in a bare id, a `/document/{id}` path or a document metadata URL.
pub(crate) fn document_id_arg(raw: &str) -> error::Result<String> {
    let raw = raw.trim();
    let id = if raw.contains('/') {
        segment_after(raw, "document").unwrap_or_default()
    } else {
        raw.to_string()
    };
    super::id_arg("document_id", &id).map_err(|_| {
        ChError::invalid(
            "document_id must be a document_id from list_filing_history or get_filing, or its document_metadata link",
        )
    })
}

fn max_bytes_arg(max_bytes: Option<u32>) -> error::Result<usize> {
    let max = max_bytes.unwrap_or(DEFAULT_DOCUMENT_BYTES) as usize;
    if max == 0 || max > MAX_DOCUMENT_BYTES {
        return Err(ChError::invalid(format!(
            "max_bytes must be between 1 and {MAX_DOCUMENT_BYTES}"
        )));
    }
    Ok(max)
}

/// The formats a document is held in, for messages: `pdf (52344 bytes), xhtml (10230 bytes)`.
fn held_formats(metadata: &Value) -> String {
    let formats: Vec<String> = metadata
        .get("formats")
        .and_then(Value::as_array)
        .map(|fs| {
            fs.iter()
                .filter_map(|f| {
                    let name = f.get("format")?.as_str()?;
                    Some(match f.get("size_bytes").and_then(Value::as_u64) {
                        Some(n) => format!("{name} ({n} bytes)"),
                        None => name.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if formats.is_empty() {
        "none".into()
    } else {
        formats.join(", ")
    }
}

/// The size Companies House gives for `content_type`: `None` when the document is not held in
/// that format, `Some(None)` when it is but no size is given.
fn held_size(raw: &Value, content_type: &str) -> Option<Option<u64>> {
    let resources = raw.get("resources")?.as_object()?;
    let (_, r) = resources
        .iter()
        .find(|(t, _)| t.eq_ignore_ascii_case(content_type))?;
    Some(r.get("content_length").and_then(Value::as_u64))
}

#[tool_router(router = documents_router, vis = "pub(crate)")]
impl CompaniesServer {
    #[tool(
        description = "Download a filed document (for example a set of accounts or a confirmation statement) from Companies House's Document API. Takes document_id from list_filing_history or get_filing. Returns the document's metadata, then the document: a PDF as an embedded resource (base64), xhtml, xml and json as text. Checks the document's formats and size first: a format it is not held in, or a document larger than max_bytes, is refused before downloading. metadata_only returns the pages, formats and sizes without the document. Only available on servers using the live API (the sandbox has no documents).",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = true)
    )]
    pub async fn download_filing_document(
        &self,
        Parameters(p): Parameters<DownloadDocumentParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(args = ?p, "download_filing_document");
        run("download_filing_document", async {
            let c = self.caller(&parts)?;
            let id = document_id_arg(&p.document_id)?;
            let format = p.format.unwrap_or(DocumentFormat::Pdf);
            let max_bytes = max_bytes_arg(p.max_bytes)?;
            let raw = match c.client.document_metadata(&c.key, &id).await {
                Ok(v) if !v.is_null() => v,
                Ok(_) | Err(ChError::NotFound(_)) => {
                    return Err(ChError::NotFound(format!(
                        "no document {id}; take document_id from list_filing_history or get_filing"
                    )));
                }
                Err(e) => return Err(e),
            };
            let metadata = document_metadata(&id, &raw);
            if p.metadata_only == Some(true) {
                return Ok(super::json_result(&metadata));
            }
            let content_type = format.content_type();
            let size = held_size(&raw, content_type).ok_or_else(|| {
                ChError::invalid(format!(
                    "document {id} is not held as {}; it is held as: {}",
                    format.name(),
                    held_formats(&metadata)
                ))
            })?;
            if let Some(size) = size
                && size > max_bytes as u64
            {
                return Err(ChError::invalid(format!(
                    "document {id} is {size} bytes as {}, more than max_bytes ({max_bytes}); raise max_bytes (up to {MAX_DOCUMENT_BYTES}) or choose another format: {}",
                    format.name(),
                    held_formats(&metadata)
                )));
            }
            // Held until the result is built, so the bytes and their encoding count against the
            // server's budget.
            let _permit = c.client.reserve_document(max_bytes)?;
            let doc = c
                .client
                .document_content(
                    &c.key,
                    &id,
                    content_type,
                    max_bytes,
                    size.map(|n| n as usize),
                )
                .await?;
            let uri = c.client.document_url(&id, true)?.to_string();
            let mut about = metadata;
            if let Value::Object(map) = &mut about {
                map.insert("format".into(), json!(format_name(&doc.content_type)));
                map.insert("content_type".into(), json!(doc.content_type));
                map.insert("size_bytes".into(), json!(doc.bytes.len()));
            }
            let blob = |bytes: Vec<u8>, uri: String| {
                ResourceContents::blob(base64::engine::general_purpose::STANDARD.encode(bytes), uri)
            };
            // Text formats are text unless they are not UTF-8, which only a blob can carry.
            let body = if format == DocumentFormat::Pdf {
                blob(doc.bytes, uri)
            } else {
                match String::from_utf8(doc.bytes) {
                    Ok(text) => ResourceContents::text(text, uri),
                    Err(e) => blob(e.into_bytes(), uri),
                }
            }
            .with_mime_type(doc.content_type);
            let about = serde_json::to_string_pretty(&about).unwrap_or_else(|_| about.to_string());
            Ok(CallToolResult::success(vec![
                ContentBlock::text(about),
                ContentBlock::resource(body),
            ]))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_ids_come_from_ids_paths_and_links() {
        for (raw, want) in [
            ("doc-abc_1", "doc-abc_1"),
            (" doc-abc ", "doc-abc"),
            ("/document/doc-abc", "doc-abc"),
            ("/document/doc-abc/content", "doc-abc"),
            (
                "https://frontend-doc-api.company-information.service.gov.uk/document/doc-abc",
                "doc-abc",
            ),
            (
                "https://document-api.company-information.service.gov.uk/document/doc-abc?x=1",
                "doc-abc",
            ),
        ] {
            assert_eq!(document_id_arg(raw).unwrap(), want, "{raw}");
        }
        for bad in [
            "",
            "/document/",
            "/other/x",
            "doc abc",
            "../x",
            "a/b",
            "doc?x",
        ] {
            assert_eq!(
                document_id_arg(bad).unwrap_err().kind(),
                "validation",
                "{bad}"
            );
        }
    }

    #[test]
    fn max_bytes_is_bounded() {
        assert_eq!(
            max_bytes_arg(None).unwrap(),
            DEFAULT_DOCUMENT_BYTES as usize
        );
        assert_eq!(
            max_bytes_arg(Some(MAX_DOCUMENT_BYTES as u32)).unwrap(),
            MAX_DOCUMENT_BYTES
        );
        assert_eq!(max_bytes_arg(Some(0)).unwrap_err().kind(), "validation");
        assert_eq!(
            max_bytes_arg(Some(MAX_DOCUMENT_BYTES as u32 + 1))
                .unwrap_err()
                .kind(),
            "validation"
        );
    }

    #[test]
    fn formats_map_to_content_types() {
        assert_eq!(DocumentFormat::Pdf.content_type(), "application/pdf");
        assert_eq!(
            DocumentFormat::Xhtml.content_type(),
            "application/xhtml+xml"
        );
        let raw = json!({"resources": {"application/pdf": {"content_length": 10}, "application/xml": {}}});
        assert_eq!(held_size(&raw, "application/pdf"), Some(Some(10)));
        assert_eq!(held_size(&raw, "application/xml"), Some(None));
        assert_eq!(held_size(&raw, "application/json"), None);
    }
}
