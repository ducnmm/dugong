use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::types::{CoinWithdrawnEvent, SuiEvent};

pub struct CoinWithdrawnHandler;

#[async_trait]
impl EventHandler for CoinWithdrawnHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        // Parse event data
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in event"))?;

        let event_data: CoinWithdrawnEvent =
            serde_json::from_value(parsed_json).context("Failed to parse CoinWithdrawn event")?;

        let amount = event_data.amount.parse::<i64>().unwrap_or(0);

        info!(
            "Handling CoinWithdrawn: xid={}, amount={} {}",
            event_data.xid, amount, event_data.coin_type
        );

        // Update balance in account_balances table (subtract)
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
        .bind(&event_data.xid)
        .bind(&event_data.coin_type)
        .bind(amount)
        .execute(pool)
        .await
        .context("Failed to update balance")?;

        // Track in transfers table
        sqlx::query(
            r#"
            INSERT INTO transfers (
                transaction_digest,
                transfer_type,
                from_xid,
                to_xid,
                coin_type,
                amount,
                timestamp
            )
            VALUES ($1, 'withdraw', $2, NULL, $3, $4, $5)
            ON CONFLICT (transaction_digest) DO NOTHING
            "#,
        )
        .bind(&event.id.tx_digest)
        .bind(&event_data.xid)
        .bind(&event_data.coin_type)
        .bind(amount)
        .bind(
            event
                .timestamp_ms
                .as_ref()
                .and_then(|ts| ts.parse::<i64>().ok())
                .unwrap_or(0),
        )
        .execute(pool)
        .await
        .context("Failed to insert withdraw transfer")?;

        Ok(())
    }
}
