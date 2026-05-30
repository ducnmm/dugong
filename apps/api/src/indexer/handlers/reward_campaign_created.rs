use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use tracing::info;

use super::EventHandler;
use crate::indexer::types::{RewardCampaignCreatedEvent, SuiEvent};

pub struct RewardCampaignCreatedHandler;

#[async_trait]
impl EventHandler for RewardCampaignCreatedHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()> {
        let parsed_json = event
            .parsed_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Missing parsed_json in event"))?;

        let event_data: RewardCampaignCreatedEvent = serde_json::from_value(parsed_json)
            .context("Failed to parse RewardCampaignCreated event")?;
        let reward_amount = event_data
            .reward_amount
            .parse::<i64>()
            .context("Invalid reward campaign amount")?;
        let max_winners = event_data
            .max_winners
            .parse::<i64>()
            .context("Invalid reward campaign max winners")?;
        let total_budget = event_data
            .total_budget
            .parse::<i64>()
            .context("Invalid reward campaign total budget")?;
        let campaign_type = campaign_type_value(&event_data.campaign_type)?;

        info!(
            "Handling RewardCampaignCreated: campaign={} tweet={} budget={}",
            event_data.campaign_id, event_data.campaign_tweet_id, total_budget
        );

        sqlx::query(
            r#"
            INSERT INTO reward_campaigns (
                campaign_object_id,
                campaign_tweet_id,
                creator_xid,
                creator_handle,
                campaign_type,
                target,
                coin_type,
                reward_amount,
                max_winners,
                create_tx_digest
            )
            VALUES ($1, $2, $3, '', $4::reward_campaign_type, $5, $6, $7, $8, $9)
            ON CONFLICT (campaign_tweet_id)
            DO UPDATE SET
                campaign_object_id = EXCLUDED.campaign_object_id,
                creator_xid = EXCLUDED.creator_xid,
                campaign_type = EXCLUDED.campaign_type,
                target = EXCLUDED.target,
                coin_type = EXCLUDED.coin_type,
                reward_amount = EXCLUDED.reward_amount,
                max_winners = EXCLUDED.max_winners,
                create_tx_digest = EXCLUDED.create_tx_digest,
                updated_at = NOW()
            "#,
        )
        .bind(&event_data.campaign_id)
        .bind(&event_data.campaign_tweet_id)
        .bind(&event_data.creator_xid)
        .bind(campaign_type)
        .bind(&event_data.target)
        .bind(&event_data.coin_type)
        .bind(reward_amount)
        .bind(max_winners)
        .bind(&event.id.tx_digest)
        .execute(pool)
        .await
        .context("Failed to upsert reward campaign")?;

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
        .bind(-total_budget)
        .execute(pool)
        .await
        .context("Failed to update reward campaign creator balance")?;

        Ok(())
    }
}

fn campaign_type_value(value: &serde_json::Value) -> Result<&'static str> {
    match value {
        serde_json::Value::Number(number) if number.as_u64() == Some(1) => Ok("top_replies"),
        serde_json::Value::Number(number) if number.as_u64() == Some(2) => Ok("first_hashtag"),
        serde_json::Value::String(value) if value == "1" || value == "top_replies" => {
            Ok("top_replies")
        }
        serde_json::Value::String(value) if value == "2" || value == "first_hashtag" => {
            Ok("first_hashtag")
        }
        other => anyhow::bail!("Invalid reward campaign type: {}", other),
    }
}
