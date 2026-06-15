use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::types::{MarketWinnerPaidEvent, SuiEvent};

pub struct MarketWinnerPaidHandler;

#[async_trait]
impl EventHandler for MarketWinnerPaidHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in MarketWinnerPaid event"))?;

        let event_data: MarketWinnerPaidEvent =
            serde_json::from_value(parsed_json).context("Failed to parse MarketWinnerPaid event")?;

        let amount = event_data.amount.parse::<i64>().unwrap_or(0);

        info!(
            market_tweet_id = %event_data.market_tweet_id,
            winner_xid = %event_data.winner_xid,
            amount,
            coin_type = %event_data.coin_type,
            "Handling MarketWinnerPaid event"
        );

        // The payout is credited to the winner's account on-chain without a coin
        // event — mirror it into account_balances so the dashboard balance matches.
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
        .bind(&event_data.winner_xid)
        .bind(&event_data.coin_type)
        .bind(amount)
        .execute(pool)
        .await
        .context("Failed to credit winner balance from market payout")?;

        Ok(())
    }
}
