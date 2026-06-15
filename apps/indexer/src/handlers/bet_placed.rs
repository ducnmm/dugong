use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::types::{BetPlacedEvent, SuiEvent};
use dugong_core::db::models::MarketBet;

pub struct BetPlacedHandler;

#[async_trait]
impl EventHandler for BetPlacedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in BetPlaced event"))?;

        let event_data: BetPlacedEvent =
            serde_json::from_value(parsed_json).context("Failed to parse BetPlaced event")?;

        let amount: i64 = event_data
            .amount
            .parse()
            .context("Failed to parse bet amount")?;

        info!(
            market_tweet_id = %event_data.market_tweet_id,
            bet_tweet_id = %event_data.bet_tweet_id,
            better_xid = %event_data.better_xid,
            side = event_data.side,
            amount,
            "Handling BetPlaced event"
        );

        MarketBet::upsert(
            pool,
            &event_data.market_tweet_id,
            &event_data.bet_tweet_id,
            &event_data.better_xid,
            event_data.side,
            &event_data.coin_type,
            amount,
            Some(event.id.tx_digest.as_str()),
        )
        .await
        .context("Failed to upsert market bet from indexer")?;

        // The stake is escrowed out of the better's account into the market pool,
        // but the on-chain debit emits no coin event — mirror it here so the
        // dashboard balance stays in sync.
        sqlx::query(
            r#"
            INSERT INTO account_balances (x_user_id, coin_type, balance)
            VALUES ($1, $2, 0)
            ON CONFLICT (x_user_id, coin_type)
            DO UPDATE SET
                balance = account_balances.balance - $3,
                updated_at = NOW()
            "#,
        )
        .bind(&event_data.better_xid)
        .bind(&event_data.coin_type)
        .bind(amount)
        .execute(pool)
        .await
        .context("Failed to debit better balance from bet")?;

        Ok(())
    }
}
