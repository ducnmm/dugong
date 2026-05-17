use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::indexer::types::{SuiEvent, TransferCompletedEvent};

pub struct TransferCompletedHandler;

#[async_trait]
impl EventHandler for TransferCompletedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        // Parse event data
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in event"))?;

        let event_data: TransferCompletedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse TransferCompleted event")?;

        let amount = event_data.amount.parse::<i64>().unwrap_or(0);

        info!(
            "Handling TransferCompleted: {} -> {}, amount={} {}, tweet_id={}",
            event_data.from_xid,
            event_data.to_xid,
            amount,
            event_data.coin_type,
            event_data.tweet_id
        );

        // Store transfer in database
        sqlx::query(
            r#"
            INSERT INTO transfers (
                transaction_digest,
                transfer_type,
                from_xid,
                to_xid,
                coin_type,
                amount,
                tweet_id,
                timestamp
            )
            VALUES ($1, 'transfer', $2, $3, $4, $5, $6, $7)
            ON CONFLICT (transaction_digest) DO NOTHING
            "#,
        )
        .bind(&event.id.tx_digest)
        .bind(&event_data.from_xid)
        .bind(&event_data.to_xid)
        .bind(&event_data.coin_type)
        .bind(amount)
        .bind(if event_data.tweet_id.is_empty() {
            None
        } else {
            Some(&event_data.tweet_id)
        })
        .bind(event_data.timestamp.parse::<i64>().unwrap_or(0))
        .execute(pool)
        .await
        .context("Failed to insert transfer")?;

        // Update sender balance (subtract)
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
        .bind(&event_data.from_xid)
        .bind(&event_data.coin_type)
        .bind(amount)
        .execute(pool)
        .await
        .context("Failed to update sender balance")?;

        // Update receiver balance (add)
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
        .bind(&event_data.to_xid)
        .bind(&event_data.coin_type)
        .bind(amount)
        .execute(pool)
        .await
        .context("Failed to update receiver balance")?;

        info!(
            "Updated balances: {} -= {}, {} += {}",
            event_data.from_xid, amount, event_data.to_xid, amount
        );

        Ok(())
    }
}
