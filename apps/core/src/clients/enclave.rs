use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::constants::enclave;

// ============================================================================
// NEW: Unified /process_tweet types (simplified architecture)
// ============================================================================

/// Command types returned by process_tweet endpoint
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CommandType {
    CreateAccount,
    Transfer,
    UpdateHandle,
    CreateMarket,
    PlaceBet,
    ResolveMarket,
}

/// Common tweet metadata
#[derive(Debug, Clone, Deserialize)]
pub struct TweetCommon {
    pub tweet_id: String,
    pub author_xid: String,
    pub author_handle: String,
}

/// Data for create_account command
#[derive(Debug, Clone, Deserialize)]
pub struct CreateAccountData {
    pub xid: String,
    pub handle: String,
}

/// Data for transfer command
#[derive(Debug, Clone, Deserialize)]
pub struct TransferData {
    pub from_xid: String,
    pub from_handle: String,
    pub to_xid: String,
    pub to_handle: String,
    pub amount: u64,
    pub coin_type: String,
}

/// Data for create_market command
#[derive(Debug, Clone, Deserialize)]
pub struct CreateMarketData {
    pub creator_xid: String,
    pub creator_handle: String,
    pub market_tweet_id: String,
    pub question: String,
    pub fee_bps: u16,
}

/// Data for place_bet command
#[derive(Debug, Clone, Deserialize)]
pub struct PlaceBetData {
    pub better_xid: String,
    pub better_handle: String,
    pub market_tweet_id: String,
    pub bet_tweet_id: String,
    pub amount: u64,
    pub coin_type: String,
    pub side: bool,
}

/// Data for resolve_market command
#[derive(Debug, Clone, Deserialize)]
pub struct ResolveMarketData {
    pub resolver_xid: String,
    pub resolver_handle: String,
    pub market_tweet_id: String,
    pub outcome: bool,
}

/// Unified response from /process_tweet endpoint
#[derive(Debug, Clone, Deserialize)]
pub struct ProcessTweetResponse {
    pub command_type: CommandType,
    pub intent: u8,
    pub timestamp_ms: u64,
    pub signature: String,
    pub common: TweetCommon,
    pub data: serde_json::Value, // Dynamic based on command_type
}

/// Request for /process_tweet endpoint
#[derive(Debug, Serialize)]
pub struct ProcessTweetRequest {
    pub tweet_url: String,
}

/// REST client for Nautilus xWallet enclave endpoints.
#[derive(Clone)]
pub struct EnclaveClient {
    base_url: String,
    http: Client,
}

