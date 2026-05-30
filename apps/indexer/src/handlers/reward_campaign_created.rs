use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::types::{RewardCampaignCreatedEvent, SuiEvent};
use dugong_core::db::models::RewardCampaign;

pub struct RewardCampaignCreatedHandler;

#[async_trait]
impl EventHandler for RewardCampaignCreatedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in RewardCampaignCreated event"))?;

        let event_data: RewardCampaignCreatedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse RewardCampaignCreated event")?;

        info!(
            campaign_tweet_id = %event_data.campaign_tweet_id,
            campaign_id = %event_data.campaign_id,
            creator_xid = %event_data.creator_xid,
            campaign_type = event_data.campaign_type,
            "Handling RewardCampaignCreated event"
        );

        let reward_amount: i64 = event_data
            .reward_amount
            .parse()
            .context("Invalid reward_amount in RewardCampaignCreated event")?;
        let max_winners: i64 = event_data
            .max_winners
            .parse()
            .context("Invalid max_winners in RewardCampaignCreated event")?;

        RewardCampaign::upsert(
            pool,
            &event_data.campaign_tweet_id,
            &event_data.campaign_id,
            &event_data.creator_xid,
            event_data.campaign_type as i16,
            &event_data.target,
            &event_data.coin_type,
            reward_amount,
            max_winners,
            Some(event.id.tx_digest.as_str()),
        )
        .await
        .context("Failed to upsert reward campaign from indexer")?;

        Ok(())
    }
}
