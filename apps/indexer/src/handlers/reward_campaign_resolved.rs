use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::{info, warn};

use super::EventHandler;
use crate::types::{RewardCampaignResolvedEvent, SuiEvent};
use dugong_core::db::models::{RewardCampaign, RewardCampaignWinner};

pub struct RewardCampaignResolvedHandler;

#[async_trait]
impl EventHandler for RewardCampaignResolvedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event.parsed_json.clone().ok_or_else(|| {
            anyhow::anyhow!("Missing parsed_json in RewardCampaignResolved event")
        })?;

        let event_data: RewardCampaignResolvedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse RewardCampaignResolved event")?;

        info!(
            campaign_tweet_id = %event_data.campaign_tweet_id,
            winner_count = %event_data.winner_count,
            "Handling RewardCampaignResolved event"
        );

        let winner_count: i64 = event_data
            .winner_count
            .parse()
            .context("Invalid winner_count in RewardCampaignResolved event")?;
        let unallocated_refund: i64 = event_data
            .unallocated_refund
            .parse()
            .context("Invalid unallocated_refund in RewardCampaignResolved event")?;

        // Mirror entitlements: the per-winner amount is the campaign's reward_amount.
        if let Some(campaign) =
            RewardCampaign::find_by_campaign_tweet_id(pool, &event_data.campaign_tweet_id)
                .await
                .context("Failed to fetch campaign for resolved event")?
        {
            for winner_xid in &event_data.winner_xids {
                if let Err(e) = RewardCampaignWinner::upsert(
                    pool,
                    &event_data.campaign_tweet_id,
                    winner_xid,
                    campaign.reward_amount,
                )
                .await
                {
                    warn!(error = %e, winner_xid = %winner_xid, "Failed to mirror campaign winner from event");
                }
            }
        }

        RewardCampaign::mark_resolved(
            pool,
            &event_data.campaign_tweet_id,
            winner_count,
            unallocated_refund,
        )
        .await
        .context("Failed to mark campaign resolved from indexer")?;

        Ok(())
    }
}
