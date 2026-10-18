use super::AuthState;
use super::authorize::client_ip;
use crate::credential::{self, Credential, CredentialCheck};
use crate::guards::Reservation;
use crate::jwt::TokenKind;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::SocketAddr;
use std::sync::Arc;

fn unauthorized(st: &AuthState, service: &str, token_present: bool) -> Response {
    let mut value = format!(
        "Bearer resource_metadata=\"{}\"",
        st.resource_metadata_url(service)
    );
    if token_present {
        value.push_str(", error=\"invalid_token\"");
    }
    let mut res = StatusCode::UNAUTHORIZED.into_response();
    if let Ok(v) = HeaderValue::from_str(&value) {
        res.headers_mut().insert(header::WWW_AUTHENTICATE, v);
    }
    res
}

fn unavailable(st: &AuthState, why: &str) -> Response {
    let body = format!(
        "Your {} could not be checked right now: {why}",
        st.verifier.label()
    );
    let mut res = (StatusCode::SERVICE_UNAVAILABLE, body).into_response();
    res.headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("30"));
    res
}

/// The token of an `Authorization: Bearer <token>` value. The scheme is case-insensitive.
fn bearer_token(value: &str) -> Option<&str> {
    let (scheme, rest) = value.trim().split_once(|c: char| c.is_ascii_whitespace())?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| rest.trim())
        .filter(|t| !t.is_empty())
}

/// An OAuth token from this server is a JWT; a credential sent directly is not.
fn looks_like_jwt(token: &str) -> bool {
    token.split('.').count() == 3
}

/// Guards one service path. It accepts either an OAuth access token issued for this service,
/// whose sealed credential is opened, or the person's credential itself as the bearer token,
/// which is checked upstream (the verdict is cached briefly under its fingerprint). Either way
/// the request goes on with a [`Credential`] extension; an access token also adds its claims.
pub async fn require_bearer(
    State((st, service)): State<(Arc<AuthState>, String)>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(bearer_token)
        .map(str::to_owned);
    let Some(token) = token.filter(|t| !t.is_empty()) else {
        return unauthorized(&st, &service, false);
    };

    if looks_like_jwt(&token) {
        let Ok(claims) = st
            .jwt
            .verify_for(&token, TokenKind::Access, &st.resource(&service))
        else {
            return unauthorized(&st, &service, true);
        };
        let Some(credential) = claims
            .payload
            .credential
            .as_deref()
            .and_then(|sealed| st.sealer.open(sealed).ok())
        else {
            return unauthorized(&st, &service, true);
        };
        req.extensions_mut().insert(credential);
        req.extensions_mut().insert(claims);
        return next.run(req).await;
    }

    if !st.verifier.plausible(&token) {
        return unauthorized(&st, &service, true);
    }
    let fingerprint = credential::fingerprint(&token);
    match st.bearer_checks.get(&fingerprint) {
        Some(true) => {}
        Some(false) => return unauthorized(&st, &service, true),
        None => {
            let peer = req
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|c| c.0.ip());
            let ip = client_ip(peer, req.headers(), &st.settings.trusted_proxies);
            // A request joining a check that is already running shares its verdict and is not
            // counted again; any other attempt counts before it goes upstream, so concurrent
            // attempts cannot get past the limit.
            let reservation: Option<Reservation> = if st.checks.is_running(&fingerprint) {
                None
            } else {
                match st.limiter.try_reserve(ip) {
                    Some(r) => Some(r),
                    None => {
                        metrics::counter!("bearer_credential_checks_total", "outcome" => "rate_limited")
                            .increment(1);
                        return (
                            StatusCode::TOO_MANY_REQUESTS,
                            "Too many rejected credentials. Wait a minute and try again.",
                        )
                            .into_response();
                    }
                }
            };
            match st.checks.check(st.verifier.as_ref(), &token).await {
                CredentialCheck::Valid => {
                    metrics::counter!("bearer_credential_checks_total", "outcome" => "valid")
                        .increment(1);
                    if let Some(r) = reservation {
                        st.limiter.refund(r);
                    }
                    st.bearer_checks.put(fingerprint, true);
                }
                CredentialCheck::Invalid(_) => {
                    metrics::counter!("bearer_credential_checks_total", "outcome" => "invalid")
                        .increment(1);
                    st.bearer_checks.put(fingerprint, false);
                    tracing::warn!(%ip, "bearer credential rejected");
                    return unauthorized(&st, &service, true);
                }
                CredentialCheck::Unavailable(why) => {
                    metrics::counter!("bearer_credential_checks_total", "outcome" => "unavailable")
                        .increment(1);
                    if let Some(r) = reservation {
                        st.limiter.refund(r);
                    }
                    tracing::warn!(reason = %why, "bearer credential could not be checked");
                    return unavailable(&st, &why);
                }
            }
        }
    }
    req.extensions_mut().insert(Credential::new(token));
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::bearer_token;

    #[test]
    fn the_bearer_scheme_is_case_insensitive() {
        for v in ["Bearer abc", "bearer abc", "BEARER  abc ", "Bearer\tabc"] {
            assert_eq!(bearer_token(v), Some("abc"), "{v:?}");
        }
        for v in ["Basic abc", "Bearer", "Bearer   ", "Bearerabc", ""] {
            assert_eq!(bearer_token(v), None, "{v:?}");
        }
    }
}
