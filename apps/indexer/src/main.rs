mod cursor;
mod event_fetcher;
mod event_processor;
mod handlers;
mod indexer;
mod types;

use dugong_core::config::Config;
use dugong_core::db::{create_pool, run_migrations};

use crate::indexer::Indexer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dugong_indexer=info,dugong_core=info".into()),
        )
        .init();

    tracing::info!("🚀 Starting Dugong Indexer Service");

    let config = Config::from_env()?;
    tracing::info!("✅ Config loaded");

    let db = create_pool(&config.database_url).await?;
    tracing::info!("✅ Database connected");

    run_migrations(&db).await?;
    tracing::info!("✅ Migrations completed");

    let indexer = Indexer::new(config, db).await?;
    tracing::info!("✅ Indexer initialized");

    indexer.start().await?;

    Ok(())
}
