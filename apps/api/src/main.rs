// Entry point for the Dugong API + transaction processor worker.
use dugong_api::build_router;
use dugong_api::processor::ProcessorWorker;
use dugong_api::webhook::handler::AppState;
use dugong_core::clients::redis_client::RedisClient;
use dugong_core::config::Config;
use dugong_core::db::{create_pool, run_migrations};
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env")).ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dugong_api=info,dugong_core=info,tower_http=debug".into()),
        )
        .init();

    info!("Starting Dugong API...");

    let config = Config::from_env()?;
    // The processor worker posts a reply to every tweet it handles, so refuse
    // to start without reply credentials instead of silently dropping replies.
    config.ensure_reply_capable()?;
    // Refuse to start without OAuth credential-security config: the auth/link-wallet
    // endpoints encrypt refresh tokens and sign session tokens, so missing keys must
    // fail loudly here rather than at the first user request.
    config.ensure_token_security()?;
    info!("Config loaded");

    let db = create_pool(&config.database_url).await?;
    info!("Database connected");

    run_migrations(&db).await?;
    info!("Migrations completed");

    let redis = RedisClient::new(&config.redis_url).await?;
    info!("Redis connected");

    let state = Arc::new(AppState {
        config: config.clone(),
        db,
        redis,
        sponsor_fallback_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    });

    let processor_state = state.clone();
    tokio::spawn(async move {
        ProcessorWorker::new(processor_state).run().await;
    });

    let app = build_router(state);

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