impl EnclaveClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: Client::new(),
        }
    }

    #[allow(dead_code)]
    pub async fn health_check(&self) -> Result<HealthCheckResponse> {
        let url = self.url(enclave::HEALTH_CHECK_ENDPOINT);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("enclave health_check request failed")?;

        Self::parse_response(resp).await
    }

    #[allow(dead_code)]
    pub async fn get_attestation(&self) -> Result<AttestationResponse> {
        let url = self.url(enclave::GET_ATTESTATION_ENDPOINT);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("enclave get_attestation request failed")?;

        Self::parse_response(resp).await
    }

    // ========================================================================
    // NEW: Unified /process_tweet method (simplified architecture)
    // ========================================================================

    /// Process tweet via unified endpoint
    /// Returns command_type and signed payload for all tweet-based commands
    pub async fn process_tweet(&self, tweet_url: &str) -> Result<ProcessTweetResponse> {
        self.post(
            enclave::PROCESS_TWEET_ENDPOINT,
            &ProcessDataRequest {
                payload: ProcessTweetRequest {
                    tweet_url: tweet_url.to_string(),
                },
            },
            "process_tweet",
        )
        .await
    }

    /// Parse transfer data from ProcessTweetResponse
    pub fn parse_transfer_data(response: &ProcessTweetResponse) -> Result<TransferData> {
        serde_json::from_value(response.data.clone())
            .context("Failed to parse transfer data from process_tweet response")
    }

    /// Parse create account data from ProcessTweetResponse
    pub fn parse_create_account_data(response: &ProcessTweetResponse) -> Result<CreateAccountData> {
        serde_json::from_value(response.data.clone())
            .context("Failed to parse create account data from process_tweet response")
    }

    /// Parse create market data from ProcessTweetResponse
    pub fn parse_create_market_data(response: &ProcessTweetResponse) -> Result<CreateMarketData> {
        serde_json::from_value(response.data.clone())
            .context("Failed to parse create market data from process_tweet response")
    }

    /// Parse place bet data from ProcessTweetResponse
    pub fn parse_place_bet_data(response: &ProcessTweetResponse) -> Result<PlaceBetData> {
        serde_json::from_value(response.data.clone())
            .context("Failed to parse place bet data from process_tweet response")
    }

    /// Parse resolve market data from ProcessTweetResponse
    pub fn parse_resolve_market_data(response: &ProcessTweetResponse) -> Result<ResolveMarketData> {
        serde_json::from_value(response.data.clone())
            .context("Failed to parse resolve market data from process_tweet response")
    }

    // ========================================================================
    // Non-tweet methods (still needed for specific flows)
    // ========================================================================

    /// Sign init account by XID (for auto-creating recipient accounts)
    pub async fn sign_init_account(&self, xid: &str) -> Result<SignedIntent<InitAccountPayload>> {
        self.post(
            enclave::PROCESS_INIT_ACCOUNT_ENDPOINT,
            &ProcessDataRequest {
                payload: InitAccountRequest {
                    xid: xid.to_string(),
                },
            },
            "process_init_account",
        )
        .await
    }

    /// Secure link wallet with Twitter access token and wallet signature verification
    /// Used for dApp wallet linking flow (not tweet-based)
    ///
    /// # Arguments
    /// * `access_token` - Twitter OAuth2 access token
    /// * `wallet_address` - Sui wallet address (0x...)
    /// * `wallet_signature` - Signature of the message by wallet (base64)
    /// * `message` - The message that was signed
    /// * `timestamp` - Timestamp when message was created
    pub async fn sign_secure_link_wallet(
        &self,
        access_token: &str,
        wallet_address: &str,
        wallet_signature: &str,
        message: &str,
        timestamp: u64,
    ) -> Result<SignedIntent<LinkWalletPayload>> {
        self.post(
            enclave::PROCESS_SECURE_LINK_WALLET_ENDPOINT,
            &ProcessDataRequest {
                payload: SecureLinkWalletRequest {
                    access_token: access_token.to_string(),
                    wallet_address: wallet_address.to_string(),
                    wallet_signature: wallet_signature.to_string(),
                    message: message.to_string(),
                    timestamp,
                },
            },
            "process_secure_link_wallet",
        )
        .await
    }

    async fn post<TReq: Serialize, TResp: DeserializeOwned>(
        &self,
        path: &str,
        body: &TReq,
        label: &str,
    ) -> Result<TResp> {
        let url = self.url(path);
        let resp = self
            .http
            .post(&url)
            .json(body)
            .send()
            .await
            .with_context(|| format!("enclave {} request failed", label))?;

        Self::parse_response(resp).await
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn parse_response<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());

        if !status.is_success() {
            return Err(anyhow!("enclave returned {}: {}", status, text));
        }

        serde_json::from_str(&text)
            .with_context(|| format!("failed to parse enclave response: {}", text))
    }
}

// ============================================================================
// Request types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessDataRequest<T> {
    pub payload: T,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InitAccountRequest {
    pub xid: String,
}

/// Secure link wallet request - verifies both Twitter token and wallet signature
#[derive(Debug, Serialize, Deserialize)]
pub struct SecureLinkWalletRequest {
    pub access_token: String,     // Twitter OAuth2 access token
    pub wallet_address: String,   // Sui wallet address (0x...)
    pub wallet_signature: String, // Signature of the message by wallet (base64)
    pub message: String,          // The message that was signed
    pub timestamp: u64,           // Timestamp when message was created
}

// ============================================================================
// Response types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct SignedIntent<T> {
    pub response: IntentMessage<T>,
    pub signature: String,
}

#[derive(Debug, Deserialize)]
pub struct IntentMessage<T> {
    pub intent: u8,
    pub timestamp_ms: u64,
    pub data: T,
}

#[derive(Debug, Deserialize)]
pub struct InitAccountPayload {
    pub xid: Vec<u8>,
    pub handle: Vec<u8>,
}

#[derive(Debug, Deserialize)]
pub struct LinkWalletPayload {
    pub xid: Vec<u8>,
    pub owner_address: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct HealthCheckResponse {
    pub pk: String,
    pub endpoints_status: HashMap<String, bool>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AttestationResponse {
    pub attestation: String,
}
