mod support;
use base64::Engine as _;
use mcp_companies_house_data::api::{ApiSettings, ChClient};
use mcp_companies_house_data::tools::CompaniesServer;
use rmcp::model::ResourceContents;
use serde_json::json;
use std::sync::Arc;
use support::fake_ch::{FakeCh, FakeState, Injected, auth_header};
use support::tools::{auth, error_text, json, params, server, settings};

async fn fake() -> FakeCh {
    FakeCh::start(FakeState::register()).await
}

fn resource(r: &rmcp::model::CallToolResult) -> ResourceContents {
    r.content[1]
        .as_resource()
        .expect("second content block is the document")
        .resource
        .clone()
}

#[tokio::test]
async fn a_pdf_comes_back_as_an_embedded_blob_fetched_without_the_key() {
    let fake = fake().await;
    let r = server(&fake)
        .download_filing_document(params(json!({"document_id": "doc-tx4"})), auth())
        .await
        .unwrap();
    let about = json(&r);
    assert_eq!(about["document_id"], "doc-tx4");
    assert_eq!(about["format"], "pdf");
    assert_eq!(about["content_type"], "application/pdf");
    assert_eq!(about["pages"], 3);
    let want = b"%PDF-1.4\n\xff\xfe fake micro-entity accounts\n%%EOF";
    assert_eq!(about["size_bytes"], want.len());
    match resource(&r) {
        ResourceContents::BlobResourceContents {
            uri,
            mime_type,
            blob,
            ..
        } => {
            assert_eq!(mime_type.as_deref(), Some("application/pdf"));
            assert!(uri.ends_with("/docapi/document/doc-tx4/content"), "{uri}");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(blob)
                .unwrap();
            assert_eq!(bytes, want);
        }
        other => panic!("expected a blob, got {other:?}"),
    }
    let log = fake.requests();
    let paths: Vec<&str> = log.iter().map(|l| l.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "/docapi/document/doc-tx4",
            "/docapi/document/doc-tx4/content",
            "/storage/doc-tx4"
        ]
    );
    let expected = auth_header();
    assert_eq!(log[0].authorization.as_deref(), Some(expected.as_str()));
    assert_eq!(log[1].authorization.as_deref(), Some(expected.as_str()));
    assert_eq!(log[1].accept.as_deref(), Some("application/pdf"));
    assert_eq!(
        log[2].authorization, None,
        "the API key must not go to document storage"
    );
}

#[tokio::test]
async fn text_formats_come_back_as_text_and_links_work_as_ids() {
    let fake = fake().await;
    let r = server(&fake)
        .download_filing_document(
            params(json!({
                "document_id": "https://frontend-doc-api.company-information.service.gov.uk/document/doc-tx4",
                "format": "xhtml"
            })),
            auth(),
        )
        .await
        .unwrap();
    assert_eq!(json(&r)["format"], "xhtml");
    match resource(&r) {
        ResourceContents::TextResourceContents {
            text, mime_type, ..
        } => {
            assert_eq!(text, "<html><body>Micro-entity accounts</body></html>");
            assert_eq!(mime_type.as_deref(), Some("application/xhtml+xml"));
        }
        other => panic!("expected text, got {other:?}"),
    }
    assert_eq!(
        fake.requests()[1].accept.as_deref(),
        Some("application/xhtml+xml")
    );
}

