pub mod combined;
pub mod config;
pub mod credential;
pub mod guards;
pub mod jwt;
pub mod oauth;
pub mod observability;
pub mod pkce;
pub mod server;
pub mod service;
pub mod tls;

pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn shutdown_signal() -> tokio_util::sync::CancellationToken {
    let ct = tokio_util::sync::CancellationToken::new();
    let trigger = ct.clone();
    tokio::spawn(async move {
        let ctrl_c = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        let term = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut s) => {
                    s.recv().await;
                }
                Err(_) => std::future::pending::<()>().await,
            }
        };
        tokio::select! { _ = ctrl_c => {}, _ = term => {} }
        tracing::info!("shutdown signal received");
        trigger.cancel();
    });
    ct
}
