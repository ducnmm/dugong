use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::indexer::types::{PredictionPayoutClaimedEvent, SuiEvent};

pub struct PredictionPayoutClaimedHandler;

#[async_trait]
impl EventHandler for PredictionPayoutClaimedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in event"))?;

        let event_data: PredictionPayoutClaimedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse PredictionPayoutClaimed event")?;
        let amount = event_data
            .amount
            .parse::<i64>()
            .context("Invalid prediction payout amount")?;

        info!(
            "Handling PredictionPayoutClaimed: market={} bettor={} amount={}",
            event_data.market_tweet_id, event_data.bettor_xid, amount
        );

        sqlx::query(
            r#"
            UPDATE prediction_market_bets AS bet
            SET payout_tx_digest = $3
            FROM prediction_markets AS market
            WHERE bet.market_id = market.id
              AND market.market_tweet_id = $1
              AND bet.bettor_xid = $2
            "#,
        )
        .bind(&event_data.market_tweet_id)
        .bind(&event_data.bettor_xid)
        .bind(&event.id.tx_digest)
        .execute(pool)
        .await
        .context("Failed to update prediction payout digest")?;

        sqlx::query(
            r#"
            INSERT INTO account_balances (x_user_id, coin_type, balance)
            VALUES ($1, $2, $3)
            ON CONFLICT (x_user_id, coin_type)
            DO UPDATE SET
                balance = account_balances.balance + EXCLUDED.balance,
                updated_at = NOW()
            "#,
        )
        .bind(&event_data.bettor_xid)
        .bind(&event_data.coin_type)
        .bind(amount)
        .execute(pool)
        .await
        .context("Failed to update payout receiver balance")?;

        Ok(())
    }
}
