use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::indexer::types::{RewardCampaignResolvedEvent, SuiEvent};

pub struct RewardCampaignResolvedHandler;

#[async_trait]
impl EventHandler for RewardCampaignResolvedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in event"))?;

        let event_data: RewardCampaignResolvedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse RewardCampaignResolved event")?;
        let winner_count = event_data
            .winner_count
            .parse::<i32>()
            .context("Invalid reward campaign winner count")?;
        let unallocated_refund = event_data
            .unallocated_refund
            .parse::<i64>()
            .context("Invalid reward campaign unallocated refund")?;

        info!(
            "Handling RewardCampaignResolved: campaign={} winners={} refund={}",
            event_data.campaign_tweet_id, winner_count, unallocated_refund
        );

        let campaign_id: Option<(i32,)> = sqlx::query_as(
            r#"
            UPDATE reward_campaigns
            SET
                status = 'resolved',
                resolved_by_tweet_id = $2,
                resolve_tx_digest = $3,
                selected_winner_count = $4,
                resolved_at = NOW(),
                updated_at = NOW()
            WHERE campaign_tweet_id = $1
            RETURNING id
            "#,
        )
        .bind(&event_data.campaign_tweet_id)
        .bind(&event_data.solve_tweet_id)
        .bind(&event.id.tx_digest)
        .bind(winner_count)
        .fetch_optional(pool)
        .await
        .context("Failed to mark reward campaign resolved")?;

        if let Some((campaign_id,)) = campaign_id {
            for (idx, winner_xid) in event_data.winner_xids.iter().enumerate() {
                sqlx::query(
                    r#"
                    INSERT INTO reward_campaign_winners (
                        campaign_id,
                        winner_xid,
                        winner_handle,
                        rank,
                        reward_amount
                    )
                    SELECT id, $2, $2, $3, reward_amount
                    FROM reward_campaigns
                    WHERE id = $1
                    ON CONFLICT (campaign_id, winner_xid)
                    DO NOTHING
                    "#,
                )
                .bind(campaign_id)
                .bind(winner_xid)
                .bind(idx as i32 + 1)
                .execute(pool)
                .await
                .context("Failed to upsert reward campaign winner")?;
            }
        }

        if unallocated_refund > 0 {
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
            .bind(&event_data.creator_xid)
            .bind(&event_data.coin_type)
            .bind(unallocated_refund)
            .execute(pool)
            .await
            .context("Failed to update reward campaign refund balance")?;
        }

        Ok(())
    }
}
