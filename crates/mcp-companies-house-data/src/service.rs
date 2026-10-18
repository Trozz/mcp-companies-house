use crate::api::{ApiSettings, ChClient};
use crate::config::DataConfig;
use crate::tools::CompaniesServer;
use crate::verifier::ChKeyVerifier;
use mcp_server_core::combined::DynHandler;
use mcp_server_core::credential::CredentialVerifier;
use mcp_server_core::service::{Service, ServiceContext, ServiceEnv, mcp_service};
use std::sync::Arc;

/// The company data service, mounted by the server at `/companies`.
pub struct DataService {
    client: Arc<ChClient>,
}

impl DataService {
    pub fn new(client: Arc<ChClient>) -> Self {
        Self { client }
    }

    fn server(&self) -> CompaniesServer {
        CompaniesServer::new(self.client.clone())
    }
}

impl Service for DataService {
    fn name(&self) -> &'static str {
        "companies"
    }

    fn tool_names(&self) -> Vec<String> {
        self.server().tool_names()
    }

    fn router(self: Arc<Self>, ctx: &ServiceContext) -> axum::Router {
        tracing::info!(
            api = %self.client.base().origin().ascii_serialization(),
            documents = %self
                .client
                .documents()
                .map(|d| d.origin().ascii_serialization())
                .unwrap_or_else(|| "none".into()),
            "companies house settings"
        );
        mcp_service(ctx, move || Ok(self.server()))
    }

    fn credential_verifier(&self) -> Option<Arc<dyn CredentialVerifier>> {
        Some(Arc::new(ChKeyVerifier::new(self.client.clone())))
    }

    fn handler(&self) -> DynHandler {
        DynHandler::new(self.server())
    }
}

/// Reads the company data configuration and builds the service. Errors go to `env.errors`.
pub fn from_env(env: &mut ServiceEnv) -> Option<Arc<dyn Service>> {
    let cfg = DataConfig::from_env(env)?;
    let mut settings = ApiSettings::new(cfg.api_url);
    if let Some(documents) = cfg.document_api_url {
        settings.documents = Some(documents);
    }
    let client = match ChClient::new(settings) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            env.errors
                .push(format!("Companies House client setup failed: {e}"));
            return None;
        }
    };
    Some(Arc::new(DataService::new(client)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> ServiceEnv {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ServiceEnv::new(Box::new(move |k: &str| map.get(k).cloned()))
    }

    #[test]
    fn builds_from_env() {
        let mut e = env(&[]);
        let svc = from_env(&mut e).expect("service");
        assert!(e.errors.is_empty(), "{:?}", e.errors);
        assert_eq!(svc.name(), "companies");
        assert!(svc.log_caps().is_empty());
        let verifier = svc.credential_verifier().expect("a key verifier");
        assert_eq!(verifier.label(), "Companies House API key");
        assert_eq!(
            svc.tool_names(),
            vec![
                "browse_company_names",
                "check_company_name_availability",
                "company_report",
                "download_filing_document",
                "due_diligence_check",
                "filter_companies",
                "get_charge",
                "get_company",
                "get_company_registers",
                "get_disqualified_officer",
                "get_exemptions",
                "get_filing",
                "get_insolvency",
                "get_officer_appointments",
                "get_uk_establishments",
                "list_charges",
                "list_filing_history",
                "list_officers",
                "list_persons_with_significant_control",
                "lookup_sic_codes",
                "officer_network",
                "search_companies",
                "search_disqualified_officers",
                "search_dissolved_companies",
                "search_officers",
            ]
        );
    }

    #[test]
    fn missing_config_is_reported_and_no_service_is_built() {
        let mut e = env(&[("COMPANIES_HOUSE_API_URL", "ftp://x")]);
        assert!(from_env(&mut e).is_none());
        let joined = e.errors.join("\n");
        assert!(joined.contains("COMPANIES_HOUSE_API_URL"), "{joined}");
    }
}
