//! Checks a Companies House API key when a person signs in or sends it as a bearer token.

use crate::api::ChClient;
use crate::error::ChError;
use mcp_server_core::credential::{CheckFuture, Credential, CredentialCheck, CredentialVerifier};
use std::sync::Arc;

pub struct ChKeyVerifier {
    client: Arc<ChClient>,
}

impl ChKeyVerifier {
    pub fn new(client: Arc<ChClient>) -> Self {
        ChKeyVerifier { client }
    }
}

/// The verdict for the outcome of one cheap authenticated request made with the key.
pub fn verdict(result: Result<(), ChError>) -> CredentialCheck {
    match result {
        // Any answer other than an authentication failure means the key was accepted.
        Ok(()) | Err(ChError::NotFound(_)) | Err(ChError::Validation(_)) => CredentialCheck::Valid,
        Err(ChError::Auth) => CredentialCheck::Invalid(None),
        Err(e @ ChError::KeyRestricted) => CredentialCheck::Invalid(Some(e.to_string())),
        Err(ChError::RateLimited { reset_in_secs }) => {
            CredentialCheck::Unavailable(match reset_in_secs {
                Some(s) => format!(
                    "this key's Companies House rate limit is used up; try again in {s} seconds"
                ),
                None => {
                    "this key's Companies House rate limit is used up; try again in a few minutes"
                        .into()
                }
            })
        }
        Err(e) => CredentialCheck::Unavailable(format!(
            "Companies House did not answer as expected ({})",
            e.kind()
        )),
    }
}

impl CredentialVerifier for ChKeyVerifier {
    fn label(&self) -> &'static str {
        "Companies House API key"
    }

    fn help(&self) -> &'static str {
        "A REST API key from the Companies House Developer Hub (developer.company-information.service.gov.uk): open Your applications, create an application, then Create new key and choose REST. Keys from a test application only work if this server uses the sandbox."
    }

    fn check<'a>(&'a self, credential: &'a str) -> CheckFuture<'a> {
        Box::pin(async move {
            let key = Credential::new(credential);
            let result = self
                .client
                .get(
                    &key,
                    "key_check",
                    &["search", "companies"],
                    &[
                        ("q", "companies house".to_string()),
                        ("items_per_page", "2".to_string()),
                    ],
                )
                .await
                .map(|_| ());
            verdict(result)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_follow_the_upstream_answer() {
        assert_eq!(verdict(Ok(())), CredentialCheck::Valid);
        assert_eq!(
            verdict(Err(ChError::NotFound("x".into()))),
            CredentialCheck::Valid
        );
        assert_eq!(verdict(Err(ChError::Auth)), CredentialCheck::Invalid(None));
        assert!(matches!(
            verdict(Err(ChError::KeyRestricted)),
            CredentialCheck::Invalid(Some(m)) if m.contains("IP or domain restrictions")
        ));
        assert!(matches!(
            verdict(Err(ChError::RateLimited { reset_in_secs: Some(42) })),
            CredentialCheck::Unavailable(m) if m.contains("42 seconds")
        ));
        assert!(matches!(
            verdict(Err(ChError::Upstream("GET /x returned HTTP 503".into()))),
            CredentialCheck::Unavailable(m) if !m.contains("/x")
        ));
    }
}