#[tokio::test]
async fn metadata_only_lists_formats_without_downloading() {
    let fake = fake().await;
    let v = json(
        &server(&fake)
            .download_filing_document(
                params(json!({"document_id": "/document/doc-tx4", "metadata_only": true})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(v["company_number"], "00000001");
    let formats: Vec<&str> = v["formats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["format"].as_str().unwrap())
        .collect();
    assert_eq!(formats, vec!["pdf", "xhtml"]);
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn a_format_the_document_is_not_held_in_is_refused_with_the_ones_it_is() {
    let fake = fake().await;
    let e = error_text(
        &server(&fake)
            .download_filing_document(
                params(json!({"document_id": "doc-tx4", "format": "xml"})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert!(e.starts_with("validation: "), "{e}");
    assert!(e.contains("not held as xml"), "{e}");
    assert!(e.contains("pdf ("), "{e}");
    assert!(e.contains("xhtml ("), "{e}");
    assert_eq!(fake.requests().len(), 1, "nothing is downloaded");
}

#[tokio::test]
async fn a_document_over_max_bytes_is_refused_before_downloading() {
    let fake = fake().await;
    let e = error_text(
        &server(&fake)
            .download_filing_document(params(json!({"document_id": "doc-big"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.contains("more than max_bytes (1048576)"), "{e}");
    assert_eq!(fake.requests().len(), 1, "nothing is downloaded");

    // Raising max_bytes downloads it.
    let r = server(&fake)
        .download_filing_document(
            params(json!({"document_id": "doc-big", "max_bytes": 3 * 1024 * 1024})),
            auth(),
        )
        .await
        .unwrap();
    assert_eq!(json(&r)["size_bytes"], 2 * 1024 * 1024);
}

#[tokio::test]
async fn a_document_without_a_size_is_cut_off_at_max_bytes() {
    let fake = fake().await;
    let e = error_text(
        &server(&fake)
            .download_filing_document(
                params(json!({"document_id": "doc-nosize", "max_bytes": 1000})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert!(e.contains("larger than max_bytes (1000)"), "{e}");
}

#[tokio::test]
async fn unknown_and_malformed_documents_are_reported() {
    let fake = fake().await;
    let e = error_text(
        &server(&fake)
            .download_filing_document(params(json!({"document_id": "doc-nope"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.starts_with("not_found: no document doc-nope"), "{e}");
    for bad in ["../x", "a b", "/elsewhere/x"] {
        let e = error_text(
            &server(&fake)
                .download_filing_document(params(json!({"document_id": bad})), auth())
                .await
                .unwrap(),
        );
        assert!(e.starts_with("validation: "), "{bad}: {e}");
    }
    let e = error_text(
        &server(&fake)
            .download_filing_document(
                params(json!({"document_id": "doc-tx4", "max_bytes": 0})),
                auth(),
            )
            .await
            .unwrap(),
    );
    assert!(e.contains("max_bytes must be between 1 and"), "{e}");
}

#[tokio::test]
async fn a_redirect_off_https_is_not_followed() {
    let fake = fake().await;
    let srv = server(&fake);
    // The metadata, then a redirect of the content to plain http.
    fake.inject(Injected::json(
        200,
        json!({"resources": {"application/pdf": {"content_length": 10}}}),
    ));
    fake.inject(Injected::status(302).header("location", "http://storage.example.com/doc-tx4"));
    let e = error_text(
        &srv.download_filing_document(params(json!({"document_id": "doc-tx4"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.starts_with("upstream: "), "{e}");
    assert!(e.contains("not https"), "{e}");
    assert_eq!(fake.requests().len(), 2, "the redirect is not followed");
}

#[tokio::test]
async fn servers_without_a_document_api_say_so() {
    let fake = fake().await;
    let mut s = settings(&fake);
    s.documents = None;
    let srv = CompaniesServer::new(Arc::new(ChClient::new(s).unwrap()));
    let e = error_text(
        &srv.download_filing_document(params(json!({"document_id": "doc-tx4"})), auth())
            .await
            .unwrap(),
    );
    assert!(e.contains("Document API has no sandbox"), "{e}");
    assert!(fake.requests().is_empty());
}

#[test]
fn only_the_live_api_gets_the_live_document_api_by_default() {
    let live = ApiSettings::new(
        url::Url::parse("https://api.company-information.service.gov.uk").unwrap(),
    );
    assert_eq!(
        live.documents.unwrap().as_str(),
        "https://document-api.company-information.service.gov.uk/"
    );
    let sandbox = ApiSettings::new(
        url::Url::parse("https://api-sandbox.company-information.service.gov.uk").unwrap(),
    );
    assert!(sandbox.documents.is_none());
}

#[tokio::test]
async fn downloads_are_refused_while_the_servers_document_budget_is_spent() {
    let fake = fake().await;
    let client = support::tools::client(&fake);
    let srv = CompaniesServer::new(client.clone());
    let held = client
        .reserve_document(mcp_companies_house_data::api::DOCUMENT_BUDGET_BYTES)
        .unwrap();
    let e = error_text(
        &srv.download_filing_document(params(json!({"document_id": "doc-tx4"})), auth())
            .await
            .unwrap(),
    );
    assert!(
        e.starts_with("rate_limited: the server is downloading too many documents"),
        "{e}"
    );
    drop(held);
    let r = srv
        .download_filing_document(params(json!({"document_id": "doc-tx4"})), auth())
        .await
        .unwrap();
    assert_eq!(json(&r)["format"], "pdf");
}

#[tokio::test]
async fn a_failed_content_request_is_sent_once_more() {
    let fake = fake().await;
    // The metadata answers normally, the content request fails once, then succeeds.
    fake.inject(Injected::json(
        200,
        json!({"resources": {"application/pdf": {"content_length": 10}}}),
    ));
    fake.inject(Injected::status(503));
    let r = server(&fake)
        .download_filing_document(params(json!({"document_id": "doc-tx4"})), auth())
        .await
        .unwrap();
    assert_eq!(json(&r)["format"], "pdf");
    let paths: Vec<String> = fake.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(
        paths,
        vec![
            "/docapi/document/doc-tx4",
            "/docapi/document/doc-tx4/content",
            "/docapi/document/doc-tx4/content",
            "/storage/doc-tx4"
        ]
    );
}
