use anyhow::{Context, Result};
use sqlx::PgPool;
use tracing::{debug, warn};

use super::handlers::EventHandler;
use super::types::{parse_event_type, SuiEvent};

pub struct EventProcessor {
    pool: PgPool,
}

impl EventProcessor {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Process a batch of events
    pub async fn process_events(&self, events: &[SuiEvent]) -> Result<usize> {
        let mut processed = 0;

        for event in events {
            match self.process_single_event(event).await {
                Ok(_) => {
                    processed += 1;
                }
                Err(e) => {
                    warn!("Failed to process event {}: {}", event.id.tx_digest, e);
                    // Continue processing other events
                }
            }
        }

        Ok(processed)
    }

    /// Process a single event
    async fn process_single_event(&self, event: &SuiEvent) -> Result<()> {
        let event_type =
            parse_event_type(&event.event_type).context("Failed to parse event type")?;

        debug!("Processing event: {} ({})", event_type, event.id.tx_digest);

        // Route to appropriate handler based on event type
        match event_type {
            "AccountCreated" => {
                self.handle_account_created(event).await?;
            }
            "WalletLinked" => {
                self.handle_wallet_linked(event).await?;
            }
            "TransferCompleted" => {
                self.handle_transfer_completed(event).await?;
            }
            "CoinDeposited" => {
                self.handle_coin_deposited(event).await?;
            }
            "CoinWithdrawn" => {
                self.handle_coin_withdrawn(event).await?;
            }
            "HandleUpdated" => {
                self.handle_handle_updated(event).await?;
            }
            "PredictionMarketCreated" => {
                self.handle_prediction_market_created(event).await?;
            }
            "PredictionBetPlaced" => {
                self.handle_prediction_bet_placed(event).await?;
            }
            "PredictionMarketResolved" => {
                self.handle_prediction_market_resolved(event).await?;
            }
            "PredictionPayoutClaimed" => {
                self.handle_prediction_payout_claimed(event).await?;
            }
            "RewardCampaignCreated" => {
                self.handle_reward_campaign_created(event).await?;
            }
            "RewardCampaignResolved" => {
                self.handle_reward_campaign_resolved(event).await?;
            }
            "RewardCampaignClaimed" => {
                self.handle_reward_campaign_claimed(event).await?;
            }
            _ => {
                warn!("Unknown event type: {}", event_type);
            }
        }

        Ok(())
    }

    async fn handle_account_created(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::account_created::AccountCreatedHandler;
        AccountCreatedHandler::handle(&self.pool, event).await
    }

    async fn handle_wallet_linked(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::wallet_linked::WalletLinkedHandler;
        WalletLinkedHandler::handle(&self.pool, event).await
    }

    async fn handle_transfer_completed(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::coin_transferred::TransferCompletedHandler;
        TransferCompletedHandler::handle(&self.pool, event).await
    }

    async fn handle_coin_deposited(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::coin_deposited::CoinDepositedHandler;
        CoinDepositedHandler::handle(&self.pool, event).await
    }

    async fn handle_coin_withdrawn(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::coin_withdrawn::CoinWithdrawnHandler;
        CoinWithdrawnHandler::handle(&self.pool, event).await
    }

    async fn handle_handle_updated(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::handle_updated::HandleUpdatedHandler;
        HandleUpdatedHandler::handle(&self.pool, event).await
    }

    async fn handle_prediction_market_created(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::prediction_market_created::PredictionMarketCreatedHandler;
        PredictionMarketCreatedHandler::handle(&self.pool, event).await
    }

    async fn handle_prediction_bet_placed(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::prediction_bet_placed::PredictionBetPlacedHandler;
        PredictionBetPlacedHandler::handle(&self.pool, event).await
    }

    async fn handle_prediction_market_resolved(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::prediction_market_resolved::PredictionMarketResolvedHandler;
        PredictionMarketResolvedHandler::handle(&self.pool, event).await
    }

    async fn handle_prediction_payout_claimed(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::prediction_payout_claimed::PredictionPayoutClaimedHandler;
        PredictionPayoutClaimedHandler::handle(&self.pool, event).await
    }

    async fn handle_reward_campaign_created(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::reward_campaign_created::RewardCampaignCreatedHandler;
        RewardCampaignCreatedHandler::handle(&self.pool, event).await
    }

    async fn handle_reward_campaign_resolved(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::reward_campaign_resolved::RewardCampaignResolvedHandler;
        RewardCampaignResolvedHandler::handle(&self.pool, event).await
    }

    async fn handle_reward_campaign_claimed(&self, event: &SuiEvent) -> Result<()> {
        use super::handlers::reward_campaign_claimed::RewardCampaignClaimedHandler;
        RewardCampaignClaimedHandler::handle(&self.pool, event).await
    }
}
