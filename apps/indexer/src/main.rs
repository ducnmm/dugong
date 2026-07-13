use dugong_core::config::Config;
use dugong_core::db::{create_pool, run_migrations};

use dugong_indexer::indexer::Indexer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load this crate's own apps/indexer/.env. Resolved relative to the crate
    // (CARGO_MANIFEST_DIR) so `cargo run -p dugong-indexer` works from any
    // directory, not just apps/indexer. dotenvy is non-overriding here, so real
    // environment variables win: in the container / on Railway this path does
    // not exist and injected vars are used, making the absent file harmless.
    dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env")).ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dugong_indexer=info,dugong_core=info".into()),
        )
        .init();

    tracing::info!("Starting Dugong Indexer Service");

    let config = Config::from_env()?;
    tracing::info!("Config loaded");

    let db = create_pool(&config.database_url).await?;
    tracing::info!("Database connected");

    run_migrations(&db).await?;
    tracing::info!("Migrations completed");

    let mut indexer = Indexer::new(config, db).await?;
    tracing::info!("Indexer initialized");

    indexer.start().await?;

    Ok(())
}
