// Copyright (c), Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::common::IntentMessage;
use crate::common::{to_signed_response, IntentScope, ProcessDataRequest, ProcessedDataResponse};
use crate::AppState;
use crate::EnclaveError;
use axum::extract::State;
use axum::Json;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

// Hex encoding/decoding for addresses
mod hex {
    pub fn decode(s: &str) -> Result<Vec<u8>, String> {
        let bytes = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Hex decode error: {}", e))?;
        Ok(bytes)
    }

    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

/// Get decimals for a coin type
fn get_coin_decimals(coin_type: &str) -> u32 {
    match coin_type.to_uppercase().as_str() {
        "SUI" => 9,
        "DUG" | "CORE" => 9,
        "WAL" => 9,
        "USDC" => 6,
        _ => 9, // Default to 9 decimals if unknown
    }
}

/// Expand shorthand coin types to full type paths
/// Testnet addresses - update these for mainnet deployment
fn expand_coin_type(coin_type: &str, dugong_package_id: &str) -> String {
    match coin_type.to_uppercase().as_str() {
        "SUI" => "0x2::sui::SUI".to_string(),
        "USDC" => "0xa1ec7fc00a6f40db9693ad1415d0c193ad3906494428cf252621037bd7117e29::usdc::USDC"
            .to_string(),
        "WAL" => "0x8270feb7375eee355e64fdb69c50abb6b5f9393a722883c1cf45f8e26048810a::wal::WAL"
            .to_string(),
        "DUG" | "CORE" => format!("{}::dug::DUG", dugong_package_id),
        _ => coin_type.to_string(),
    }
}

/// Convert coin type to canonical format expected by Move's `type_name::get<T>()`
/// Example: "0x2::sui::SUI" -> "0000000000000000000000000000000000000000000000000000000000000002::sui::SUI"
fn to_canonical_coin_type(coin_type: &str, dugong_package_id: &str) -> String {
    let expanded = expand_coin_type(coin_type, dugong_package_id);

    if let Some(rest) = expanded.strip_prefix("0x") {
        if let Some(idx) = rest.find("::") {
            let addr = &rest[..idx];
            let module_and_type = &rest[idx..];
            let canonical_addr = format!("{:0>64}", addr);
            return format!("{}{}", canonical_addr, module_and_type);
        }
    }

    expanded
}

// ====
// Dugong Enclave Server Logic
// Processes Twitter-based transfer commands
// ====

/// Transfer payload that will be signed and sent to Sui blockchain
/// This must match TransferCoinPayload in dugong.move
/// IMPORTANT: All string fields must be Vec<u8> to match Move's vector<u8>
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransferPayload {
    pub from_xid: Vec<u8>,  // Twitter user ID as bytes
    pub to_xid: Vec<u8>,    // Twitter user ID as bytes
    pub amount: u64,        // Amount in smallest unit (MIST for SUI)
    pub coin_type: Vec<u8>, // Coin type as bytes (canonical, matches Move type_name)
    pub tweet_id: Vec<u8>,  // Tweet ID for idempotency
}

/// Init account payload that will be signed and sent to Sui blockchain
/// This must match InitAccountPayload in dugong.move
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InitAccountPayload {
    pub xid: Vec<u8>,    // Twitter user ID as bytes
    pub handle: Vec<u8>, // Twitter handle as bytes (e.g., b"alice")
}

/// Request containing XID to initialize account
#[derive(Debug, Serialize, Deserialize)]
pub struct InitAccountRequest {
    pub xid: String, // Twitter user ID
    pub handle: Option<String>,
}

/// Link wallet payload that will be signed and sent to Sui blockchain
/// This must match LinkWalletPayload in dugong.move
/// IMPORTANT: owner_address must be [u8; 32] to match Move's `address` type
/// Move `address` serializes as 32 bytes directly, NOT as Vec<u8> (which has length prefix)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LinkWalletPayload {
    pub xid: Vec<u8>,            // Twitter user ID as bytes
    pub owner_address: [u8; 32], // Sui wallet address (32 bytes, matches Move `address`)
}

/// Secure link wallet request with access token and wallet signature verification
/// This ensures that:
/// 1. The access_token belongs to the Twitter user (XID)
/// 2. The wallet_signature proves ownership of the wallet address
#[derive(Debug, Serialize, Deserialize)]
pub struct SecureLinkWalletRequest {
    pub access_token: String,     // Twitter OAuth2 access token
    pub wallet_address: String,   // Sui wallet address (0x...)
    pub wallet_signature: String, // Signature of the message by wallet (base64)
    pub message: String,          // The message that was signed
    pub timestamp: u64,           // Timestamp when message was created
}

/// Update handle payload that will be signed and sent to Sui blockchain
/// This must match UpdateHandlePayload in dugong.move
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateHandlePayload {
    pub xid: Vec<u8>,        // Twitter user ID as bytes
    pub new_handle: Vec<u8>, // New Twitter handle as bytes
}

/// Create market payload — must match CreateMarketPayload in core.move
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateMarketPayload {
    pub creator_xid: Vec<u8>,
    pub market_tweet_id: Vec<u8>,
    pub question: Vec<u8>,
    pub fee_bps: u16,
}

/// Place bet payload — must match PlaceBetPayload in core.move
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlaceBetPayload {
    pub better_xid: Vec<u8>,
    pub market_tweet_id: Vec<u8>,
    pub bet_tweet_id: Vec<u8>,
    pub amount: u64,
    pub coin_type: Vec<u8>,
    pub side: bool,
}

/// Resolve market payload — must match ResolveMarketPayload in core.move
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResolveMarketPayload {
    pub resolver_xid: Vec<u8>,
    pub market_tweet_id: Vec<u8>,
    pub outcome: bool,
}

/// Create reward campaign payload — must match CreateRewardCampaignPayload in core.move
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateRewardCampaignPayload {
    pub creator_xid: Vec<u8>,
    pub campaign_tweet_id: Vec<u8>,
    pub campaign_type: u8,
    pub target: Vec<u8>,
    pub reward_amount: u64,
    pub max_winners: u64,
    pub coin_type: Vec<u8>,
}

/// Resolve reward campaign payload — must match ResolveRewardCampaignPayload in core.move
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResolveRewardCampaignPayload {
    pub creator_xid: Vec<u8>,
    pub campaign_tweet_id: Vec<u8>,
    pub solve_tweet_id: Vec<u8>,
}

/// Generic claim payload — must match ClaimPayload in core.move
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClaimPayload {
    pub claimant_xid: Vec<u8>,
    pub target_tweet_id: Vec<u8>,
    pub claim_tweet_id: Vec<u8>,
}

// ============================================================================
// UNIFIED /process_tweet ENDPOINT - NEW SIMPLIFIED ARCHITECTURE
// ============================================================================

/// Command types that can be parsed from tweets
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CommandType {
    CreateAccount,
    Transfer,
    UpdateHandle,
    CreateMarket,
    PlaceBet,
    ResolveMarket,
    CreateRewardCampaign,
    ResolveRewardCampaign,
    Claim,
}

/// Common tweet metadata included in all responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TweetCommon {
    pub tweet_id: String,
    pub author_xid: String,
    pub author_handle: String,
}

/// Data for create_account command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccountData {
    pub xid: String,
    pub handle: String,
}

/// Data for transfer command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferData {
    pub from_xid: String,
    pub from_handle: String,
    pub to_xid: String,
    pub to_handle: String,
    pub amount: u64,
    pub coin_type: String,
}

/// Data for create_market command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMarketData {
    pub creator_xid: String,
    pub creator_handle: String,
    pub market_tweet_id: String,
    pub question: String,
    pub fee_bps: u16,
}

/// Data for place_bet command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceBetData {
    pub better_xid: String,
    pub better_handle: String,
    pub market_tweet_id: String,
    pub bet_tweet_id: String,
    pub amount: u64,
    pub coin_type: String,
    pub side: bool, // true = yes, false = no
}

/// Data for resolve_market command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveMarketData {
    pub resolver_xid: String,
    pub resolver_handle: String,
    pub market_tweet_id: String,
    pub outcome: bool, // true = yes, false = no
}

/// Data for create_reward_campaign command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRewardCampaignData {
    pub creator_xid: String,
    pub creator_handle: String,
    pub campaign_tweet_id: String,
    pub campaign_type: u8, // 1 = top replies, 2 = first hashtag
    pub target: String,
    pub reward_amount: u64,
    pub max_winners: u64,
    pub coin_type: String,
}

/// Data for resolve_reward_campaign command.
/// `campaign_tweet_id` is the parent (campaign) tweet; winners are selected off-chain by the worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveRewardCampaignData {
    pub resolver_xid: String,
    pub resolver_handle: String,
    pub campaign_tweet_id: String,
    pub solve_tweet_id: String,
}

/// Data for claim command. `target_tweet_id` is the parent (market or campaign) tweet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimData {
    pub claimant_xid: String,
    pub claimant_handle: String,
    pub target_tweet_id: String,
    pub claim_tweet_id: String,
}

/// Unified response for /process_tweet endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessTweetResponse {
    pub command_type: CommandType,
    pub intent: u8,
    pub timestamp_ms: u64,
    pub signature: String,
    pub common: TweetCommon,
    pub data: ProcessTweetData,
}

