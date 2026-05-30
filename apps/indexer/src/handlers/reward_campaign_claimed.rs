use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::types::{RewardCampaignClaimedEvent, SuiEvent};
use dugong_core::db::models::RewardCampaignWinner;

pub struct RewardCampaignClaimedHandler;

#[async_trait]
impl EventHandler for RewardCampaignClaimedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in RewardCampaignClaimed event"))?;

        let event_data: RewardCampaignClaimedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse RewardCampaignClaimed event")?;

        info!(
            campaign_tweet_id = %event_data.campaign_tweet_id,
            winner_xid = %event_data.winner_xid,
            amount = %event_data.amount,
            "Handling RewardCampaignClaimed event"
        );

        RewardCampaignWinner::mark_claimed_indexed(
            pool,
            &event_data.campaign_tweet_id,
            &event_data.winner_xid,
            Some(event.id.tx_digest.as_str()),
        )
        .await
        .context("Failed to mark reward claimed from indexer")?;

        Ok(())
    }
}
