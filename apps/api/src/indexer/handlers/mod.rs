pub mod account_created;
pub mod coin_deposited;
pub mod coin_transferred;
pub mod coin_withdrawn;
pub mod handle_updated;
pub mod prediction_bet_placed;
pub mod prediction_market_created;
pub mod prediction_market_resolved;
pub mod prediction_payout_claimed;
pub mod reward_campaign_claimed;
pub mod reward_campaign_created;
pub mod reward_campaign_resolved;
pub mod wallet_linked;

use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;

use crate::indexer::types::SuiEvent;

/// Trait for event handlers
#[async_trait]
pub trait EventHandler {
    async fn handle(pool: &PgPool, event: &SuiEvent) -> Result<()>;
}
