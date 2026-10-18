pub mod api;
pub mod codes;
pub mod config;
pub mod error;
pub mod service;
pub mod sic;
pub mod tools;
pub mod verifier;
pub mod view;

pub use service::{DataService, from_env};

pub const VERSION: &str = match option_env!("APP_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
