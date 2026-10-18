pub mod authorize;
pub mod metadata;
pub mod middleware;
pub mod register;
pub mod token;

pub use middleware::require_bearer;

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
    pub admin_password: SecretString,
    pub static_token: Option<SecretString>,
    pub trusted_proxies: Vec<IpAddr>,
}

pub struct AuthState {
    pub settings: OAuthSettings,
    pub jwt: Jwt,
    pub limiter: LoginLimiter,
    pub replay: ReplayGuard,
    services: Vec<String>,
}

impl AuthState {
    /// `services` are the enabled service names; each is the protected resource `PUBLIC_URL/<name>`.
    /// Tokens the authorization server keeps for itself (client, csrf, code, refresh) use the
    /// public URL as audience; access tokens use their resource.
    pub fn new(settings: OAuthSettings, services: &[&str]) -> Arc<Self> {
        let jwt = Jwt::new(
            settings.signing_key.expose_secret().as_bytes(),
            settings.public_url.clone(),
            settings.public_url.clone(),
        );
        Arc::new(Self {
            settings,
            jwt,
            limiter: LoginLimiter::new(5, Duration::from_secs(60)),
            replay: ReplayGuard::new(),
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
