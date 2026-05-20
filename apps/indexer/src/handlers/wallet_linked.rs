use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use dugong_core::db::models::DugongAccount;
use crate::types::{SuiEvent, WalletLinkedEvent};

pub struct WalletLinkedHandler;

#[async_trait]
impl EventHandler for WalletLinkedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        // Parse event data
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in event"))?;

        let event_data: WalletLinkedEvent =
            serde_json::from_value(parsed_json).context("Failed to parse WalletLinked event")?;

        info!(
            "Handling WalletLinked: xid={}, owner={}",
            event_data.xid, event_data.owner_address
        );

        // Update owner_address in database
        DugongAccount::link_owner(pool, &event_data.xid, &event_data.owner_address)
            .await
            .context("Failed to link owner")?;

        Ok(())
    }
}
