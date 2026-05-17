use dugong_api::config::Config;
use dugong_api::db::{create_pool, run_migrations};
use dugong_api::indexer::Indexer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dugong_api=info".into()),
        )
        .init();

    tracing::info!("🚀 Starting Dugong Indexer Service");

    // Load config
    let config = Config::from_env()?;
    tracing::info!("✅ Config loaded");

    // Setup database
    let db = create_pool(&config.database_url).await?;
    tracing::info!("✅ Database connected");

    // Run migrations
    run_migrations(&db).await?;
    tracing::info!("✅ Migrations completed");

    // Create and start indexer
    let indexer = Indexer::new(config, db).await?;
    tracing::info!("✅ Indexer initialized");

    // Start indexing (blocks forever)
    indexer.start().await?;

    Ok(())
}
