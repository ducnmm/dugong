// Re-export types from sui_client for convenience
pub use dugong_core::clients::sui_client::{EventPage, SuiEvent};

use serde::{Deserialize, Serialize};

/// Dugong-specific event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountCreatedEvent {
    pub xid: String,
    pub handle: String,
    pub account_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletLinkedEvent {
    pub xid: String,
    pub owner_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferCompletedEvent {
    pub from_xid: String,
    pub to_xid: String,
    pub tweet_id: String,
    pub coin_type: String,
    pub amount: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinDepositedEvent {
    pub xid: String,
    pub coin_type: String,
    pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinWithdrawnEvent {
    pub xid: String,
    pub coin_type: String,
    pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandleUpdatedEvent {
    pub xid: String,
    pub old_handle: String,
    pub new_handle: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketCreatedEvent {
    pub market_tweet_id: String,
    pub market_id: String,
    pub creator_xid: String,
    pub question: String,
    pub fee_bps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BetPlacedEvent {
    pub market_tweet_id: String,
    pub bet_tweet_id: String,
    pub better_xid: String,
    pub side: bool,
    pub coin_type: String,
    pub amount: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketResolvedEvent {
    pub market_tweet_id: String,
    pub resolver_xid: String,
    pub outcome: bool,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardCampaignCreatedEvent {
    pub campaign_id: String,
    pub campaign_tweet_id: String,
    pub creator_xid: String,
    pub campaign_type: u8,
    pub target: String,
    pub coin_type: String,
    pub reward_amount: String,
    pub max_winners: String,
    pub total_budget: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardCampaignResolvedEvent {
    pub campaign_id: String,
    pub campaign_tweet_id: String,
    pub solve_tweet_id: String,
    pub creator_xid: String,
    pub winner_xids: Vec<String>,
    pub winner_count: String,
    pub unallocated_refund: String,
    pub coin_type: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardCampaignClaimedEvent {
    pub campaign_id: String,
    pub campaign_tweet_id: String,
    pub winner_xid: String,
    pub coin_type: String,
    pub amount: String,
    pub timestamp: String,
}

/// Parse event type from full event type string
pub fn parse_event_type(full_type: &str) -> Option<&str> {
    // Example: 0x...::events::AccountCreated -> AccountCreated
    full_type.split("::").last()
}
