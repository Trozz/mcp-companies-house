#[derive(Debug, thiserror::Error)]
pub enum ChError {
    #[error(
        "Companies House rejected the API key. Check COMPANIES_HOUSE_API_KEY is a REST API key (not a stream key) for the environment COMPANIES_HOUSE_API_URL points at: sandbox keys only work against the sandbox"
    )]
    Auth,
    /// A complete message: tool errors are already prefixed with the kind, `not_found: `.
    #[error("{0}")]
    NotFound(String),
    #[error("{}", rate_limited_message(*.reset_in_secs))]
    RateLimited {
        /// Seconds until Companies House resets the window, when it said.
        reset_in_secs: Option<u64>,
    },
    #[error("{0}")]
    Validation(String),
    #[error("Companies House error: {0}")]
    Upstream(String),
    #[error("{0}")]
    Internal(String),
}

fn rate_limited_message(reset_in_secs: Option<u64>) -> String {
    let base = "the Companies House rate limit (600 requests per 5 minutes per API key) is used up";
    match reset_in_secs {
        Some(s) => format!("{base}; it resets in {s} seconds"),
        None => format!("{base}; try again in a few minutes"),
    }
}

impl ChError {
    pub fn kind(&self) -> &'static str {
        match self {
            ChError::Auth => "auth",
            ChError::NotFound(_) => "not_found",
            ChError::RateLimited { .. } => "rate_limited",
            ChError::Validation(_) => "validation",
            ChError::Upstream(_) => "upstream",
            ChError::Internal(_) => "internal",
        }
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        ChError::Validation(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, ChError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_match_the_spec() {
        let all = [
            (ChError::Auth, "auth"),
            (ChError::NotFound("x".into()), "not_found"),
            (
                ChError::RateLimited {
                    reset_in_secs: None,
                },
                "rate_limited",
            ),
            (ChError::invalid("bad"), "validation"),
            (ChError::Upstream("500".into()), "upstream"),
            (ChError::Internal("bug".into()), "internal"),
        ];
        for (e, kind) in all {
            assert_eq!(e.kind(), kind);
        }
    }

    #[test]
    fn auth_message_explains_sandbox_keys() {
        let msg = ChError::Auth.to_string();
        assert!(msg.contains("COMPANIES_HOUSE_API_KEY"), "{msg}");
        assert!(msg.contains("sandbox"), "{msg}");
    }

    #[test]
    fn not_found_messages_are_not_prefixed_twice() {
        let e = ChError::NotFound("no company with number 1".into());
        assert_eq!(
            format!("{}: {e}", e.kind()),
            "not_found: no company with number 1"
        );
    }

    #[test]
    fn rate_limit_message_says_when_to_retry() {
        let msg = ChError::RateLimited {
            reset_in_secs: Some(42),
        }
        .to_string();
        assert!(msg.contains("600 requests per 5 minutes"), "{msg}");
        assert!(msg.contains("resets in 42 seconds"), "{msg}");
        let msg = ChError::RateLimited {
            reset_in_secs: None,
        }
        .to_string();
        assert!(msg.contains("try again in a few minutes"), "{msg}");
    }
}
