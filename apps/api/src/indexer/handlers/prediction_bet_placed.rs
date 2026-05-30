use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::indexer::types::{PredictionBetPlacedEvent, SuiEvent};

pub struct PredictionBetPlacedHandler;

#[async_trait]
impl EventHandler for PredictionBetPlacedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in event"))?;

        let event_data: PredictionBetPlacedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse PredictionBetPlaced event")?;
        let amount = event_data
            .amount
            .parse::<i64>()
            .context("Invalid prediction bet amount")?;

        info!(
            "Handling PredictionBetPlaced: market={} bettor={} choice={} amount={}",
            event_data.market_tweet_id, event_data.bettor_xid, event_data.choice, amount
        );

        let market_id: (i32,) = sqlx::query_as(
            r#"
            INSERT INTO prediction_markets (
                market_object_id,
                market_tweet_id,
                creator_xid,
                creator_handle,
                question
            )
            VALUES ($1, $2, '', '', '')
            ON CONFLICT (market_tweet_id)
            DO UPDATE SET
                market_object_id = COALESCE(prediction_markets.market_object_id, EXCLUDED.market_object_id),
                updated_at = NOW()
            RETURNING id
            "#,
        )
        .bind(&event_data.market_id)
        .bind(&event_data.market_tweet_id)
        .fetch_one(pool)
        .await
        .context("Failed to upsert prediction market placeholder")?;

        sqlx::query(
            r#"
            INSERT INTO prediction_market_bets (
                market_id,
                bet_tweet_id,
                bettor_xid,
                bettor_handle,
                choice,
                coin_type,
                amount,
                bet_tx_digest
            )
            VALUES ($1, $2, $3, '', $4::prediction_bet_choice, $5, $6, $7)
            ON CONFLICT (bet_tweet_id)
            DO UPDATE SET
                market_id = EXCLUDED.market_id,
                bettor_xid = EXCLUDED.bettor_xid,
                bettor_handle = COALESCE(NULLIF(prediction_market_bets.bettor_handle, ''), EXCLUDED.bettor_handle),
                choice = EXCLUDED.choice,
                coin_type = EXCLUDED.coin_type,
                amount = EXCLUDED.amount,
                bet_tx_digest = EXCLUDED.bet_tx_digest
            "#,
        )
        .bind(market_id.0)
        .bind(&event_data.bet_tweet_id)
        .bind(&event_data.bettor_xid)
        .bind(&event_data.choice)
        .bind(&event_data.coin_type)
        .bind(amount)
        .bind(&event.id.tx_digest)
        .execute(pool)
        .await
        .context("Failed to upsert prediction bet")?;

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
        .bind(-amount)
        .execute(pool)
        .await
        .context("Failed to update bettor balance")?;

        Ok(())
    }
}
