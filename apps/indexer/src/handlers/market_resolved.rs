use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::types::{MarketResolvedEvent, SuiEvent};
use dugong_core::db::models::Market;

pub struct MarketResolvedHandler;

#[async_trait]
impl EventHandler for MarketResolvedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in MarketResolved event"))?;

        let event_data: MarketResolvedEvent =
            serde_json::from_value(parsed_json).context("Failed to parse MarketResolved event")?;

        info!(
            market_tweet_id = %event_data.market_tweet_id,
            resolver_xid = %event_data.resolver_xid,
            outcome = event_data.outcome,
            "Handling MarketResolved event"
        );

        Market::set_resolved(
            pool,
            &event_data.market_tweet_id,
            event_data.outcome,
            Some(&event.id.tx_digest),
        )
        .await
        .context("Failed to set market resolved from indexer")?;

        Ok(())
    }
}
