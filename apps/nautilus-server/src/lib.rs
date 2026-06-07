// Copyright (c), Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use fastcrypto::ed25519::Ed25519KeyPair;
use serde_json::json;
use std::fmt;

mod apps {
    #[cfg(feature = "seal-example")]
    #[path = "seal-example/mod.rs"]
    pub mod seal_example;

    #[cfg(feature = "dugong")]
    #[path = "dugong/mod.rs"]
    pub mod dugong;
}

pub mod app {
    #[cfg(feature = "seal-example")]
    pub use crate::apps::seal_example::*;

    #[cfg(feature = "dugong")]
    pub use crate::apps::dugong::*;
}

pub mod common;

/// Production base URL for the TwitterAPI.io (Tweeter) endpoints.
pub const TWITTERAPI_IO_BASE_URL: &str = "https://api.twitterapi.io";
/// Production base URL for the official Twitter/X API (OAuth2 `users/me`).
pub const TWITTER_API_BASE_URL: &str = "https://api.twitter.com";

/// App state, at minimum needs to maintain the ephemeral keypair.
pub struct AppState {
    /// Ephemeral keypair on boot
    pub eph_kp: Ed25519KeyPair,
    /// API key used by the selected enclave app.
    pub api_key: String,
    /// Base URL for TwitterAPI.io requests (overridable in tests).
    pub twitterapi_io_base_url: String,
    /// Base URL for the official Twitter/X API (overridable in tests).
    pub twitter_api_base_url: String,
    /// Latest Dugong package id used to resolve the DUG coin type.
    pub dugong_package_id: String,
}

/// Build the Axum router exposing the dugong enclave HTTP handlers.
///
/// Extracted so integration tests can drive the handlers in-process via
/// `tower::ServiceExt::oneshot`; `main.rs` wires the same routes plus the
/// host-only attestation/health endpoints.
#[cfg(feature = "dugong")]
pub fn build_router(state: std::sync::Arc<AppState>) -> axum::Router {
    use crate::app::{process_init_account, process_secure_link_wallet, process_tweet};
    use axum::routing::post;

    axum::Router::new()
        .route("/process_tweet", post(process_tweet))
        .route("/process_init_account", post(process_init_account))
        .route(
            "/process_secure_link_wallet",
            post(process_secure_link_wallet),
        )
        .with_state(state)
}

/// Implement IntoResponse for EnclaveError.
impl IntoResponse for EnclaveError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            EnclaveError::GenericError(e) => (StatusCode::BAD_REQUEST, e),
        };
        let body = Json(json!({
            "error": error_message,
        }));
        (status, body).into_response()
    }
}

/// Enclave errors enum.
#[derive(Debug)]
pub enum EnclaveError {
    GenericError(String),
}

impl fmt::Display for EnclaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnclaveError::GenericError(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for EnclaveError {}
