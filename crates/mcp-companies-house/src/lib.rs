use mcp_server_core::config::CoreConfig;
use mcp_server_core::credential::CredentialVerifier;
use mcp_server_core::oauth::{AuthState, OAuthSettings};
use mcp_server_core::server::build_app;
use mcp_server_core::service::{Service, ServiceContext, ServiceEnv, ServiceFactory};
use std::sync::Arc;

pub const VERSION: &str = match option_env!("APP_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

pub const SERVICE_NAME: &str = "mcp-companies-house";
/// Tracing target prefix: also matches `mcp_companies_house_data` and every other
/// `mcp_companies_house_*` crate.
pub const CRATE_TARGET: &str = "mcp_companies_house";

/// Every service this binary can run, in mount order. Adding a service: a library crate
/// implementing `mcp_server_core::service::Service`, a dependency in Cargo.toml and one line here.
pub const SERVICES: &[(&str, ServiceFactory)] =
    &[("companies", mcp_companies_house_data::from_env)];

/// Resolves `COMPANIES_HOUSE_SERVICES` (comma list, unset means every registered service).
pub fn select<'a>(
    raw: Option<&str>,
    registry: &'a [(&'static str, ServiceFactory)],
) -> Result<Vec<&'a (&'static str, ServiceFactory)>, String> {
    let Some(raw) = raw else {
        return Ok(registry.iter().collect());
    };
    let known = || {
        registry
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut selected: Vec<&(&'static str, ServiceFactory)> = Vec::new();
    for name in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let Some(entry) = registry.iter().find(|(n, _)| *n == name) else {
            return Err(format!(
                "COMPANIES_HOUSE_SERVICES contains an unknown service: {name} (known: {})",
                known()
            ));
        };
        if !selected.iter().any(|(n, _)| *n == name) {
            selected.push(entry);
        }
    }
    if selected.is_empty() {
        return Err(format!(
            "COMPANIES_HOUSE_SERVICES must name at least one service (known: {})",
            known()
        ));
    }
    Ok(selected)
}

pub struct Loaded {
    pub core: CoreConfig,
    pub services: Vec<Arc<dyn Service>>,
}

/// The credential check people sign in with: the first enabled service that defines one.
pub fn verifier(services: &[Arc<dyn Service>]) -> Option<Arc<dyn CredentialVerifier>> {
    services.iter().find_map(|s| s.credential_verifier())
}

/// Reads the core configuration and builds every selected service. All errors are collected in
/// `env.errors`; `None` means the server cannot start. Unselected services never read their config.
pub fn load(env: &mut ServiceEnv, registry: &[(&'static str, ServiceFactory)]) -> Option<Loaded> {
    let core = CoreConfig::from_env(env, SERVICE_NAME, CRATE_TARGET);
    let raw = env.optional("COMPANIES_HOUSE_SERVICES");
    let selected = match select(raw.as_deref(), registry) {
        Ok(s) => s,
        Err(e) => {
            env.errors.push(e);
            Vec::new()
        }
    };
    let mut services = Vec::new();
    for (_, factory) in selected {
        if let Some(s) = factory(env) {
            services.push(s);
        }
    }
    if !services.is_empty() && verifier(&services).is_none() {
        env.errors
            .push("no enabled service defines how people sign in".to_string());
    }
    // Services may share variables (COMPANIES_HOUSE_API_URL): report each error once.
    let mut seen = std::collections::HashSet::new();
    env.errors.retain(|e| seen.insert(e.clone()));
    if !env.errors.is_empty() {
        return None;
    }
    Some(Loaded {
        core: core?,
        services,
    })
}

/// The full HTTP application: health, favicon, OAuth routes and each service at `/<name>`, every
/// service its own OAuth protected resource.
pub fn app(
    oauth: OAuthSettings,
    services: &[Arc<dyn Service>],
    ctx: &ServiceContext,
) -> axum::Router {
    let names: Vec<&str> = services.iter().map(|s| s.name()).collect();
    let verifier = verifier(services).expect("load() requires a service that checks credentials");
    let auth = AuthState::new(oauth, &names, verifier);
    let routers = services
        .iter()
        .map(|s| (s.name(), s.clone().router(ctx)))
        .collect();
    build_app(auth, routers).route(
        "/favicon.ico",
        axum::routing::get(|| async {
            (
                [(axum::http::header::CONTENT_TYPE, "image/png")],
                include_bytes!("../assets/favicon.png").as_slice(),
            )
        }),
    )
}

/// Drops any user directive for a capped target and caps it at info, for dependencies that log
/// secrets at trace. Directives with a `[` are dropped too: span-scoped ones could re-enable a
/// capped target.
pub fn log_filter(level: &str, caps: &[&str]) -> String {
    let mut parts: Vec<String> = level
        .split(',')
        .map(str::trim)
        .filter(|d| !d.is_empty() && !d.contains('[') && !caps.iter().any(|c| d.starts_with(c)))
        .map(String::from)
        .collect();
    parts.extend(caps.iter().map(|c| format!("{c}=info")));
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_server_core::service::ServiceContext;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> ServiceEnv {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ServiceEnv::new(Box::new(move |k: &str| map.get(k).cloned()))
    }

    const CORE: &[(&str, &str)] = &[
        ("PUBLIC_URL", "https://ch.example.com"),
        ("OAUTH_SIGNING_KEY", "0123456789abcdef0123456789abcdef"),
    ];

    fn with_core(extra: &[(&str, &str)]) -> ServiceEnv {
        let mut all = CORE.to_vec();
        all.extend_from_slice(extra);
        env(&all)
    }

    fn needy(env: &mut ServiceEnv) -> Option<Arc<dyn Service>> {
        env.required("NEEDY_TOKEN");
        None
    }

    /// Two services reading the same variable, like a future streaming service would.
    fn sharing(env: &mut ServiceEnv) -> Option<Arc<dyn Service>> {
        env.required("SHARED_TOKEN");
        None
    }

    /// A service that defines no way to sign in.
    struct Plain;

    impl Service for Plain {
        fn name(&self) -> &'static str {
            "plain"
        }
        fn tool_names(&self) -> Vec<String> {
            Vec::new()
        }
        fn router(self: Arc<Self>, _: &ServiceContext) -> axum::Router {
            axum::Router::new()
        }
    }

    fn plain(_: &mut ServiceEnv) -> Option<Arc<dyn Service>> {
        Some(Arc::new(Plain))
    }

    const TWO: &[(&str, ServiceFactory)] = &[
        ("companies", mcp_companies_house_data::from_env),
        ("needy", needy),
    ];

    fn names(v: &[&(&'static str, ServiceFactory)]) -> Vec<&'static str> {
        v.iter().map(|(n, _)| *n).collect()
    }

    #[test]
    fn unset_selects_every_registered_service() {
        assert_eq!(
            names(&select(None, TWO).unwrap()),
            vec!["companies", "needy"]
        );
    }

    #[test]
    fn explicit_selection_is_trimmed_and_deduplicated() {
        assert_eq!(
            names(&select(Some(" needy , companies,needy"), TWO).unwrap()),
            vec!["needy", "companies"]
        );
    }

    #[test]
    fn unknown_or_empty_selection_is_an_error() {
        let e = select(Some("companies,filing"), TWO).unwrap_err();
        assert_eq!(
            e,
            "COMPANIES_HOUSE_SERVICES contains an unknown service: filing (known: companies, needy)"
        );
        let e = select(Some(" , "), TWO).unwrap_err();
        assert!(
            e.starts_with("COMPANIES_HOUSE_SERVICES must name at least one service"),
            "{e}"
        );
    }

    #[test]
    fn unselected_services_do_not_read_config() {
        let mut e = with_core(&[("COMPANIES_HOUSE_SERVICES", "companies")]);
        let loaded = load(&mut e, TWO).expect("loads");
        assert!(e.errors.is_empty(), "{:?}", e.errors);
        assert_eq!(loaded.services.len(), 1);
        assert_eq!(loaded.services[0].name(), "companies");

        let mut e = with_core(&[]);
        assert!(load(&mut e, TWO).is_none());
        assert_eq!(e.errors, vec!["NEEDY_TOKEN is required".to_string()]);
    }

    #[test]
    fn core_and_service_errors_are_reported_together() {
        let mut e = env(&[
            ("PUBLIC_URL", "ftp://x"),
            ("COMPANIES_HOUSE_API_URL", "http://example.com"),
        ]);
        assert!(load(&mut e, SERVICES).is_none());
        let joined = e.errors.join("\n");
        assert!(joined.contains("PUBLIC_URL"), "{joined}");
        assert!(joined.contains("OAUTH_SIGNING_KEY is required"), "{joined}");
        assert!(joined.contains("COMPANIES_HOUSE_API_URL"), "{joined}");
    }

    #[test]
    fn selection_errors_are_reported_with_the_rest() {
        let mut e = with_core(&[("COMPANIES_HOUSE_SERVICES", "filing")]);
        assert!(load(&mut e, SERVICES).is_none());
        assert_eq!(
            e.errors,
            vec![
                "COMPANIES_HOUSE_SERVICES contains an unknown service: filing (known: companies)"
                    .to_string()
            ]
        );
    }

    #[test]
    fn shared_variable_errors_are_reported_once() {
        const SHARED: &[(&str, ServiceFactory)] = &[("one", sharing), ("two", sharing)];
        let mut e = with_core(&[]);
        assert!(load(&mut e, SHARED).is_none());
        assert_eq!(e.errors, vec!["SHARED_TOKEN is required".to_string()]);
    }

    #[test]
    fn a_service_must_define_how_people_sign_in() {
        const PLAIN: &[(&str, ServiceFactory)] = &[("plain", plain)];
        let mut e = with_core(&[]);
        assert!(load(&mut e, PLAIN).is_none());
        assert_eq!(
            e.errors,
            vec!["no enabled service defines how people sign in".to_string()]
        );
        let mut e = with_core(&[]);
        let loaded = load(&mut e, SERVICES).expect("loads");
        assert!(verifier(&loaded.services).is_some());
    }

    #[test]
    fn registry_names_match_service_names() {
        for (name, factory) in SERVICES {
            let mut e = with_core(&[]);
            let svc = factory(&mut e).unwrap_or_else(|| panic!("{name}: {:?}", e.errors));
            assert_eq!(svc.name(), *name);
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "{name} must be a lowercase path segment"
            );
        }
    }

    #[test]
    fn log_filter_caps_every_listed_target() {
        assert_eq!(
            log_filter("trace,a=trace,b::x=debug", &["a", "b"]),
            "trace,a=info,b=info"
        );
        assert_eq!(log_filter("warn", &[]), "warn");
        assert_eq!(
            log_filter("info,[{x}]=trace,[x]=trace,debug", &["a"]),
            "info,debug,a=info"
        );
    }

    #[test]
    fn debug_directive_covers_service_crates() {
        use tracing::Level;
        use tracing_subscriber::layer::SubscriberExt;
        let directives = mcp_server_core::observability::build_filter("info", true, CRATE_TARGET);
        let subscriber =
            tracing_subscriber::registry().with(tracing_subscriber::EnvFilter::new(directives));
        tracing::subscriber::with_default(subscriber, || {
            assert!(tracing::enabled!(target: "mcp_companies_house_data::api", Level::DEBUG));
            assert!(tracing::enabled!(target: "mcp_companies_house", Level::DEBUG));
            assert!(!tracing::enabled!(target: "hyper", Level::DEBUG));
        });
    }
}