/// Union type for command-specific data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProcessTweetData {
    CreateAccount(CreateAccountData),
    Transfer(TransferData),
    CreateMarket(CreateMarketData),
    PlaceBet(PlaceBetData),
    ResolveMarket(ResolveMarketData),
    CreateRewardCampaign(CreateRewardCampaignData),
    ResolveRewardCampaign(ResolveRewardCampaignData),
    Claim(ClaimData),
}

/// Error response for /process_tweet endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessTweetError {
    pub error: bool,
    pub error_code: String,
    pub message: String,
    pub suggestion: String,
}

/// Request for /process_tweet endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessTweetRequest {
    pub tweet_url: String,
}

/// Unified /process_tweet endpoint
/// Parses tweet command and returns appropriate signed payload
///
/// Supported commands:
/// - Create Account: "@dugong create account" or "@dugong init"
/// - Transfer: "@dugong send <amount> <coin> to @<receiver>"
pub async fn process_tweet(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ProcessDataRequest<ProcessTweetRequest>>,
) -> Result<Json<ProcessTweetResponse>, EnclaveError> {
    let tweet_url = request.payload.tweet_url.clone();
    info!("Processing tweet via unified endpoint: {}", tweet_url);

    let current_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| EnclaveError::GenericError(format!("Failed to get current timestamp: {}", e)))?
        .as_millis() as u64;

    // Fetch tweet data from Tweeter API. This keeps tweet verification inside
    // the enclave instead of relying on the worker-provided webhook payload.
    let tweet_data =
        fetch_tweet_data(&state.twitterapi_io_base_url, &state.api_key, &tweet_url).await?;

    info!(
        "Tweet fetched - ID: {}, Author: {} (@{}), Text: {}",
        tweet_data.tweet_id, tweet_data.author_xid, tweet_data.author_handle, tweet_data.text
    );

    // Parse command type from tweet text
    let parsed_command = parse_tweet_command_type(&tweet_data.text, &tweet_data.author_xid)?;

    info!("Parsed command type: {:?}", parsed_command);

    // Process based on command type and build response
    match parsed_command {
        ParsedCommand::CreateAccount => {
            process_create_account_command(&state, &tweet_data, current_timestamp).await
        }
        ParsedCommand::Transfer { receiver_username } => {
            process_transfer_command(&state, &tweet_data, &receiver_username, current_timestamp)
                .await
        }
        ParsedCommand::CreateMarket { question } => {
            process_create_market_command(&state, &tweet_data, &question, current_timestamp).await
        }
        ParsedCommand::PlaceBet {
            amount,
            coin_type,
            side,
        } => {
            process_place_bet_command(
                &state,
                &tweet_data,
                amount,
                &coin_type,
                side,
                current_timestamp,
            )
            .await
        }
        ParsedCommand::ResolveMarket { outcome } => {
            process_resolve_market_command(&state, &tweet_data, outcome, current_timestamp).await
        }
        ParsedCommand::CreateRewardCampaign {
            campaign_type,
            target,
            reward_amount,
            max_winners,
            coin_type,
        } => {
            process_create_reward_campaign_command(
                &state,
                &tweet_data,
                campaign_type,
                &target,
                reward_amount,
                max_winners,
                &coin_type,
                current_timestamp,
            )
            .await
        }
        ParsedCommand::ResolveRewardCampaign => {
            process_resolve_reward_campaign_command(&state, &tweet_data, current_timestamp).await
        }
        ParsedCommand::Claim => process_claim_command(&state, &tweet_data, current_timestamp).await,
    }
}

/// Internal struct for tweet data fetched from Tweeter API
struct TweetData {
    tweet_id: String,
    author_xid: String,
    author_handle: String,
    text: String,
    mentions: Vec<TweetMention>,
    /// Parent tweet ID: in_reply_to_tweet_id, falling back to conversation_id
    parent_tweet_id: Option<String>,
}

struct TweetMention {
    id: Option<String>,
    username: String,
}

/// Internal enum for parsed commands
#[derive(Debug)]
enum ParsedCommand {
    CreateAccount,
    Transfer {
        receiver_username: String,
    },
    CreateMarket {
        question: String,
    },
    PlaceBet {
        amount: f64,
        coin_type: String,
        side: bool,
    },
    ResolveMarket {
        outcome: bool,
    },
    CreateRewardCampaign {
        campaign_type: u8,
        target: String,
        reward_amount: f64,
        max_winners: u64,
        coin_type: String,
    },
    ResolveRewardCampaign,
    Claim,
}

