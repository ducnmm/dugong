mod api;
mod clients;
mod config;
mod constants;
mod db;
mod error;
mod indexer;
mod processor;
mod twitter_session;
mod webhook;

use crate::clients::redis_client::RedisClient;
use crate::config::Config;
use crate::db::{create_pool, run_migrations};
use crate::indexer::Indexer;
use crate::processor::ProcessorWorker;
use crate::webhook::handler::{handle_crc_challenge, handle_webhook, health_check, AppState};
use axum::{routing::get, Router};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dugong_api=info,tower_http=debug".into()),
        )
        .init();

    info!("Starting Dugong Backend...");

    // Load config
    let config = Config::from_env()?;
    // The processor worker posts a reply to every tweet it handles, so refuse
    // to start without reply credentials instead of silently dropping replies.
    config.ensure_reply_capable()?;
    info!("Config loaded");

    // Setup database
    let db = create_pool(&config.database_url).await?;
    info!("Database connected");

    // Run migrations
    run_migrations(&db).await?;
    info!("Migrations completed");

    // Setup Redis
    let redis = RedisClient::new(&config.redis_url).await?;
    info!("Redis connected");

    // Create shared state
    let state = Arc::new(AppState {
        config: config.clone(),
        db,
        redis,
    });

    // Start indexer worker (if enabled)
    if config.enable_indexer {
        info!("Indexer is ENABLED in API server");
        let indexer_config = config.clone();
        let indexer_db = state.db.clone();
        tokio::spawn(async move {
            match Indexer::new(indexer_config, indexer_db).await {
                Ok(indexer) => {
                    if let Err(e) = indexer.start().await {
                        tracing::error!("Indexer error: {}", e);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to initialize indexer: {}", e);
                }
            }
        });
    } else {
        info!("Indexer is DISABLED - run dugong-indexer binary separately");
    }

    // Start transaction processor worker
    let processor_state = state.clone();
    tokio::spawn(async move {
        ProcessorWorker::new(processor_state).run().await;
    });

    // Build router
    let app = Router::new()
        .route("/", get(health_check))
        .route("/webhook", get(handle_crc_challenge).post(handle_webhook))
        .route(
            "/api/account/by-wallet/:address",
            get(crate::api::get_account_by_wallet),
        )
        .route(
            "/api/account/:sui_object_id/balance",
            get(crate::api::get_account_balance),
        )
        .route(
            "/api/account/:sui_object_id/transactions",
            get(crate::api::get_transactions_by_account),
        )
        .route(
            "/api/transaction/:tx_digest",
            get(crate::api::get_transaction_by_digest),
        )
        .route("/api/accounts/search", get(crate::api::search_accounts))
        .route(
            "/api/accounts/:twitter_user_id",
            get(crate::api::get_account_by_twitter_id),
        )
        .route(
            "/api/accounts/:twitter_user_id/transactions",
            get(crate::api::get_account_transactions),
        )
        // Secure link wallet endpoints
        .route(
            "/api/link-wallet/generate-message",
            axum::routing::post(crate::api::generate_link_message),
        )
        .route(
            "/api/link-wallet/submit",
            axum::routing::post(crate::api::secure_link_wallet),
        )
        // X OAuth 2.0 Authentication
        .route(
            "/api/auth/twitter/token",
            axum::routing::post(crate::api::exchange_twitter_token),
        )
        // Transaction Sponsorship (Enoki)
        .route(
            "/api/sponsor",
            axum::routing::post(crate::api::sponsor_transaction),
        )
        .route(
            "/api/execute",
            axum::routing::post(crate::api::execute_sponsored_transaction),
        )
        .layer(
            CorsLayer::permissive(), // Allow all origins for development
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Start server
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Listening on http://{}", addr);
    info!("Webhook endpoint: http://{}/webhook", addr);
    info!("Health check: http://{}/", addr);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    axum::serve(listener, app).await?;

    Ok(())
}
