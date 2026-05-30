use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::indexer::types::{PredictionMarketResolvedEvent, SuiEvent};

pub struct PredictionMarketResolvedHandler;

#[async_trait]
impl EventHandler for PredictionMarketResolvedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in event"))?;

        let event_data: PredictionMarketResolvedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse PredictionMarketResolved event")?;

        info!(
            "Handling PredictionMarketResolved: market={} outcome={}",
            event_data.market_tweet_id, event_data.outcome
        );

        sqlx::query(
            r#"
            UPDATE prediction_markets
            SET
                status = 'resolved',
                outcome = $2::prediction_bet_choice,
                resolved_by_tweet_id = $3,
                resolve_tx_digest = $4,
                resolved_at = NOW(),
                updated_at = NOW()
            WHERE market_tweet_id = $1
            "#,
        )
        .bind(&event_data.market_tweet_id)
        .bind(&event_data.outcome)
        .bind(&event_data.solve_tweet_id)
        .bind(&event.id.tx_digest)
        .execute(pool)
        .await
        .context("Failed to mark prediction market resolved")?;

        Ok(())
    }
}
