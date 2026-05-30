use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::indexer::types::{RewardCampaignClaimedEvent, SuiEvent};

pub struct RewardCampaignClaimedHandler;

#[async_trait]
impl EventHandler for RewardCampaignClaimedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in event"))?;

        let event_data: RewardCampaignClaimedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse RewardCampaignClaimed event")?;
        let amount = event_data
            .amount
            .parse::<i64>()
            .context("Invalid reward campaign claim amount")?;

        info!(
            "Handling RewardCampaignClaimed: campaign={} winner={} amount={}",
            event_data.campaign_tweet_id, event_data.winner_xid, amount
        );

        sqlx::query(
            r#"
            UPDATE reward_campaign_winners AS winner
            SET claim_tx_digest = $3, claimed_at = NOW()
            FROM reward_campaigns AS campaign
            WHERE winner.campaign_id = campaign.id
              AND campaign.campaign_tweet_id = $1
              AND winner.winner_xid = $2
            "#,
        )
        .bind(&event_data.campaign_tweet_id)
        .bind(&event_data.winner_xid)
        .bind(&event.id.tx_digest)
        .execute(pool)
        .await
        .context("Failed to update reward campaign claim digest")?;

        sqlx::query(
            r#"
            UPDATE reward_campaigns
            SET paid_winner_count = paid_winner_count + 1, updated_at = NOW()
            WHERE campaign_tweet_id = $1
            "#,
        )
        .bind(&event_data.campaign_tweet_id)
        .execute(pool)
        .await
        .context("Failed to update reward campaign paid winner count")?;

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
        .context("Failed to update reward campaign winner balance")?;

        Ok(())
    }
}
