use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::types::{MarketCreatedEvent, SuiEvent};
use dugong_core::db::models::Market;

pub struct MarketCreatedHandler;

#[async_trait]
impl EventHandler for MarketCreatedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in MarketCreated event"))?;

        let event_data: MarketCreatedEvent =
            serde_json::from_value(parsed_json).context("Failed to parse MarketCreated event")?;

        info!(
            market_tweet_id = %event_data.market_tweet_id,
            market_id = %event_data.market_id,
            creator_xid = %event_data.creator_xid,
            question = %event_data.question,
            "Handling MarketCreated event"
        );

        Market::upsert(
            pool,
            &event_data.market_tweet_id,
            &event_data.market_id,
            &event_data.creator_xid,
            &event_data.question,
            event_data.fee_bps as i16,
            Some(event.id.tx_digest.as_str()),
        )
        .await
        .context("Failed to upsert market from indexer")?;

        Ok(())
    }
}
