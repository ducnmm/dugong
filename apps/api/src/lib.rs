pub mod error;
pub mod processor;
pub mod routes;
pub mod webhook;

use axum::{routing::get, Router};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::webhook::handler::{handle_crc_challenge, handle_webhook, health_check, AppState};

/// Build the API router with all routes and middleware wired to `state`.
///
/// Extracted from `main` so integration tests can exercise the full router
/// via `tower::ServiceExt::oneshot` without binding a TCP listener.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(health_check))
        .route("/webhook", get(handle_crc_challenge).post(handle_webhook))
        .route(
            "/api/account/by-wallet/:address",
            get(routes::get_account_by_wallet),
        )
        .route(
            "/api/account/:sui_object_id/balance",
            get(routes::get_account_balance),
        )
        .route(
            "/api/account/:sui_object_id/transactions",
            get(routes::get_transactions_by_account),
        )
        .route(
            "/api/transaction/:tx_digest",
            get(routes::get_transaction_by_digest),
        )
        .route("/api/accounts/search", get(routes::search_accounts))
        .route(
            "/api/accounts/:twitter_user_id",
            get(routes::get_account_by_twitter_id),
        )
        .route(
            "/api/accounts/:twitter_user_id/transactions",
            get(routes::get_account_transactions),
        )
        .route(
            "/api/link-wallet/generate-message",
            axum::routing::post(routes::generate_link_message),
        )
        .route(
            "/api/link-wallet/submit",
            axum::routing::post(routes::secure_link_wallet),
        )
        .route(
            "/api/auth/twitter/token",
            axum::routing::post(routes::exchange_twitter_token),
        )
        .route(
            "/api/auth/twitter/ensure-account",
            axum::routing::post(routes::ensure_dugong_account),
        )
        .route(
            "/api/sponsor",
            axum::routing::post(routes::sponsor_transaction),
        )
        .route(
            "/api/execute",
            axum::routing::post(routes::execute_sponsored_transaction),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