#[derive(Debug, Deserialize)]
struct TweeterTweetsResponse {
    #[serde(default)]
    tweets: Vec<TweeterTweet>,
    status: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TweeterTweet {
    id: String,
    text: String,
    author: TweeterUser,
    #[serde(default)]
    entities: Option<TweeterTweetEntities>,
    #[serde(rename = "inReplyToTweetId", default)]
    in_reply_to_tweet_id: Option<String>,
    #[serde(rename = "conversationId", default)]
    conversation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TweeterTweetEntities {
    #[serde(default)]
    user_mentions: Vec<TweeterUserMention>,
}

#[derive(Debug, Deserialize)]
struct TweeterUserMention {
    id_str: Option<String>,
    screen_name: String,
}

#[derive(Debug, Deserialize)]
struct TweeterUser {
    id: String,
    #[serde(rename = "userName")]
    user_name: String,
}

#[derive(Debug, Deserialize)]
struct TweeterUserInfoResponse {
    #[serde(default)]
    data: Option<TweeterUser>,
    status: String,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl TweeterTweetsResponse {
    fn error_message(&self) -> String {
        self.message
            .as_ref()
            .or(self.msg.as_ref())
            .cloned()
            .unwrap_or_else(|| "unknown tweeter API error".to_string())
    }
}

impl TweeterUserInfoResponse {
    fn error_message(&self) -> String {
        self.message
            .as_ref()
            .or(self.msg.as_ref())
            .cloned()
            .unwrap_or_else(|| "unknown tweeter API error".to_string())
    }
}

/// Fetch tweet data from Tweeter API.
async fn fetch_tweet_data(
    base_url: &str,
    api_key: &str,
    tweet_url: &str,
) -> Result<TweetData, EnclaveError> {
    let client = reqwest::Client::new();

    // Extract tweet ID from URL
    let tweet_id_regex = Regex::new(r"(?:x|twitter)\.com/[^/]+/status/(\d+)")
        .map_err(|_| EnclaveError::GenericError("Invalid tweet URL regex".to_string()))?;

    let tweet_id = tweet_id_regex
        .captures(tweet_url)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str())
        .ok_or_else(|| EnclaveError::GenericError("Invalid tweet URL format".to_string()))?;

    info!("Fetching tweet ID: {}", tweet_id);

    let response = client
        .get(format!("{}/twitter/tweets", base_url))
        .header("X-API-Key", api_key)
        .query(&[("tweet_ids", tweet_id)])
        .send()
        .await
        .map_err(|e| {
            EnclaveError::GenericError(format!("Failed to fetch tweet from Tweeter API: {}", e))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(EnclaveError::GenericError(format!(
            "Tweeter API returned error {}: {}",
            status, body
        )));
    }

    let response = response
        .json::<TweeterTweetsResponse>()
        .await
        .map_err(|e| {
            EnclaveError::GenericError(format!("Failed to parse Tweeter API response: {}", e))
        })?;

    if !response.status.eq_ignore_ascii_case("success") {
        return Err(EnclaveError::GenericError(format!(
            "Tweeter API error: {}",
            response.error_message()
        )));
    }

    let tweet = response
        .tweets
        .into_iter()
        .find(|tweet| tweet.id == tweet_id)
        .ok_or_else(|| EnclaveError::GenericError(format!("Tweet {} not found", tweet_id)))?;

    let mentions = tweet
        .entities
        .map(|entities| {
            entities
                .user_mentions
                .into_iter()
                .map(|mention| TweetMention {
                    id: mention.id_str,
                    username: mention.screen_name,
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(TweetData {
        tweet_id: tweet_id.to_string(),
        author_xid: tweet.author.id,
        author_handle: tweet.author.user_name,
        text: tweet.text,
        mentions,
        parent_tweet_id: tweet.in_reply_to_tweet_id.or(tweet.conversation_id),
    })
}

/// Parse tweet text to determine command type
fn parse_tweet_command_type(
    tweet_text: &str,
    _author_xid: &str,
) -> Result<ParsedCommand, EnclaveError> {
    // Transfer: @dugong send <amount> <coin> to @<receiver>
    let transfer_regex = Regex::new(r"(?i)@\w+\s+send\s+(\d+(?:\.\d+)?)\s+(\w+)\s+to\s+@(\w+)")
        .map_err(|_| EnclaveError::GenericError("Invalid transfer regex".to_string()))?;

    // Create market: @dugong create market: <question>
    let create_market_regex = Regex::new(r"(?i)@\w+\s+create\s+market:\s*(.+)")
        .map_err(|_| EnclaveError::GenericError("Invalid create market regex".to_string()))?;

    // Place bet: @dugong bet <amount> <coin> on|with yes|no
    let place_bet_regex =
        Regex::new(r"(?i)@\w+\s+bet\s+(\d+(?:\.\d+)?)\s+(\w+)\s+(?:on|with)\s+(yes|no)")
            .map_err(|_| EnclaveError::GenericError("Invalid place bet regex".to_string()))?;

    // Resolve market: @dugong resolve|solve yes|no
    let resolve_market_regex = Regex::new(r"(?i)@\w+\s+(?:resolve|solve)\s+(yes|no)")
        .map_err(|_| EnclaveError::GenericError("Invalid resolve market regex".to_string()))?;

    // Reward campaign (top replies): @dugong reward top 3 replies to this tweet with 5 SUI each
    let reward_top_replies_regex = Regex::new(
        r"(?i)@\w+\s+reward\s+top\s+(\d+)\s+repl(?:y|ies)\s+to\s+this\s+tweet\s+with\s+(\d+(?:\.\d+)?)\s+(\w+)\s+each",
    )
    .map_err(|_| EnclaveError::GenericError("Invalid top replies reward regex".to_string()))?;

    // Reward campaign (first hashtag): @dugong reward 10 SUI to first 10 users who tweeted #SuiFest
    let reward_first_hashtag_regex = Regex::new(
        r"(?i)@\w+\s+reward\s+(\d+(?:\.\d+)?)\s+(\w+)\s+to\s+first\s+(\d+)\s+users?\s+who\s+tweeted\s+(#\w+)",
    )
    .map_err(|_| EnclaveError::GenericError("Invalid first hashtag reward regex".to_string()))?;

    // Resolve campaign (bare): @dugong solve! / @dugong resolve  (no yes/no — checked AFTER market resolve)
    let resolve_campaign_regex = Regex::new(r"(?i)@\w+\s+(?:resolve|solve)!?\s*$")
        .map_err(|_| EnclaveError::GenericError("Invalid resolve campaign regex".to_string()))?;

    // Claim: @dugong claim [reward|payout|winnings]
    let claim_regex = Regex::new(r"(?i)@\w+\s+claim(?:\s+(?:reward|payout|winnings))?!?\s*$")
        .map_err(|_| EnclaveError::GenericError("Invalid claim regex".to_string()))?;

    // Create account: @dugong create [account] OR @dugong init [account]
    // Checked last so "create market:" takes precedence over bare "create"
    let create_account_regex = Regex::new(r"(?i)@\w+\s+(create|init)(\s+account)?")
        .map_err(|_| EnclaveError::GenericError("Invalid create account regex".to_string()))?;

    // Check reward campaign (top replies) — keyword "reward top" is distinctive
    if let Some(caps) = reward_top_replies_regex.captures(tweet_text) {
        let max_winners: u64 = caps
            .get(1)
            .and_then(|m| m.as_str().parse().ok())
            .ok_or_else(|| EnclaveError::GenericError("Invalid max winners".to_string()))?;
        let reward_amount: f64 = caps
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .ok_or_else(|| EnclaveError::GenericError("Invalid reward amount".to_string()))?;
        let coin_type = caps
            .get(3)
            .map(|m| m.as_str().to_uppercase())
            .unwrap_or_default();
        info!(
            "Detected CreateRewardCampaign (top replies): top {} {} {} each",
            max_winners, reward_amount, coin_type
        );
        return Ok(ParsedCommand::CreateRewardCampaign {
            campaign_type: 1,
            target: "replies".to_string(),
            reward_amount,
            max_winners,
            coin_type,
        });
    }

    // Check reward campaign (first hashtag)
    if let Some(caps) = reward_first_hashtag_regex.captures(tweet_text) {
        let reward_amount: f64 = caps
            .get(1)
            .and_then(|m| m.as_str().parse().ok())
            .ok_or_else(|| EnclaveError::GenericError("Invalid reward amount".to_string()))?;
        let coin_type = caps
            .get(2)
            .map(|m| m.as_str().to_uppercase())
            .unwrap_or_default();
        let max_winners: u64 = caps
            .get(3)
            .and_then(|m| m.as_str().parse().ok())
            .ok_or_else(|| EnclaveError::GenericError("Invalid max winners".to_string()))?;
        let target = caps
            .get(4)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        info!(
            "Detected CreateRewardCampaign (first hashtag): first {} {} {} for {}",
            max_winners, reward_amount, coin_type, target
        );
        return Ok(ParsedCommand::CreateRewardCampaign {
            campaign_type: 2,
            target,
            reward_amount,
            max_winners,
            coin_type,
        });
    }

    // Check create market (before create account so "create market:" wins)
    if let Some(caps) = create_market_regex.captures(tweet_text) {
        let question = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .ok_or_else(|| {
                EnclaveError::GenericError("Failed to extract market question".to_string())
            })?;
        info!("Detected CreateMarket command: {}", question);
        return Ok(ParsedCommand::CreateMarket { question });
    }

    // Check place bet
    if let Some(caps) = place_bet_regex.captures(tweet_text) {
        let amount_str = caps.get(1).map(|m| m.as_str()).unwrap_or("0");
        let amount: f64 = amount_str
            .parse()
            .map_err(|_| EnclaveError::GenericError("Invalid bet amount".to_string()))?;
        let coin_type = caps
            .get(2)
            .map(|m| m.as_str().to_uppercase())
            .unwrap_or_default();
        let side = caps
            .get(3)
            .map(|m| m.as_str().to_lowercase() == "yes")
            .unwrap_or(false);
        info!(
            "Detected PlaceBet command: {} {} side={}",
            amount, coin_type, side
        );
        return Ok(ParsedCommand::PlaceBet {
            amount,
            coin_type,
            side,
        });
    }

    // Check resolve market
    if let Some(caps) = resolve_market_regex.captures(tweet_text) {
        let outcome = caps
            .get(1)
            .map(|m| m.as_str().to_lowercase() == "yes")
            .unwrap_or(false);
        info!("Detected ResolveMarket command: outcome={}", outcome);
        return Ok(ParsedCommand::ResolveMarket { outcome });
    }

    // Check resolve campaign (bare "solve!"/"resolve" with no yes/no) — after market resolve
    if resolve_campaign_regex.is_match(tweet_text) {
        info!("Detected ResolveRewardCampaign command");
        return Ok(ParsedCommand::ResolveRewardCampaign);
    }

    // Check claim (market payout or campaign reward; disambiguated by parent tweet downstream)
    if claim_regex.is_match(tweet_text) {
        info!("Detected Claim command");
        return Ok(ParsedCommand::Claim);
    }

    // Check transfer
    if let Some(caps) = transfer_regex.captures(tweet_text) {
        let receiver_username = caps.get(3).map(|m| m.as_str().to_string()).ok_or_else(|| {
            EnclaveError::GenericError("Failed to extract receiver username".to_string())
        })?;
        info!("Detected Transfer command to @{}", receiver_username);
        return Ok(ParsedCommand::Transfer { receiver_username });
    }

    // Check create account (bare "create" or "init")
    if create_account_regex.is_match(tweet_text) {
        info!("Detected CreateAccount command");
        return Ok(ParsedCommand::CreateAccount);
    }

    Err(EnclaveError::GenericError(
        "Could not parse tweet command. Supported: 'create market: <q>', 'bet <amt> <coin> on|with yes|no', 'resolve|solve yes|no', 'send <amt> <coin> to @<user>', 'create account'".to_string()
    ))
}

/// Process create account command
async fn process_create_account_command(
    state: &Arc<AppState>,
    tweet_data: &TweetData,
    timestamp_ms: u64,
) -> Result<Json<ProcessTweetResponse>, EnclaveError> {
    // Create InitAccountPayload
    let payload = InitAccountPayload {
        xid: tweet_data.author_xid.clone().into_bytes(),
        handle: tweet_data.author_handle.clone().into_bytes(),
    };

    // Sign the payload
    let signed = to_signed_response(
        &state.eph_kp,
        payload.clone(),
        timestamp_ms,
        IntentScope::ProcessData, // Intent = 0
    );

    // Build unified response
    let response = ProcessTweetResponse {
        command_type: CommandType::CreateAccount,
        intent: 0, // INIT_ACCOUNT_INTENT
        timestamp_ms,
        signature: signed.signature,
        common: TweetCommon {
            tweet_id: tweet_data.tweet_id.clone(),
            author_xid: tweet_data.author_xid.clone(),
            author_handle: tweet_data.author_handle.clone(),
        },
        data: ProcessTweetData::CreateAccount(CreateAccountData {
            xid: tweet_data.author_xid.clone(),
            handle: tweet_data.author_handle.clone(),
        }),
    };

    info!(
        "Created ProcessTweetResponse for CreateAccount: XID={}, handle=@{}",
        tweet_data.author_xid, tweet_data.author_handle
    );

    Ok(Json(response))
}

/// Process transfer command
async fn process_transfer_command(
    state: &Arc<AppState>,
    tweet_data: &TweetData,
    receiver_username: &str,
    timestamp_ms: u64,
) -> Result<Json<ProcessTweetResponse>, EnclaveError> {
    // Parse transfer details from tweet text
    let transfer_regex = Regex::new(r"(?i)@\w+\s+send\s+(\d+(?:\.\d+)?)\s+(\w+)\s+to\s+@(\w+)")
        .map_err(|_| EnclaveError::GenericError("Invalid transfer regex".to_string()))?;

    let captures = transfer_regex.captures(&tweet_data.text).ok_or_else(|| {
        EnclaveError::GenericError("Failed to parse transfer command".to_string())
    })?;

    // Parse amount
    let amount_str = captures
        .get(1)
        .ok_or_else(|| EnclaveError::GenericError("Failed to extract amount".to_string()))?
        .as_str();

    let amount_float: f64 = amount_str
        .parse()
        .map_err(|_| EnclaveError::GenericError("Invalid amount format".to_string()))?;

    // Parse coin type
    let coin_type = captures
        .get(2)
        .ok_or_else(|| EnclaveError::GenericError("Failed to extract coin type".to_string()))?
        .as_str()
        .to_uppercase();

    // Get decimals for this coin type
    let decimals = get_coin_decimals(&coin_type);
    let multiplier = 10_u64.pow(decimals);

    // Convert amount to smallest unit based on coin decimals
    let amount_units = (amount_float * multiplier as f64) as u64;

    // Convert coin type to canonical format (matches Move type_name::get<T>())
    let canonical_coin_type = to_canonical_coin_type(&coin_type, &state.dugong_package_id);

    // Resolve receiver user ID from the fetched tweet entities first. If the
    // provider did not return mention entities, fall back to a separate user
    // lookup in the same provider.
    let to_xid = if let Some(xid) = resolve_mentioned_user_id(tweet_data, receiver_username) {
        xid
    } else {
        fetch_user_id_by_username(
            &state.twitterapi_io_base_url,
            &state.api_key,
            receiver_username,
        )
        .await?
    };

    // Create TransferPayload
    let payload = TransferPayload {
        from_xid: tweet_data.author_xid.clone().into_bytes(),
        to_xid: to_xid.clone().into_bytes(),
        amount: amount_units,
        coin_type: canonical_coin_type.clone().into_bytes(),
        tweet_id: tweet_data.tweet_id.clone().into_bytes(),
    };

    // Sign the payload with TransferCoin intent
    let signed = to_signed_response(
        &state.eph_kp,
        payload.clone(),
        timestamp_ms,
        IntentScope::TransferCoin, // Intent = 2
    );

    // Build unified response
    let response = ProcessTweetResponse {
        command_type: CommandType::Transfer,
        intent: 2, // TRANSFER_COIN_INTENT
        timestamp_ms,
        signature: signed.signature,
        common: TweetCommon {
            tweet_id: tweet_data.tweet_id.clone(),
            author_xid: tweet_data.author_xid.clone(),
            author_handle: tweet_data.author_handle.clone(),
        },
        data: ProcessTweetData::Transfer(TransferData {
            from_xid: tweet_data.author_xid.clone(),
            from_handle: tweet_data.author_handle.clone(),
            to_xid: to_xid.clone(),
            to_handle: receiver_username.to_string(),
            amount: amount_units,
            coin_type: coin_type.clone(),
        }),
    };

    info!(
        "Created ProcessTweetResponse for Transfer: {} {} from @{} to @{}",
        amount_float, coin_type, tweet_data.author_handle, receiver_username
    );

    Ok(Json(response))
}

/// Process create market command.
/// The tweet that issues the command IS the market's anchor tweet, so its ID
/// becomes `market_tweet_id` in the payload.
async fn process_create_market_command(
    state: &Arc<AppState>,
    tweet_data: &TweetData,
    question: &str,
    timestamp_ms: u64,
) -> Result<Json<ProcessTweetResponse>, EnclaveError> {
    let market_tweet_id = tweet_data.tweet_id.clone();
    let fee_bps: u16 = 100; // 1% default fee

    let payload = CreateMarketPayload {
        creator_xid: tweet_data.author_xid.clone().into_bytes(),
        market_tweet_id: market_tweet_id.clone().into_bytes(),
        question: question.as_bytes().to_vec(),
        fee_bps,
    };

    let signed = to_signed_response(
        &state.eph_kp,
        payload,
        timestamp_ms,
        IntentScope::CreateMarket,
    );

    let response = ProcessTweetResponse {
        command_type: CommandType::CreateMarket,
        intent: 5,
        timestamp_ms,
        signature: signed.signature,
        common: TweetCommon {
            tweet_id: tweet_data.tweet_id.clone(),
            author_xid: tweet_data.author_xid.clone(),
            author_handle: tweet_data.author_handle.clone(),
        },
        data: ProcessTweetData::CreateMarket(CreateMarketData {
            creator_xid: tweet_data.author_xid.clone(),
            creator_handle: tweet_data.author_handle.clone(),
            market_tweet_id: market_tweet_id.clone(),
            question: question.to_string(),
            fee_bps,
        }),
    };

    info!(
        "Created ProcessTweetResponse for CreateMarket: market_tweet_id={}, question={}",
        market_tweet_id, question
    );

    Ok(Json(response))
}

/// Process place bet command.
/// The bet is a reply to the market tweet; `parent_tweet_id` identifies the market.
async fn process_place_bet_command(
    state: &Arc<AppState>,
    tweet_data: &TweetData,
    amount_float: f64,
    coin_type: &str,
    side: bool,
    timestamp_ms: u64,
) -> Result<Json<ProcessTweetResponse>, EnclaveError> {
    let market_tweet_id = tweet_data.parent_tweet_id.clone().ok_or_else(|| {
        EnclaveError::GenericError("Bet tweet must be a reply to the market tweet".to_string())
    })?;

    let decimals = get_coin_decimals(coin_type);
    let amount = (amount_float * 10_u64.pow(decimals) as f64) as u64;
    let canonical_coin_type = to_canonical_coin_type(coin_type, &state.dugong_package_id);

    let payload = PlaceBetPayload {
        better_xid: tweet_data.author_xid.clone().into_bytes(),
        market_tweet_id: market_tweet_id.clone().into_bytes(),
        bet_tweet_id: tweet_data.tweet_id.clone().into_bytes(),
        amount,
        coin_type: canonical_coin_type.clone().into_bytes(),
        side,
    };

    let signed = to_signed_response(&state.eph_kp, payload, timestamp_ms, IntentScope::PlaceBet);

    let response = ProcessTweetResponse {
        command_type: CommandType::PlaceBet,
        intent: 6,
        timestamp_ms,
        signature: signed.signature,
        common: TweetCommon {
            tweet_id: tweet_data.tweet_id.clone(),
            author_xid: tweet_data.author_xid.clone(),
            author_handle: tweet_data.author_handle.clone(),
        },
        data: ProcessTweetData::PlaceBet(PlaceBetData {
            better_xid: tweet_data.author_xid.clone(),
            better_handle: tweet_data.author_handle.clone(),
            market_tweet_id: market_tweet_id.clone(),
            bet_tweet_id: tweet_data.tweet_id.clone(),
            amount,
            coin_type: coin_type.to_string(),
            side,
        }),
    };

    info!(
        "Created ProcessTweetResponse for PlaceBet: market={}, bet_tweet={}, {} {} side={}",
        market_tweet_id, tweet_data.tweet_id, amount_float, coin_type, side
    );

    Ok(Json(response))
}

/// Process resolve market command.
/// The resolve is a reply to the market tweet; `parent_tweet_id` identifies the market.
async fn process_resolve_market_command(
    state: &Arc<AppState>,
    tweet_data: &TweetData,
    outcome: bool,
    timestamp_ms: u64,
) -> Result<Json<ProcessTweetResponse>, EnclaveError> {
    let market_tweet_id = tweet_data.parent_tweet_id.clone().ok_or_else(|| {
        EnclaveError::GenericError("Resolve tweet must be a reply to the market tweet".to_string())
    })?;

    let payload = ResolveMarketPayload {
        resolver_xid: tweet_data.author_xid.clone().into_bytes(),
        market_tweet_id: market_tweet_id.clone().into_bytes(),
        outcome,
    };

    let signed = to_signed_response(
        &state.eph_kp,
        payload,
        timestamp_ms,
        IntentScope::ResolveMarket,
    );

    let response = ProcessTweetResponse {
        command_type: CommandType::ResolveMarket,
        intent: 7,
        timestamp_ms,
        signature: signed.signature,
        common: TweetCommon {
            tweet_id: tweet_data.tweet_id.clone(),
            author_xid: tweet_data.author_xid.clone(),
            author_handle: tweet_data.author_handle.clone(),
        },
        data: ProcessTweetData::ResolveMarket(ResolveMarketData {
            resolver_xid: tweet_data.author_xid.clone(),
            resolver_handle: tweet_data.author_handle.clone(),
            market_tweet_id: market_tweet_id.clone(),
            outcome,
        }),
    };

    info!(
        "Created ProcessTweetResponse for ResolveMarket: market={}, outcome={}",
        market_tweet_id, outcome
    );

    Ok(Json(response))
}

/// Process create reward campaign command.
/// The campaign tweet itself (`tweet_data.tweet_id`) is the campaign identifier.
#[allow(clippy::too_many_arguments)]
async fn process_create_reward_campaign_command(
    state: &Arc<AppState>,
    tweet_data: &TweetData,
    campaign_type: u8,
    target: &str,
    reward_amount_float: f64,
    max_winners: u64,
    coin_type: &str,
    timestamp_ms: u64,
) -> Result<Json<ProcessTweetResponse>, EnclaveError> {
    let campaign_tweet_id = tweet_data.tweet_id.clone();
    let decimals = get_coin_decimals(coin_type);
    let reward_amount = (reward_amount_float * 10_u64.pow(decimals) as f64) as u64;
    let canonical_coin_type = to_canonical_coin_type(coin_type, &state.dugong_package_id);

    let payload = CreateRewardCampaignPayload {
        creator_xid: tweet_data.author_xid.clone().into_bytes(),
        campaign_tweet_id: campaign_tweet_id.clone().into_bytes(),
        campaign_type,
        target: target.as_bytes().to_vec(),
        reward_amount,
        max_winners,
        coin_type: canonical_coin_type.clone().into_bytes(),
    };

    let signed = to_signed_response(
        &state.eph_kp,
        payload,
        timestamp_ms,
        IntentScope::CreateRewardCampaign,
    );

    let response = ProcessTweetResponse {
        command_type: CommandType::CreateRewardCampaign,
        intent: 8,
        timestamp_ms,
        signature: signed.signature,
        common: TweetCommon {
            tweet_id: tweet_data.tweet_id.clone(),
            author_xid: tweet_data.author_xid.clone(),
            author_handle: tweet_data.author_handle.clone(),
        },
        data: ProcessTweetData::CreateRewardCampaign(CreateRewardCampaignData {
            creator_xid: tweet_data.author_xid.clone(),
            creator_handle: tweet_data.author_handle.clone(),
            campaign_tweet_id: campaign_tweet_id.clone(),
            campaign_type,
            target: target.to_string(),
            reward_amount,
            max_winners,
            coin_type: coin_type.to_string(),
        }),
    };

    info!(
        "Created ProcessTweetResponse for CreateRewardCampaign: campaign_tweet_id={}, type={}, {} x {} {}",
        campaign_tweet_id, campaign_type, reward_amount_float, max_winners, coin_type
    );

    Ok(Json(response))
}

/// Process resolve reward campaign command.
/// The resolve is a reply to the campaign tweet; `parent_tweet_id` identifies the campaign.
async fn process_resolve_reward_campaign_command(
    state: &Arc<AppState>,
    tweet_data: &TweetData,
    timestamp_ms: u64,
) -> Result<Json<ProcessTweetResponse>, EnclaveError> {
    let campaign_tweet_id = tweet_data.parent_tweet_id.clone().ok_or_else(|| {
        EnclaveError::GenericError(
            "Resolve campaign tweet must be a reply to the campaign tweet".to_string(),
        )
    })?;

    let payload = ResolveRewardCampaignPayload {
        creator_xid: tweet_data.author_xid.clone().into_bytes(),
        campaign_tweet_id: campaign_tweet_id.clone().into_bytes(),
        solve_tweet_id: tweet_data.tweet_id.clone().into_bytes(),
    };

    let signed = to_signed_response(
        &state.eph_kp,
        payload,
        timestamp_ms,
        IntentScope::ResolveRewardCampaign,
    );

    let response = ProcessTweetResponse {
        command_type: CommandType::ResolveRewardCampaign,
        intent: 9,
        timestamp_ms,
        signature: signed.signature,
        common: TweetCommon {
            tweet_id: tweet_data.tweet_id.clone(),
            author_xid: tweet_data.author_xid.clone(),
            author_handle: tweet_data.author_handle.clone(),
        },
        data: ProcessTweetData::ResolveRewardCampaign(ResolveRewardCampaignData {
            resolver_xid: tweet_data.author_xid.clone(),
            resolver_handle: tweet_data.author_handle.clone(),
            campaign_tweet_id: campaign_tweet_id.clone(),
            solve_tweet_id: tweet_data.tweet_id.clone(),
        }),
    };

    info!(
        "Created ProcessTweetResponse for ResolveRewardCampaign: campaign={}",
        campaign_tweet_id
    );

    Ok(Json(response))
}

/// Process claim command (market payout or campaign reward).
/// The claim is a reply to the market/campaign tweet; `parent_tweet_id` is the target.
async fn process_claim_command(
    state: &Arc<AppState>,
    tweet_data: &TweetData,
    timestamp_ms: u64,
) -> Result<Json<ProcessTweetResponse>, EnclaveError> {
    let target_tweet_id = tweet_data.parent_tweet_id.clone().ok_or_else(|| {
        EnclaveError::GenericError(
            "Claim tweet must be a reply to a market or campaign tweet".to_string(),
        )
    })?;

    let payload = ClaimPayload {
        claimant_xid: tweet_data.author_xid.clone().into_bytes(),
        target_tweet_id: target_tweet_id.clone().into_bytes(),
        claim_tweet_id: tweet_data.tweet_id.clone().into_bytes(),
    };

    let signed = to_signed_response(&state.eph_kp, payload, timestamp_ms, IntentScope::Claim);

    let response = ProcessTweetResponse {
        command_type: CommandType::Claim,
        intent: 10,
        timestamp_ms,
        signature: signed.signature,
        common: TweetCommon {
            tweet_id: tweet_data.tweet_id.clone(),
            author_xid: tweet_data.author_xid.clone(),
            author_handle: tweet_data.author_handle.clone(),
        },
        data: ProcessTweetData::Claim(ClaimData {
            claimant_xid: tweet_data.author_xid.clone(),
            claimant_handle: tweet_data.author_handle.clone(),
            target_tweet_id: target_tweet_id.clone(),
            claim_tweet_id: tweet_data.tweet_id.clone(),
        }),
    };

    info!(
        "Created ProcessTweetResponse for Claim: target={}",
        target_tweet_id
    );

    Ok(Json(response))
}

/// Initialize account endpoint
/// Creates a signed InitAccountPayload for creating new Dugong accounts
pub async fn process_init_account(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ProcessDataRequest<InitAccountRequest>>,
) -> Result<Json<ProcessedDataResponse<IntentMessage<InitAccountPayload>>>, EnclaveError> {
    let xid = request.payload.xid.clone();
    info!("Initializing account for XID: {}", xid);

    let current_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| EnclaveError::GenericError(format!("Failed to get current timestamp: {}", e)))?
        .as_millis() as u64;

    let handle = request
        .payload
        .handle
        .as_deref()
        .map(|h| h.trim().trim_start_matches('@'))
        .filter(|h| !h.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            // Last-resort fallback for call sites that only know the XID.
            format!("user_{}", &xid[..std::cmp::min(xid.len(), 8)])
        });
    info!("Using handle for account init: @{}", handle);

    // Create payload
    let payload = InitAccountPayload {
        xid: xid.into_bytes(),
        handle: handle.into_bytes(),
    };

    info!(
        "Created InitAccountPayload for XID: {} with handle: {}",
        String::from_utf8_lossy(&payload.xid),
        String::from_utf8_lossy(&payload.handle)
    );

    // Sign and return the payload
    Ok(Json(to_signed_response(
        &state.eph_kp,
        payload,
        current_timestamp,
        IntentScope::ProcessData, // Intent = 0 (INIT_ACCOUNT_INTENT)
    )))
}

/// Secure link wallet endpoint
/// Verifies both Twitter access token AND wallet signature before signing
///
/// Security flow:
/// 1. Verify access_token with Twitter API → get XID
/// 2. Verify wallet_signature matches wallet_address
/// 3. Verify message contains correct XID and wallet_address
/// 4. Sign LinkWalletPayload for on-chain execution
pub async fn process_secure_link_wallet(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ProcessDataRequest<SecureLinkWalletRequest>>,
) -> Result<Json<ProcessedDataResponse<IntentMessage<LinkWalletPayload>>>, EnclaveError> {
    let req = &request.payload;
    info!(
        "Processing secure link wallet for address: {}",
        req.wallet_address
    );

    // 1. Verify access token with Twitter API and get user info (XID)
    let twitter_user =
        verify_twitter_access_token(&state.twitter_api_base_url, &req.access_token).await?;
    let xid = twitter_user.id.clone();
    info!(
        "Verified Twitter user: {} (@{})",
        xid, twitter_user.username
    );

    // 2. Verify the message format is correct
    // Expected format: "Link XID:{xid} to wallet {wallet_address} at {timestamp}"
    let expected_message = format!(
        "Link XID:{} to wallet {} at {}",
        xid, req.wallet_address, req.timestamp
    );

    if req.message != expected_message {
        return Err(EnclaveError::GenericError(format!(
            "Invalid message format. Expected: '{}', Got: '{}'",
            expected_message, req.message
        )));
    }
    info!("Message format verified");

    // 3. Verify wallet signature
    verify_sui_wallet_signature(&req.wallet_address, &req.message, &req.wallet_signature)?;
    info!(
        "Wallet signature verified for address: {}",
        req.wallet_address
    );

    // 4. Check timestamp is not too old (5 minutes max)
    let current_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| EnclaveError::GenericError(format!("Failed to get current timestamp: {}", e)))?
        .as_millis() as u64;

    let max_age_ms = 5 * 60 * 1000; // 5 minutes
    if current_timestamp > req.timestamp + max_age_ms {
        return Err(EnclaveError::GenericError(
            "Message timestamp is too old (max 5 minutes)".to_string(),
        ));
    }
    info!("Timestamp verified");

    // 5. Parse wallet address
    let address_hex = if req.wallet_address.starts_with("0x") {
        &req.wallet_address[2..]
    } else {
        &req.wallet_address
    };

    let address_bytes = hex::decode(address_hex)
        .map_err(|e| EnclaveError::GenericError(format!("Invalid Sui address format: {}", e)))?;

    if address_bytes.len() != 32 {
        return Err(EnclaveError::GenericError(format!(
            "Invalid Sui address length: expected 32 bytes, got {}",
            address_bytes.len()
        )));
    }

    // 6. Create and sign payload
    // Convert Vec<u8> to [u8; 32] - we already verified length is 32
    let owner_address: [u8; 32] = address_bytes.try_into().map_err(|_| {
        EnclaveError::GenericError("Failed to convert address to [u8; 32]".to_string())
    })?;

    let payload = LinkWalletPayload {
        xid: xid.into_bytes(),
        owner_address,
    };

    info!(
        "Created secure LinkWalletPayload for XID: {} -> wallet: {}",
        String::from_utf8_lossy(&payload.xid),
        req.wallet_address
    );

    // Sign and return the payload
    Ok(Json(to_signed_response(
        &state.eph_kp,
        payload,
        current_timestamp,
        IntentScope::LinkWallet,
    )))
}

/// Twitter user info response
#[derive(Debug, Deserialize)]
struct TwitterUserInfo {
    id: String,
    username: String,
}

/// Verify Twitter access token and return user info
async fn verify_twitter_access_token(
    base_url: &str,
    access_token: &str,
) -> Result<TwitterUserInfo, EnclaveError> {
    let client = reqwest::Client::new();

    // Call Twitter API to verify token and get user info
    let url = format!("{}/2/users/me", base_url);

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .map_err(|e| {
            EnclaveError::GenericError(format!("Failed to verify Twitter access token: {}", e))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(EnclaveError::GenericError(format!(
            "Twitter API returned error {}: {}",
            status, body
        )));
    }

    let json: serde_json::Value = response.json().await.map_err(|e| {
        EnclaveError::GenericError(format!("Failed to parse Twitter response: {}", e))
    })?;

    let data = json.get("data").ok_or_else(|| {
        EnclaveError::GenericError("Twitter response missing 'data' field".to_string())
    })?;

    let id = data
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| EnclaveError::GenericError("Twitter response missing user ID".to_string()))?
        .to_string();

    let username = data
        .get("username")
        .and_then(|v| v.as_str())
        .ok_or_else(|| EnclaveError::GenericError("Twitter response missing username".to_string()))?
        .to_string();

    Ok(TwitterUserInfo { id, username })
}

/// Verify Sui wallet signature
///
/// The signature should be created by the wallet signing the message.
/// For Sui, we use personal message signing which prepends intent bytes.
fn verify_sui_wallet_signature(
    wallet_address: &str,
    message: &str,
    signature_base64: &str,
) -> Result<(), EnclaveError> {
    use fastcrypto::ed25519::{Ed25519PublicKey, Ed25519Signature};
    use fastcrypto::encoding::{Base64, Encoding};
    use fastcrypto::traits::{ToFromBytes, VerifyingKey};

    // Decode signature (format: flag || signature || public_key)
    let sig_bytes = Base64::decode(signature_base64)
        .map_err(|e| EnclaveError::GenericError(format!("Failed to decode signature: {}", e)))?;

    // Sui signature format: 1 byte flag + 64 bytes signature + 32 bytes pubkey = 97 bytes for Ed25519
    if sig_bytes.len() < 97 {
        return Err(EnclaveError::GenericError(format!(
            "Invalid signature length: expected >= 97 bytes for Ed25519, got {}",
            sig_bytes.len()
        )));
    }

    let flag = sig_bytes[0];

    // Flag 0x00 = Ed25519
    if flag != 0x00 {
        return Err(EnclaveError::GenericError(format!(
            "Unsupported signature scheme: flag=0x{:02x}. Only Ed25519 (0x00) is supported.",
            flag
        )));
    }

    let signature_bytes = &sig_bytes[1..65];
    let pubkey_bytes = &sig_bytes[65..97];

    // Verify public key matches wallet address
    // Sui address = blake2b_256(flag || pubkey)[0..32]
    use fastcrypto::hash::{Blake2b256, HashFunction};
    let mut data = vec![flag];
    data.extend_from_slice(pubkey_bytes);
    let computed_address = Blake2b256::digest(&data);
    let computed_address_hex = hex::encode(computed_address.as_ref());

    let expected_address = wallet_address.strip_prefix("0x").unwrap_or(wallet_address);

    if computed_address_hex.to_lowercase() != expected_address.to_lowercase() {
        return Err(EnclaveError::GenericError(format!(
            "Public key does not match wallet address. Expected: {}, Got: {}",
            expected_address, computed_address_hex
        )));
    }

    // Sui signPersonalMessage signs:
    // blake2b_256(intent(PersonalMessage) || BCS(vector<u8>(message_bytes))).
    // The dApp kit passes raw UTF-8 bytes to signPersonalMessage, which BCS
    // encodes as a vector<u8> before adding the intent domain separator.
    let message_bcs = bcs::to_bytes(&message.as_bytes().to_vec()).map_err(|e| {
        EnclaveError::GenericError(format!("Failed to BCS serialize message bytes: {}", e))
    })?;
    let mut signing_input = Vec::with_capacity(3 + message_bcs.len());
    signing_input.extend_from_slice(&[3, 0, 0]); // PersonalMessage, V0, Sui
    signing_input.extend_from_slice(&message_bcs);
    let digest = Blake2b256::digest(&signing_input);

    let pubkey = Ed25519PublicKey::from_bytes(pubkey_bytes)
        .map_err(|e| EnclaveError::GenericError(format!("Invalid public key: {}", e)))?;

    let signature = Ed25519Signature::from_bytes(signature_bytes)
        .map_err(|e| EnclaveError::GenericError(format!("Invalid signature: {}", e)))?;

    pubkey
        .verify(digest.as_ref(), &signature)
        .map_err(|e| EnclaveError::GenericError(format!("Signature verification failed: {}", e)))?;

    Ok(())
}

fn resolve_mentioned_user_id(tweet_data: &TweetData, username: &str) -> Option<String> {
    tweet_data
        .mentions
        .iter()
        .find(|mention| mention.username.eq_ignore_ascii_case(username))
        .and_then(|mention| mention.id.clone())
}

/// Fetch Twitter user ID by username via Tweeter API.
async fn fetch_user_id_by_username(
    base_url: &str,
    api_key: &str,
    username: &str,
) -> Result<String, EnclaveError> {
    let client = reqwest::Client::new();

    info!("Fetching user ID for @{}", username);

    let response = client
        .get(format!("{}/twitter/user/info", base_url))
        .header("X-API-Key", api_key)
        .query(&[("userName", username)])
        .send()
        .await
        .map_err(|e| EnclaveError::GenericError(format!("Failed to fetch user info: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(EnclaveError::GenericError(format!(
            "Tweeter API returned error {} for @{}: {}",
            status, username, body
        )));
    }

    let response = response
        .json::<TweeterUserInfoResponse>()
        .await
        .map_err(|e| {
            EnclaveError::GenericError(format!("Failed to parse user info response: {}", e))
        })?;

    if !response.status.eq_ignore_ascii_case("success") {
        return Err(EnclaveError::GenericError(format!(
            "Tweeter API error when fetching user {}: {}",
            username,
            response.error_message()
        )));
    }

    let user_id = response.data.map(|user| user.id).ok_or_else(|| {
        EnclaveError::GenericError(format!("Failed to extract user ID for @{}", username))
    })?;

    info!("Found user ID: {} for @{}", user_id, username);

    Ok(user_id)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::common::IntentMessage;
    use fastcrypto::encoding::{Encoding, Hex};

    #[test]
    fn test_transfer_payload_serde() {
        // Test that serialization is consistent with Move contract
        // All strings must be Vec<u8> to match Move's vector<u8>
        let payload = TransferPayload {
            from_xid: b"123456789".to_vec(),
            to_xid: b"987654321".to_vec(),
            amount: 5_000_000_000, // 5 SUI in MIST
            coin_type: to_canonical_coin_type("SUI", "0x0").into_bytes(),
            tweet_id: b"1234567890123456789".to_vec(),
        };

        let timestamp = 1744038900000u64;
        let intent_msg = IntentMessage::new(payload, timestamp, IntentScope::ProcessData);

        // Serialize to BCS
        let signing_payload = bcs::to_bytes(&intent_msg).expect("should not fail");

        // Print hex for debugging
        println!("BCS hex: {}", Hex::encode(&signing_payload));

        // Verify it can be deserialized
        let _deserialized: IntentMessage<TransferPayload> =
            bcs::from_bytes(&signing_payload).expect("should deserialize");
    }

    #[test]
    fn test_transfer_actual_values() {
        // Test with ACTUAL values from enclave response to debug signature verification
        // This should produce the EXACT BCS that the signature was created for
        let from_xid = "1985975069177511936";
        let to_xid = "1786143256721436672";
        let amount: u64 = 10000000;
        let tweet_id = "1996063548280299962";
        let timestamp_ms: u64 = 1764736025823; // FRESH timestamp from recent enclave call
        let signature_hex = "a99d5bbd8ac60a17d1e21404c41f192ea7e8496255ab3e75e05d1e18d37249432e0b85fe00d083e1682020917d4baf923595ad2063b61e342ff15129ef06350b";
        let pk_hex = "09cba98f149884f6d0ec0b06045e966c6b6e2043eca3643712d169dd935d6804";

        let canonical_coin_type = to_canonical_coin_type("SUI", "0x0");

        let payload = TransferPayload {
            from_xid: from_xid.as_bytes().to_vec(),
            to_xid: to_xid.as_bytes().to_vec(),
            amount,
            coin_type: canonical_coin_type.clone().into_bytes(),
            tweet_id: tweet_id.as_bytes().to_vec(),
        };

        // Use TransferCoin intent (= 2), NOT ProcessData (= 0)
        let intent_msg = IntentMessage::new(payload, timestamp_ms, IntentScope::TransferCoin);

        // Serialize to BCS
        let signing_payload = bcs::to_bytes(&intent_msg).expect("should not fail");

        // Print for comparison
        println!("=== Enclave-side BCS output ===");
        println!("from_xid: {} ({} bytes)", from_xid, from_xid.len());
        println!("to_xid: {} ({} bytes)", to_xid, to_xid.len());
        println!("amount: {}", amount);
        println!(
            "coin_type: {} ({} bytes)",
            canonical_coin_type,
            canonical_coin_type.len()
        );
        println!("tweet_id: {} ({} bytes)", tweet_id, tweet_id.len());
        println!("timestamp_ms: {}", timestamp_ms);
        println!("intent: {} (TransferCoin)", 2);
        println!();
        println!("BCS hex: {}", Hex::encode(&signing_payload));
        println!("BCS length: {} bytes", signing_payload.len());

        // Verify signature
        use fastcrypto::ed25519::{Ed25519PublicKey, Ed25519Signature};
        use fastcrypto::traits::{ToFromBytes, VerifyingKey};

        let pk_bytes = Hex::decode(pk_hex).expect("Invalid pk hex");
        let sig_bytes = Hex::decode(signature_hex).expect("Invalid sig hex");

        let public_key = Ed25519PublicKey::from_bytes(&pk_bytes).expect("Invalid public key");
        let signature = Ed25519Signature::from_bytes(&sig_bytes).expect("Invalid signature");

        match public_key.verify(&signing_payload, &signature) {
            Ok(_) => println!("✅ Signature VALID!"),
            Err(e) => {
                println!("❌ Signature INVALID: {:?}", e);
                println!("This means enclave signed a DIFFERENT BCS");
            }
        }

        // Also test using to_signed_response to compare
        println!("\n=== Test with to_signed_response ===");
        use fastcrypto::ed25519::Ed25519KeyPair;
        use fastcrypto::traits::KeyPair as FcKeyPair;
        use rand::SeedableRng;

        // Create a dummy keypair (since we can't verify without the real one)
        let seed = [0u8; 32];
        let dummy_kp = Ed25519KeyPair::generate(&mut rand::rngs::StdRng::from_seed(seed));

        let payload2 = TransferPayload {
            from_xid: from_xid.as_bytes().to_vec(),
            to_xid: to_xid.as_bytes().to_vec(),
            amount,
            coin_type: canonical_coin_type.clone().into_bytes(),
            tweet_id: tweet_id.as_bytes().to_vec(),
        };

        let signed =
            to_signed_response(&dummy_kp, payload2, timestamp_ms, IntentScope::TransferCoin);
        println!("to_signed_response signature: {}", signed.signature);
    }

    #[test]
    fn test_transfer_regex() {
        let regex = Regex::new(r"@\w+\s+send\s+(\d+(?:\.\d+)?)\s+(\w+)\s+to\s+@(\w+)").unwrap();

        // Test case 1: Integer amount
        let tweet1 = "@Dugong send 5 SUI to @alice";
        let captures1 = regex.captures(tweet1).unwrap();
        assert_eq!(captures1.get(1).unwrap().as_str(), "5");
        assert_eq!(captures1.get(2).unwrap().as_str(), "SUI");
        assert_eq!(captures1.get(3).unwrap().as_str(), "alice");

        // Test case 2: Decimal amount
        let tweet2 = "@dugong send 10.5 USDC to @bob";
        let captures2 = regex.captures(tweet2).unwrap();
        assert_eq!(captures2.get(1).unwrap().as_str(), "10.5");
        assert_eq!(captures2.get(2).unwrap().as_str(), "USDC");
        assert_eq!(captures2.get(3).unwrap().as_str(), "bob");

        // Test case 3: Should not match invalid format
        let tweet3 = "Just sending 5 SUI";
        assert!(regex.captures(tweet3).is_none());
    }

    // ========================================================================
    // Tests for unified /process_tweet endpoint (NEW)
    // ========================================================================

    #[test]
    fn test_parse_tweet_command_type_create_account() {
        // Test various create account formats
        let test_cases = vec![
            "@dugong create account",
            "@dugong create",
            "@dugong init account",
            "@dugong init",
            "@Dugong CREATE ACCOUNT", // Case insensitive
            "@DUGONG Init Account",
        ];

        for tweet in test_cases {
            let result = parse_tweet_command_type(tweet, "123456789");
            assert!(result.is_ok(), "Failed for tweet: {}", tweet);
            match result.unwrap() {
                ParsedCommand::CreateAccount => {}
                other => panic!(
                    "Expected CreateAccount, got {:?} for tweet: {}",
                    other, tweet
                ),
            }
        }
    }

    #[test]
    fn test_parse_tweet_command_type_transfer() {
        // Test various transfer formats
        let test_cases = vec![
            ("@dugong send 100 SUI to @bob", "bob"),
            ("@dugong send 10.5 USDC to @alice", "alice"),
            ("@Dugong SEND 50 sui TO @Charlie", "Charlie"),
            ("@dugong send 0.001 ETH to @user123", "user123"),
        ];

        for (tweet, expected_receiver) in test_cases {
            let result = parse_tweet_command_type(tweet, "123456789");
            assert!(result.is_ok(), "Failed for tweet: {}", tweet);
            match result.unwrap() {
                ParsedCommand::Transfer { receiver_username } => {
                    assert_eq!(
                        receiver_username, expected_receiver,
                        "Wrong receiver for tweet: {}",
                        tweet
                    );
                }
                other => panic!("Expected Transfer, got {:?} for tweet: {}", other, tweet),
            }
        }
    }

    #[test]
    fn test_parse_tweet_command_type_invalid() {
        // Test invalid formats that should fail
        let test_cases = vec![
            "hello world",
            "@dugong",
            "@dugong invalid command",
            "send 100 SUI to @bob", // Missing @dugong
        ];

        for tweet in test_cases {
            let result = parse_tweet_command_type(tweet, "123456789");
            assert!(result.is_err(), "Should have failed for tweet: {}", tweet);
        }
    }

    #[test]
    fn test_command_type_serialization() {
        // Test CommandType serialization (for JSON response)
        assert_eq!(
            serde_json::to_string(&CommandType::CreateAccount).unwrap(),
            r#""create_account""#
        );
        assert_eq!(
            serde_json::to_string(&CommandType::Transfer).unwrap(),
            r#""transfer""#
        );
    }

    #[test]
    fn test_parse_tweet_command_type_create_market() {
        let test_cases = vec![
            (
                "@DugongWallet create market: BTC over 100K before March?",
                "BTC over 100K before March?",
            ),
            (
                "@dugongwallet CREATE MARKET: Will ETH flip BTC?",
                "Will ETH flip BTC?",
            ),
            (
                "@DugongWallet create market:  leading spaces trimmed  ",
                "leading spaces trimmed",
            ),
        ];

        for (tweet, expected_question) in test_cases {
            let result = parse_tweet_command_type(tweet, "123");
            assert!(result.is_ok(), "Failed for tweet: {}", tweet);
            match result.unwrap() {
                ParsedCommand::CreateMarket { question } => {
                    assert_eq!(
                        question.trim_end(),
                        expected_question,
                        "Wrong question for: {}",
                        tweet
                    );
                }
                other => panic!("Expected CreateMarket, got {:?} for: {}", other, tweet),
            }
        }
    }

    #[test]
    fn test_parse_tweet_command_type_place_bet() {
        let test_cases = vec![
            ("@DugongWallet bet 5 SUI on yes", 5.0, "SUI", true),
            ("@DugongWallet bet 10.5 USDC with no", 10.5, "USDC", false),
            ("@DugongWallet BET 100 WAL ON YES", 100.0, "WAL", true),
        ];

        for (tweet, expected_amount, expected_coin, expected_side) in test_cases {
            let result = parse_tweet_command_type(tweet, "123");
            assert!(result.is_ok(), "Failed for tweet: {}", tweet);
            match result.unwrap() {
                ParsedCommand::PlaceBet {
                    amount,
                    coin_type,
                    side,
                } => {
                    assert!(
                        (amount - expected_amount).abs() < 0.001,
                        "Wrong amount for: {}",
                        tweet
                    );
                    assert_eq!(coin_type, expected_coin, "Wrong coin for: {}", tweet);
                    assert_eq!(side, expected_side, "Wrong side for: {}", tweet);
                }
                other => panic!("Expected PlaceBet, got {:?} for: {}", other, tweet),
            }
        }
    }

    #[test]
    fn test_parse_tweet_command_type_resolve_market() {
        let test_cases = vec![
            ("@DugongWallet resolve yes", true),
            ("@DugongWallet resolve no", false),
            ("@DugongWallet solve yes", true),
            ("@DugongWallet RESOLVE YES", true),
        ];

        for (tweet, expected_outcome) in test_cases {
            let result = parse_tweet_command_type(tweet, "123");
            assert!(result.is_ok(), "Failed for tweet: {}", tweet);
            match result.unwrap() {
                ParsedCommand::ResolveMarket { outcome } => {
                    assert_eq!(outcome, expected_outcome, "Wrong outcome for: {}", tweet);
                }
                other => panic!("Expected ResolveMarket, got {:?} for: {}", other, tweet),
            }
        }
    }

    #[test]
    fn test_create_market_payload_bcs_roundtrip() {
        let payload = CreateMarketPayload {
            creator_xid: b"123456789".to_vec(),
            market_tweet_id: b"1800000000000000001".to_vec(),
            question: b"Will BTC hit 100K?".to_vec(),
            fee_bps: 100,
        };

        let timestamp = 1744038900000u64;
        let intent_msg = IntentMessage::new(payload, timestamp, IntentScope::CreateMarket);
        let bcs_bytes = bcs::to_bytes(&intent_msg).expect("BCS serialize failed");
        let _roundtrip: IntentMessage<CreateMarketPayload> =
            bcs::from_bytes(&bcs_bytes).expect("BCS deserialize failed");
        println!("CreateMarketPayload BCS: {}", Hex::encode(&bcs_bytes));
    }

    #[test]
    fn test_place_bet_payload_bcs_roundtrip() {
        let payload = PlaceBetPayload {
            better_xid: b"987654321".to_vec(),
            market_tweet_id: b"1800000000000000001".to_vec(),
            bet_tweet_id: b"1800000000000000002".to_vec(),
            amount: 5_000_000_000,
            coin_type: to_canonical_coin_type("SUI", "0x0").into_bytes(),
            side: true,
        };

        let timestamp = 1744038900000u64;
        let intent_msg = IntentMessage::new(payload, timestamp, IntentScope::PlaceBet);
        let bcs_bytes = bcs::to_bytes(&intent_msg).expect("BCS serialize failed");
        let _roundtrip: IntentMessage<PlaceBetPayload> =
            bcs::from_bytes(&bcs_bytes).expect("BCS deserialize failed");
        println!("PlaceBetPayload BCS: {}", Hex::encode(&bcs_bytes));
    }

    #[test]
    fn test_resolve_market_payload_bcs_roundtrip() {
        let payload = ResolveMarketPayload {
            resolver_xid: b"123456789".to_vec(),
            market_tweet_id: b"1800000000000000001".to_vec(),
            outcome: true,
        };

        let timestamp = 1744038900000u64;
        let intent_msg = IntentMessage::new(payload, timestamp, IntentScope::ResolveMarket);
        let bcs_bytes = bcs::to_bytes(&intent_msg).expect("BCS serialize failed");
        let _roundtrip: IntentMessage<ResolveMarketPayload> =
            bcs::from_bytes(&bcs_bytes).expect("BCS deserialize failed");
        println!("ResolveMarketPayload BCS: {}", Hex::encode(&bcs_bytes));
    }

    #[test]
    fn test_parse_reward_campaign_top_replies() {
        let result = parse_tweet_command_type(
            "@DugongWallet reward top 3 replies to this tweet with 5 SUI each",
            "123",
        );
        match result.unwrap() {
            ParsedCommand::CreateRewardCampaign {
                campaign_type,
                target,
                reward_amount,
                max_winners,
                coin_type,
            } => {
                assert_eq!(campaign_type, 1);
                assert_eq!(target, "replies");
                assert!((reward_amount - 5.0).abs() < 0.001);
                assert_eq!(max_winners, 3);
                assert_eq!(coin_type, "SUI");
            }
            other => panic!("Expected CreateRewardCampaign, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_reward_campaign_first_hashtag() {
        let result = parse_tweet_command_type(
            "@DugongWallet reward 10 SUI to first 10 users who tweeted #SuiFest",
            "123",
        );
        match result.unwrap() {
            ParsedCommand::CreateRewardCampaign {
                campaign_type,
                target,
                reward_amount,
                max_winners,
                coin_type,
            } => {
                assert_eq!(campaign_type, 2);
                assert_eq!(target, "#SuiFest");
                assert!((reward_amount - 10.0).abs() < 0.001);
                assert_eq!(max_winners, 10);
                assert_eq!(coin_type, "SUI");
            }
            other => panic!("Expected CreateRewardCampaign, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_resolve_campaign_vs_market() {
        // Bare solve! → campaign resolve
        match parse_tweet_command_type("@DugongWallet solve!", "123").unwrap() {
            ParsedCommand::ResolveRewardCampaign => {}
            other => panic!("Expected ResolveRewardCampaign, got {:?}", other),
        }
        // solve yes → market resolve (must still win)
        match parse_tweet_command_type("@DugongWallet solve yes", "123").unwrap() {
            ParsedCommand::ResolveMarket { outcome } => assert!(outcome),
            other => panic!("Expected ResolveMarket, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_claim() {
        for tweet in [
            "@DugongWallet claim",
            "@DugongWallet claim reward",
            "@DugongWallet CLAIM!",
        ] {
            match parse_tweet_command_type(tweet, "123").unwrap() {
                ParsedCommand::Claim => {}
                other => panic!("Expected Claim, got {:?} for {}", other, tweet),
            }
        }
    }

    #[test]
    fn test_reward_campaign_payloads_bcs_roundtrip() {
        let timestamp = 1744038900000u64;

        let create = CreateRewardCampaignPayload {
            creator_xid: b"123456789".to_vec(),
            campaign_tweet_id: b"1800000000000000001".to_vec(),
            campaign_type: 1,
            target: b"replies".to_vec(),
            reward_amount: 5_000_000_000,
            max_winners: 3,
            coin_type: to_canonical_coin_type("SUI", "0x0").into_bytes(),
        };
        let msg = IntentMessage::new(create, timestamp, IntentScope::CreateRewardCampaign);
        let bytes = bcs::to_bytes(&msg).expect("BCS serialize failed");
        let _rt: IntentMessage<CreateRewardCampaignPayload> =
            bcs::from_bytes(&bytes).expect("BCS deserialize failed");

        let resolve = ResolveRewardCampaignPayload {
            creator_xid: b"123456789".to_vec(),
            campaign_tweet_id: b"1800000000000000001".to_vec(),
            solve_tweet_id: b"1800000000000000009".to_vec(),
        };
        let msg = IntentMessage::new(resolve, timestamp, IntentScope::ResolveRewardCampaign);
        let bytes = bcs::to_bytes(&msg).expect("BCS serialize failed");
        let _rt: IntentMessage<ResolveRewardCampaignPayload> =
            bcs::from_bytes(&bytes).expect("BCS deserialize failed");

        let claim = ClaimPayload {
            claimant_xid: b"987654321".to_vec(),
            target_tweet_id: b"1800000000000000001".to_vec(),
            claim_tweet_id: b"1800000000000000010".to_vec(),
        };
        let msg = IntentMessage::new(claim, timestamp, IntentScope::Claim);
        let bytes = bcs::to_bytes(&msg).expect("BCS serialize failed");
        let _rt: IntentMessage<ClaimPayload> =
            bcs::from_bytes(&bytes).expect("BCS deserialize failed");
    }

    #[test]
    fn test_create_market_not_confused_with_create_account() {
        // "create market:" must NOT match as CreateAccount
        let result = parse_tweet_command_type("@DugongWallet create market: Some question", "123");
        match result.unwrap() {
            ParsedCommand::CreateMarket { .. } => {}
            other => panic!("Expected CreateMarket, got {:?}", other),
        }

        // Bare "create" still resolves to CreateAccount
        let result = parse_tweet_command_type("@DugongWallet create account", "123");
        match result.unwrap() {
            ParsedCommand::CreateAccount => {}
            other => panic!("Expected CreateAccount, got {:?}", other),
        }
    }

    #[test]
    fn test_process_tweet_response_serialization() {
        // Test full response serialization
        let response = ProcessTweetResponse {
            command_type: CommandType::Transfer,
            intent: 2,
            timestamp_ms: 1700000000000,
            signature: "abc123".to_string(),
            common: TweetCommon {
                tweet_id: "123456789".to_string(),
                author_xid: "111222333".to_string(),
                author_handle: "alice".to_string(),
            },
            data: ProcessTweetData::Transfer(TransferData {
                from_xid: "111222333".to_string(),
                from_handle: "alice".to_string(),
                to_xid: "444555666".to_string(),
                to_handle: "bob".to_string(),
                amount: 1000000000,
                coin_type: "SUI".to_string(),
            }),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""command_type":"transfer""#));
        assert!(json.contains(r#""intent":2"#));
        assert!(json.contains(r#""tweet_id":"123456789""#));

        // Verify it can be deserialized back
        let parsed: ProcessTweetResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.command_type, CommandType::Transfer);
        assert_eq!(parsed.intent, 2);
    }
}
