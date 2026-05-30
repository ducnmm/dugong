use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::indexer::types::{PredictionMarketCreatedEvent, SuiEvent};

pub struct PredictionMarketCreatedHandler;

#[async_trait]
impl EventHandler for PredictionMarketCreatedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in event"))?;

        let event_data: PredictionMarketCreatedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse PredictionMarketCreated event")?;

        info!(
            "Handling PredictionMarketCreated: market={} tweet={}",
            event_data.market_id, event_data.market_tweet_id
        );

        sqlx::query(
            r#"
            INSERT INTO prediction_markets (
                market_object_id,
                market_tweet_id,
                creator_xid,
                creator_handle,
                question,
                create_tx_digest
            )
            VALUES ($1, $2, $3, '', $4, $5)
            ON CONFLICT (market_tweet_id)
            DO UPDATE SET
                market_object_id = EXCLUDED.market_object_id,
                creator_xid = EXCLUDED.creator_xid,
                question = EXCLUDED.question,
                create_tx_digest = EXCLUDED.create_tx_digest,
                updated_at = NOW()
            "#,
        )
        .bind(&event_data.market_id)
        .bind(&event_data.market_tweet_id)
        .bind(&event_data.creator_xid)
        .bind(&event_data.question)
        .bind(&event.id.tx_digest)
        .execute(pool)
        .await
        .context("Failed to upsert prediction market")?;

        Ok(())
    }
}
