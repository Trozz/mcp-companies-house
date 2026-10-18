pub mod authorize;
pub mod metadata;
pub mod middleware;
pub mod register;
pub mod token;

pub use middleware::require_bearer;

use crate::credential::{CheckCache, CheckRunner, CredentialVerifier, Sealer};
use crate::guards::{LoginLimiter, ReplayGuard};
use crate::jwt::Jwt;
use axum::Router;
use axum::routing::{get, post};
use secrecy::{ExposeSecret, SecretString};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

pub struct OAuthSettings {
    pub public_url: String,
    pub signing_key: SecretString,
    pub trusted_proxies: Vec<IpAddr>,
}

/// How long a bearer credential found valid upstream is trusted before it is checked again.
pub const BEARER_VALID_FOR: Duration = Duration::from_secs(600);
/// How long a rejected bearer credential is remembered.
pub const BEARER_INVALID_FOR: Duration = Duration::from_secs(60);
const BEARER_CACHE_ENTRIES: usize = 10_000;
/// Upstream credential checks allowed to run at once, across sign-ins and bearer credentials.
pub const MAX_RUNNING_CHECKS: usize = 16;
/// Longest an upstream credential check may take before it counts as unavailable.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest a check waits for a free slot when [`MAX_RUNNING_CHECKS`] are already running.
pub const CHECK_SLOT_WAIT: Duration = Duration::from_secs(5);

pub struct AuthState {
    pub settings: OAuthSettings,
    pub jwt: Jwt,
    pub limiter: LoginLimiter,
    pub replay: ReplayGuard,
    /// Checks each person's credential with the upstream service.
    pub verifier: Arc<dyn CredentialVerifier>,
    /// Seals credentials into tokens.
    pub sealer: Sealer,
    /// Recent verdicts on credentials sent directly as bearer tokens.
    pub bearer_checks: CheckCache,
    /// Runs upstream checks: shared per credential, capped and timed out.
    pub checks: CheckRunner,
    services: Vec<String>,
}

impl AuthState {
    /// `services` are the enabled service names; each is the protected resource `PUBLIC_URL/<name>`.
    /// Tokens the authorization server keeps for itself (client, csrf, code, refresh) use the
    /// public URL as audience; access tokens use their resource.
    pub fn new(
        settings: OAuthSettings,
        services: &[&str],
        verifier: Arc<dyn CredentialVerifier>,
    ) -> Arc<Self> {
        let key = settings.signing_key.expose_secret().as_bytes();
        let jwt = Jwt::new(
            key,
            settings.public_url.clone(),
            settings.public_url.clone(),
        );
        let sealer = Sealer::new(key);
        Arc::new(Self {
            settings,
            jwt,
            limiter: LoginLimiter::new(5, Duration::from_secs(60)),
            replay: ReplayGuard::new(),
            verifier,
            sealer,
            bearer_checks: CheckCache::new(
                BEARER_VALID_FOR,
                BEARER_INVALID_FOR,
                BEARER_CACHE_ENTRIES,
            ),
            checks: CheckRunner::new(MAX_RUNNING_CHECKS, CHECK_TIMEOUT, CHECK_SLOT_WAIT),
            services: services.iter().map(|s| s.to_string()).collect(),
        })
    }

    pub fn services(&self) -> &[String] {
        &self.services
    }

    pub fn has_service(&self, service: &str) -> bool {
        self.services.iter().any(|s| s == service)
    }

    pub fn resource(&self, service: &str) -> String {
        format!("{}/{service}", self.settings.public_url)
    }

    pub fn resource_metadata_url(&self, service: &str) -> String {
        format!(
            "{}/.well-known/oauth-protected-resource/{service}",
            self.settings.public_url
        )
    }

    /// The enabled service whose resource URL is exactly `resource`.
    pub fn service_for_resource(&self, resource: &str) -> Option<&str> {
        self.services
            .iter()
            .find(|s| self.resource(s) == resource)
            .map(String::as_str)
    }
}

pub fn oauth_router(state: Arc<AuthState>) -> Router {
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource/{service}",
            get(metadata::protected_resource),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(metadata::authorization_server),
        )
        .route("/register", post(register::register))
        .route("/token", post(token::token))
        .route(
            "/authorize",
            get(authorize::get_authorize).post(authorize::post_authorize),
        )
        .with_state(state)
}
