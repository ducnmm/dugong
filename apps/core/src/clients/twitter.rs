#![allow(dead_code)]

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use tracing::{info, warn};

use crate::config::Config;

/// Max candidates fetched from TwitterAPI.io advanced search per campaign resolution.
const MAX_CAMPAIGN_SEARCH_RESULTS: usize = 50;

/// Attempts for campaign candidate search. TwitterAPI.io's advanced_search is flaky /
/// eventually-consistent — a single call may omit freshly-posted replies, return only
/// a partial set (e.g. just the campaign tweet + the creator's own reply), or return an
/// empty body — and it can take a few minutes to FULLY converge (with sustained windows
/// where different calls disagree). So we retry (with capped backoff) over a multi-minute
/// window until an ELIGIBLE candidate appears before concluding the crowd is empty.
const CAMPAIGN_SEARCH_ATTEMPTS: u64 = 12;

/// Per-attempt backoff (seconds) for campaign candidate search: ramps up then caps, so
/// the total retry window spans ~3 minutes (covers the observed search convergence lag).
fn campaign_search_backoff_secs(attempt: u64) -> u64 {
    (3 * attempt).min(25)
}

fn configured_docs_url() -> String {
    env::var("DOCS_URL")
        .or_else(|_| env::var("NEXT_PUBLIC_DOCS_URL"))
        .unwrap_or_else(|_| "http://127.0.0.1:3004".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn configured_web_url() -> String {
    for key in [
        "WEB_URL",
        "APP_URL",
        "FRONTEND_URL",
        "NEXT_PUBLIC_DUGONG_APP_URL",
        "VITE_APP_URL",
    ] {
        if let Ok(url) = env::var(key) {
            let url = url.trim().trim_end_matches('/');
            if !url.is_empty() {
                return url.to_string();
            }
        }
    }

    if let Ok(domain) = env::var("RAILWAY_SERVICE_WEB_URL") {
        let domain = domain.trim().trim_end_matches('/');
        if domain.starts_with("http://") || domain.starts_with("https://") {
            return domain.to_string();
        }
        return format!("https://{domain}");
    }

    "http://127.0.0.1:43173".to_string()
}

/// A candidate winner discovered for a reward campaign (a reply author or hashtag tweeter).
#[derive(Debug, Clone)]
pub struct RewardCampaignCandidate {
    pub tweet_id: String,
    pub author_xid: String,
    pub author_handle: String,
    pub created_at: DateTime<Utc>,
}

/// Default production base URL for Twitter's official API (api.twitter.com).
pub const TWITTER_API_BASE_URL: &str = "https://api.twitter.com";
/// Default production base URL for the TwitterAPI.io third-party service.
pub const TWITTERAPI_IO_BASE_URL: &str = "https://api.twitterapi.io";

// ====== OAuth 2.0 Types ======

/// OAuth 2.0 token response from Twitter
#[derive(Debug, Deserialize)]
pub struct OAuth2TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

/// Outcome of a refresh-token exchange, distinguishing failures the caller must
/// react to differently: a definitively-dead refresh token (the user must
/// re-authenticate) vs. a transient error (retrying later may succeed).
#[derive(Debug)]
pub enum RefreshError {
    /// The refresh token is invalid/revoked/expired — re-authentication required.
    ReauthRequired(String),
    /// Transport or server-side error; the stored token may still be valid.
    Transient(anyhow::Error),
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefreshError::ReauthRequired(msg) => write!(f, "re-authentication required: {msg}"),
            RefreshError::Transient(err) => write!(f, "transient refresh error: {err}"),
        }
    }
}

impl std::error::Error for RefreshError {}

/// Twitter user info from /2/users/me endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwitterUserInfo {
    pub id: String,
    pub name: String,
    pub username: String,
}

/// Response wrapper for /2/users/me
#[derive(Debug, Deserialize)]
struct UsersMeResponse {
    data: TwitterUserInfo,
}

/// OAuth 2.0 client for user authentication
pub struct TwitterOAuth2Client {
    http_client: Client,
    client_id: String,
    client_secret: String,
    api_base: String,
}

impl TwitterOAuth2Client {
    pub fn new(config: &Config) -> Self {
        Self::with_base_url(config, TWITTER_API_BASE_URL.to_string())
    }

    /// Construct a client pointed at a custom Twitter API base URL (used in tests).
    pub fn with_base_url(config: &Config, api_base: String) -> Self {
        Self {
            http_client: Client::new(),
            client_id: config.twitter_oauth2_client_id.clone(),
            client_secret: config.twitter_oauth2_client_secret.clone(),
            api_base,
        }
    }

    /// Exchange authorization code for access token (OAuth 2.0 with PKCE)
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<OAuth2TokenResponse> {
        let url = format!("{}/2/oauth2/token", self.api_base);

        // Build form data
        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ];

        // Create Basic auth header (client_id:client_secret)
        let credentials = format!("{}:{}", self.client_id, self.client_secret);
        let auth_header = format!("Basic {}", BASE64.encode(credentials.as_bytes()));

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", &auth_header)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&params)
            .send()
            .await
            .context("Failed to send token exchange request")?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .context("Failed to read token response body")?;

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Twitter OAuth2 token exchange failed ({}): {}",
                status,
                response_text
            ));
        }

        let token_response: OAuth2TokenResponse =
            serde_json::from_str(&response_text).context("Failed to parse token response")?;

        info!("Successfully exchanged code for access token");
        Ok(token_response)
    }

    /// Exchange a stored refresh token for a fresh access token
    /// (`grant_type=refresh_token`, Basic client credentials).
    ///
    /// Twitter **rotates** refresh tokens: the returned [`OAuth2TokenResponse`]
    /// usually carries a new `refresh_token` that the caller MUST persist in place
    /// of the one passed in. Errors are classified so the caller can tell apart a
    /// dead token (re-login) from a transient failure (safe to retry).
    pub async fn refresh_access_token(
        &self,
        refresh_token: &str,
    ) -> Result<OAuth2TokenResponse, RefreshError> {
        let url = format!("{}/2/oauth2/token", self.api_base);

        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];

        let credentials = format!("{}:{}", self.client_id, self.client_secret);
        let auth_header = format!("Basic {}", BASE64.encode(credentials.as_bytes()));

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", &auth_header)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&params)
            .send()
            .await
            .map_err(|e| {
                RefreshError::Transient(
                    anyhow::Error::new(e).context("refresh token request failed"),
                )
            })?;

        let status = response.status();
        let response_text = response.text().await.map_err(|e| {
            RefreshError::Transient(
                anyhow::Error::new(e).context("failed to read refresh response"),
            )
        })?;

        if !status.is_success() {
            // 401/invalid_grant/invalid_request/unauthorized_client → the refresh
            // token is dead; the user must re-authenticate. 429/5xx → transient.
            let body_l = response_text.to_lowercase();
            let is_reauth = status == reqwest::StatusCode::UNAUTHORIZED
                || body_l.contains("invalid_grant")
                || body_l.contains("invalid_request")
                || body_l.contains("unauthorized_client")
                || (status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS);
            // Do NOT log the response body — it can echo token material.
            let msg = format!("Twitter refresh failed ({status})");
            return Err(if is_reauth {
                RefreshError::ReauthRequired(msg)
            } else {
                RefreshError::Transient(anyhow::anyhow!(msg))
            });
        }

        let token_response: OAuth2TokenResponse =
            serde_json::from_str(&response_text).map_err(|e| {
                RefreshError::Transient(
                    anyhow::Error::new(e).context("failed to parse refresh response"),
                )
            })?;

        info!("Successfully refreshed Twitter access token");
        Ok(token_response)
    }

    /// Get authenticated user info using access token
    pub async fn get_user_info(&self, access_token: &str) -> Result<TwitterUserInfo> {
        let url = format!("{}/2/users/me", self.api_base);

        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await
            .context("Failed to send user info request")?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .context("Failed to read user info response body")?;

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Twitter API get user info failed ({}): {}",
                status,
                response_text
            ));
        }

        let user_response: UsersMeResponse =
            serde_json::from_str(&response_text).context("Failed to parse user info response")?;

        info!(
            user_id = %user_response.data.id,
            username = %user_response.data.username,
            "Retrieved authenticated user info"
        );

        Ok(user_response.data)
    }
}

/// TwitterAPI.io client for posting replies and public user lookup.
pub struct TwitterClient {
    http_client: Client,
    twitterapi_io_api_key: String,
    twitterapi_io_login_cookies: Option<String>,
    twitterapi_io_proxy: Option<String>,
    twitterapi_io_base: String,
    docs_url: String,
    web_url: String,
}

/// Request body for creating a tweet through TwitterAPI.io.
#[derive(Debug, Serialize)]
struct CreateTweetRequest {
    login_cookies: String,
    tweet_text: String,
    proxy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_tweet_id: Option<String>,
}

/// Response from creating a tweet
#[derive(Debug, Deserialize)]
struct CreateTweetResponse {
    status: String,
    // TwitterAPI.io is inconsistent: some endpoints return the human-readable
    // error under `msg`, others (e.g. user_login_v2) under `message`. Accept
    // either so failures are never logged as an opaque "unknown error".
    #[serde(alias = "message")]
    msg: Option<String>,
    tweet_id: Option<String>,
}

/// Response from getting user by username
#[derive(Debug, Deserialize)]
struct GetUserResponse {
    status: String,
    msg: Option<String>,
    data: TwitterUser,
}

/// Twitter user info
#[derive(Debug, Deserialize)]
pub struct TwitterUser {
    pub id: String,
    #[serde(rename = "userName")]
    pub username: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub name: String,
}

/// Transaction result for building reply message
pub struct TransactionResult {
    pub tx_digest: String,
    pub from_handle: String,
    pub to_handle: String,
    pub amount: u64,
    pub coin_type: String,
    pub original_tweet_id: String,
}

impl TwitterClient {
    pub fn new(config: &Config) -> Self {
        Self::with_base_url(config, TWITTERAPI_IO_BASE_URL.to_string())
    }

    /// Construct a client pointed at a custom TwitterAPI.io base URL (used in tests).
    pub fn with_base_url(config: &Config, twitterapi_io_base: String) -> Self {
        Self {
            http_client: Client::new(),
            twitterapi_io_api_key: config.twitterapi_io_api_key.clone(),
            twitterapi_io_login_cookies: config.twitterapi_io_login_cookies.clone(),
            twitterapi_io_proxy: config.twitterapi_io_proxy.clone(),
            twitterapi_io_base,
            docs_url: configured_docs_url(),
            web_url: configured_web_url(),
        }
    }

    fn tx_url(&self, tx_digest: &str) -> String {
        format!("{}/tx/{}", self.web_url, tx_digest)
    }

    fn tweet_char_weight(ch: char) -> usize {
        if ch.is_ascii() {
            1
        } else {
            2
        }
    }

    fn truncate_for_tweet(text: &str, max_weight: usize) -> String {
        let text = text.trim();
        let suffix = "...";
        let suffix_weight = suffix.len();
        let limit = max_weight.saturating_sub(suffix_weight);
        let mut output = String::new();
        let mut weight = 0usize;
        let mut truncated = false;

        for ch in text.chars() {
            let char_weight = Self::tweet_char_weight(ch);
            if weight + char_weight > limit {
                truncated = true;
                break;
            }

            output.push(ch);
            weight += char_weight;
        }

        if truncated {
            output.push_str(suffix);
        }

        output
    }

    /// Reply to a tweet with transaction success message
    pub async fn reply_transfer_success(&self, result: &TransactionResult) -> Result<String> {
        // Get coin decimals and format amount for display
        let (decimals, coin_symbol) = if result.coin_type.to_uppercase() == "SUI"
            || result.coin_type.contains("sui::SUI")
        {
            (9, "SUI")
        } else if result.coin_type.to_uppercase() == "USDC"
            || result.coin_type.contains("usdc::USDC")
        {
            (6, "USDC")
        } else if result.coin_type.to_uppercase() == "WAL" || result.coin_type.contains("wal::WAL")
        {
            (9, "WAL")
        } else {
            // For unknown coins, try to extract symbol from type path
            let symbol = result
                .coin_type
                .split("::")
                .last()
                .unwrap_or(&result.coin_type);
            (9, symbol) // Default to 9 decimals
        };

        let divisor = 10_u64.pow(decimals);
        let amount_float = result.amount as f64 / divisor as f64;
        // Round to 2 decimals for display, then trim trailing zeros (e.g. 0.01, 1, 5).
        let amount_str = format!("{:.2}", amount_float)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
        let display_amount = format!("{} {}", amount_str, coin_symbol);

        // Build success message
        let message = format!(
            "Transaction successful!\n\n\
            Sent {} from @{} to @{}\n\n\
            View on tx:\n\
            {}",
            display_amount, result.from_handle, result.to_handle, self.tx_url(&result.tx_digest)
        );

        info!(
            tweet_id = %result.original_tweet_id,
            tx_digest = %result.tx_digest,
            "Replying to tweet with transaction success"
        );

        self.reply_to_tweet(&result.original_tweet_id, &message)
            .await
    }

    /// Reply to a tweet with account creation success message
    pub async fn reply_account_created(
        &self,
        tweet_id: &str,
        handle: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let message = format!(
            "Welcome to Dugong, @{}!\n\n\
            Your account has been created successfully.\n\n\
            You can now receive and send crypto via tweets!\n\n\
            View on tx:\n\
            {}",
            handle, self.tx_url(tx_digest)
        );

        info!(
            tweet_id = %tweet_id,
            handle = %handle,
            tx_digest = %tx_digest,
            "Replying to tweet with account creation success"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply to a tweet when the account already exists.
    pub async fn reply_account_already_exists(
        &self,
        tweet_id: &str,
        handle: &str,
        account_id: Option<&str>,
    ) -> Result<String> {
        // Twitter rejects verbatim-duplicate tweet text with HTTP 422, so the
        // reply must vary per account. Include the @handle and the unique
        // account object id (also the useful info to surface to the user).
        let message = match account_id {
            Some(account_id) => format!("@{} Account Already Exist\n\n{}", handle, account_id),
            None => format!("@{} Account Already Exist", handle),
        };

        info!(
            tweet_id = %tweet_id,
            handle = %handle,
            account_id = ?account_id,
            "Replying to tweet with account already exists message"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply to a tweet with wallet linking success message
    pub async fn reply_wallet_linked(
        &self,
        tweet_id: &str,
        handle: &str,
        wallet_address: &str,
        tx_digest: &str,
    ) -> Result<String> {
        // Truncate wallet address for display
        let short_address = if wallet_address.len() > 12 {
            format!(
                "{}...{}",
                &wallet_address[..8],
                &wallet_address[wallet_address.len() - 6..]
            )
        } else {
            wallet_address.to_string()
        };

        let message = format!(
            "Wallet linked successfully, @{}!\n\n\
            Your Dugong is now connected to:\n\
            {}\n\n\
            You can now deposit/withdraw directly from your wallet!\n\n\
            View on tx:\n\
            {}",
            handle, short_address, self.tx_url(tx_digest)
        );

        info!(
            tweet_id = %tweet_id,
            handle = %handle,
            wallet = %wallet_address,
            tx_digest = %tx_digest,
            "Replying to tweet with wallet linking success"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Get Twitter user by username (handle)
    pub async fn get_user_by_username(&self, username: &str) -> Result<TwitterUser> {
        // Remove @ prefix if present
        let clean_username = username.trim_start_matches('@');
        let url = format!("{}/twitter/user/info", self.twitterapi_io_base);

        let response = self
            .http_client
            .get(&url)
            .header("X-API-Key", &self.twitterapi_io_api_key)
            .query(&[("userName", clean_username)])
            .send()
            .await
            .context("Failed to send get user request")?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .context("Failed to read response body")?;

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "TwitterAPI.io error ({}): {}",
                status,
                response_text
            ));
        }

        let user_response: GetUserResponse =
            serde_json::from_str(&response_text).context("Failed to parse user response")?;

        if !user_response.status.eq_ignore_ascii_case("success") {
            return Err(anyhow::anyhow!(
                "TwitterAPI.io user lookup failed for @{}: {}",
                clean_username,
                user_response
                    .msg
                    .unwrap_or_else(|| "unknown error".to_string())
            ));
        }

        info!(
            user_id = %user_response.data.id,
            username = %user_response.data.username,
            "Retrieved TwitterAPI.io user by username"
        );

        Ok(user_response.data)
    }

    /// Reply confirming a market was created with betting instructions
    pub async fn reply_market_created(
        &self,
        tweet_id: &str,
        question: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let question = Self::truncate_for_tweet(question, 120);
        let message = format!(
            "Market created.\n\n\
            {}\n\n\
            Predict:\n\
            @DugongWallet predict <amount> <coin> on yes/no\n\n\
            Tx:\n\
            {}",
            question, self.tx_url(tx_digest)
        );

        info!(tweet_id = %tweet_id, "Replying with market created message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply confirming a bet was placed
    pub async fn reply_bet_placed(
        &self,
        tweet_id: &str,
        handle: &str,
        amount_display: &str,
        side: bool,
        tx_digest: &str,
    ) -> Result<String> {
        let side_str = if side { "YES" } else { "NO" };
        let message = format!(
            "Prediction placed, @{}!\n\n\
            {} on {}\n\n\
            Your stake is escrowed — payouts are distributed when the creator resolves the market.\n\n\
            View on tx:\n\
            {}",
            handle, amount_display, side_str, self.tx_url(tx_digest)
        );

        info!(tweet_id = %tweet_id, handle = %handle, "Replying with bet placed message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply with market resolution payout summary
    pub async fn reply_market_resolved(
        &self,
        tweet_id: &str,
        outcome: bool,
        winner_count: usize,
        tx_digest: &str,
    ) -> Result<String> {
        let outcome_str = if outcome { "YES" } else { "NO" };
        let message = format!(
            "Market resolved: {}\n\n\
            {} winner(s) can now claim. Reply to the market tweet with @DugongWallet claim to collect your payout.\n\n\
            View on tx:\n\
            {}",
            outcome_str, winner_count, self.tx_url(tx_digest)
        );

        info!(tweet_id = %tweet_id, outcome = %outcome_str, "Replying with market resolved message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when market is already closed / already resolved
    pub async fn reply_market_closed(&self, tweet_id: &str, handle: &str) -> Result<String> {
        let message = format!(
            "@{} — this market is already closed.\n\n\
            Predictions are only accepted while the market is open.",
            handle
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when resolver is not the market creator
    pub async fn reply_unauthorized_resolve(&self, tweet_id: &str, handle: &str) -> Result<String> {
        let message = format!(
            "@{} — only the market creator can resolve this market.",
            handle
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when market tweet cannot be found in the registry
    pub async fn reply_market_not_found(&self, tweet_id: &str, handle: &str) -> Result<String> {
        let message = format!(
            "@{} — no prediction market found for this tweet.\n\n\
            Make sure you are replying directly to the market creation tweet.",
            handle
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when a market has no bets to resolve.
    pub async fn reply_market_has_no_bets(&self, tweet_id: &str, handle: &str) -> Result<String> {
        let message = format!(
            "@{} — this market has no predictions yet.\n\n\
            Resolve it after at least one valid prediction has been placed.",
            handle
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when a market payout claim is attempted before resolution.
    pub async fn reply_market_not_resolved_yet(
        &self,
        tweet_id: &str,
        handle: &str,
    ) -> Result<String> {
        let message = format!(
            "@{} — this market is not resolved yet.\n\n\
            Payouts can only be claimed after the creator resolves the market.",
            handle
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply confirming a reward campaign was created
    pub async fn reply_campaign_created(
        &self,
        tweet_id: &str,
        reward_display: &str,
        max_winners: u64,
        tx_digest: &str,
    ) -> Result<String> {
        let message = format!(
            "Reward campaign created!\n\n\
            {} each for up to {} winner(s).\n\n\
            Winners will be chosen by the creator. When ready, the creator resolves with:\n\
            @DugongWallet solve!\n\n\
            Winners then claim with:\n\
            @DugongWallet claim\n\n\
            View on tx:\n\
            {}",
            reward_display, max_winners, self.tx_url(tx_digest)
        );
        info!(tweet_id = %tweet_id, "Replying with campaign created message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply with reward campaign resolution summary
    pub async fn reply_campaign_resolved(
        &self,
        tweet_id: &str,
        winner_count: u64,
        tx_digest: &str,
    ) -> Result<String> {
        let message = format!(
            "Campaign resolved!\n\n\
            {} winner(s) selected. Reply to the campaign tweet with @DugongWallet claim to collect your reward.\n\n\
            View on tx:\n\
            {}",
            winner_count, self.tx_url(tx_digest)
        );
        info!(tweet_id = %tweet_id, "Replying with campaign resolved message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply confirming a reward was claimed
    pub async fn reply_reward_claimed(
        &self,
        tweet_id: &str,
        handle: &str,
        reward_display: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let message = format!(
            "Reward claimed, @{}!\n\n\
            {} has been credited to your @DugongWallet account.\n\n\
            View on tx:\n\
            {}",
            handle, reward_display, self.tx_url(tx_digest)
        );
        info!(tweet_id = %tweet_id, handle = %handle, "Replying with reward claimed message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply confirming a prediction-market payout was claimed
    pub async fn reply_market_payout_claimed(
        &self,
        tweet_id: &str,
        handle: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let message = format!(
            "Market payout claimed, @{}!\n\n\
            Your winnings have been credited to your @DugongWallet account.\n\n\
            View on tx:\n\
            {}",
            handle, self.tx_url(tx_digest)
        );
        info!(tweet_id = %tweet_id, handle = %handle, "Replying with market payout claimed message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when a campaign already exists for this tweet
    pub async fn reply_campaign_already_exists(&self, tweet_id: &str) -> Result<String> {
        let message = "A reward campaign already exists for this tweet.".to_string();
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when a reward campaign cannot be found for a resolve command.
    pub async fn reply_campaign_not_found(&self, tweet_id: &str, handle: &str) -> Result<String> {
        let message = format!(
            "@{} — no reward campaign found for this tweet.\n\n\
            Make sure you are replying directly to the reward campaign tweet.",
            handle
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when a reward campaign is already resolved.
    pub async fn reply_campaign_already_resolved(
        &self,
        tweet_id: &str,
        handle: &str,
    ) -> Result<String> {
        let message = format!(
            "@{} — this reward campaign is already resolved.\n\n\
            Winners can reply with @DugongWallet claim if they have not claimed yet.",
            handle
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when resolver is not the campaign creator
    pub async fn reply_unauthorized_campaign_resolve(
        &self,
        tweet_id: &str,
        handle: &str,
    ) -> Result<String> {
        let message = format!(
            "@{} — only the campaign creator can resolve this campaign.",
            handle
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when a claimant has no entitlement / nothing to claim
    pub async fn reply_nothing_to_claim(&self, tweet_id: &str, handle: &str) -> Result<String> {
        let message = format!(
            "@{} — nothing to claim here.\n\n\
            You can only claim a reward or payout you are entitled to, after the creator resolves.",
            handle
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when a reward claim is attempted before campaign resolution.
    pub async fn reply_campaign_not_resolved_yet(
        &self,
        tweet_id: &str,
        handle: &str,
    ) -> Result<String> {
        let message = format!(
            "@{} — this reward campaign is not resolved yet.\n\n\
            Winners can claim only after the creator resolves the campaign.",
            handle
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when a user tries to claim the same reward twice.
    pub async fn reply_already_claimed(&self, tweet_id: &str, handle: &str) -> Result<String> {
        let message = format!("@{} — this reward has already been claimed.", handle);
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Reply when a tweet mentions the bot but does not match a supported command.
    pub async fn reply_unsupported_command(&self, tweet_id: &str) -> Result<String> {
        let message = format!(
            "I didn't recognize that command.\n\n\
            Try one of Dugong's supported commands:\n\
            1) @DugongWallet create account\n\
            2) @DugongWallet send <amount> <coin> to @<user>\n\
            3) @DugongWallet create market: <question>\n\
            4) @DugongWallet predict <amount> <coin> on yes/no\n\
            5) @DugongWallet resolve | solve yes/no\n\n\
            You can check all docs here:\n\
            {}/tweet-commands",
            self.docs_url
        );
        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Fetch top reply authors to a campaign tweet (campaign_type = top replies).
    ///
    /// TwitterAPI.io's advanced_search is eventually-consistent and flaky: a given
    /// call may return the conversation root but omit freshly-posted replies, or
    /// return an empty body entirely — and the result set varies call to call. A
    /// single bad response at resolve time would silently select zero winners and
    /// refund a campaign that actually had replies. So retry until at least one
    /// genuine reply appears (a tweet other than the campaign tweet itself), backing
    /// off between attempts. The emptiness check is POST-filter: `[campaign tweet]`
    /// alone is not an adequate result and triggers a retry. If every successful
    /// response is reply-free we treat the crowd as genuinely empty; a persistent
    /// transport error is fatal so resolve fails and the campaign stays open to retry.
    pub async fn fetch_top_reply_candidates(
        &self,
        campaign_tweet_id: &str,
        creator_xid: &str,
        max_winners: usize,
    ) -> Result<Vec<RewardCampaignCandidate>> {
        let query = format!("conversation_id:{}", campaign_tweet_id);
        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 1..=CAMPAIGN_SEARCH_ATTEMPTS {
            // Over-fetch the raw page (up to MAX_CAMPAIGN_SEARCH_RESULTS), NOT
            // max_winners: the campaign tweet and the creator's own replies rank
            // highest under "Top" and must be filtered out below, so truncating the
            // raw search to max_winners (e.g. 1) would discard the eligible repliers
            // before we ever see them. dedupe_candidates applies the max_winners cap
            // after filtering.
            match self
                .search_campaign_candidates_once(&query, "Top", MAX_CAMPAIGN_SEARCH_RESULTS)
                .await
            {
                Ok(mut candidates) => {
                    // Drop the campaign tweet itself AND the creator's own tweets (the
                    // creator is never an eligible winner). The retry adequacy check is
                    // on ELIGIBLE candidates: a flaky partial response carrying only the
                    // campaign tweet + the creator's confirmation reply is NOT adequate
                    // and must be retried — otherwise it would resolve with 0 winners
                    // even though the crowd replied.
                    candidates.retain(|candidate| {
                        candidate.tweet_id != campaign_tweet_id
                            && candidate.author_xid != creator_xid
                    });
                    if !candidates.is_empty() {
                        return dedupe_candidates(candidates, max_winners);
                    }
                    warn!(
                        attempt,
                        campaign_tweet_id,
                        "advanced_search returned no eligible reply candidates yet; retrying"
                    );
                }
                Err(e) => {
                    warn!(attempt, campaign_tweet_id, error = %e, "advanced_search failed; retrying");
                    last_err = Some(e);
                }
            }
            if attempt < CAMPAIGN_SEARCH_ATTEMPTS {
                tokio::time::sleep(std::time::Duration::from_secs(
                    campaign_search_backoff_secs(attempt),
                ))
                .await;
            }
        }

        match last_err {
            Some(e) => Err(e)
                .with_context(|| format!("Failed to search replies for {}", campaign_tweet_id)),
            None => Ok(Vec::new()),
        }
    }

    /// Fetch the first users who tweeted a hashtag (campaign_type = first hashtag).
    /// Retries on the same flaky/empty advanced_search behavior as the reply path.
    pub async fn fetch_first_hashtag_candidates(
        &self,
        hashtag: &str,
        creator_xid: &str,
        max_winners: usize,
    ) -> Result<Vec<RewardCampaignCandidate>> {
        let query = hashtag.trim().to_string();
        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 1..=CAMPAIGN_SEARCH_ATTEMPTS {
            // Over-fetch (see fetch_top_reply_candidates): pull the full result set so
            // the creator's own hashtag tweets can be filtered out, then sort by
            // created_at and cap to max_winners in dedupe_candidates — truncating the
            // raw search to max_winners here would also break "first K" ordering.
            match self
                .search_campaign_candidates_once(&query, "Latest", MAX_CAMPAIGN_SEARCH_RESULTS)
                .await
            {
                Ok(mut candidates) => {
                    // Exclude the creator's own hashtag tweets — they can never win, so a
                    // creator-only response is not an adequate result and must be retried.
                    candidates.retain(|candidate| candidate.author_xid != creator_xid);
                    if !candidates.is_empty() {
                        candidates.sort_by_key(|candidate| candidate.created_at);
                        return dedupe_candidates(candidates, max_winners);
                    }
                    warn!(attempt, hashtag = %query, "advanced_search returned no eligible hashtag candidates yet; retrying");
                }
                Err(e) => {
                    warn!(attempt, hashtag = %query, error = %e, "advanced_search failed; retrying");
                    last_err = Some(e);
                }
            }
            if attempt < CAMPAIGN_SEARCH_ATTEMPTS {
                tokio::time::sleep(std::time::Duration::from_secs(
                    campaign_search_backoff_secs(attempt),
                ))
                .await;
            }
        }

        match last_err {
            Some(e) => {
                Err(e).with_context(|| format!("Failed to search hashtag candidates for {}", query))
            }
            None => Ok(Vec::new()),
        }
    }

    async fn search_campaign_candidates_once(
        &self,
        query: &str,
        query_type: &str,
        max_results: usize,
    ) -> Result<Vec<RewardCampaignCandidate>> {
        let max_results = max_results.clamp(1, MAX_CAMPAIGN_SEARCH_RESULTS);
        let url = format!("{}/twitter/tweet/advanced_search", self.twitterapi_io_base);
        let mut cursor: Option<String> = None;
        let mut candidates = Vec::new();

        while candidates.len() < max_results {
            let mut params = vec![("query", query), ("queryType", query_type)];
            if let Some(cursor) = cursor.as_deref() {
                params.push(("cursor", cursor));
            }

            let response = self
                .http_client
                .get(&url)
                .header("X-API-Key", &self.twitterapi_io_api_key)
                .query(&params)
                .send()
                .await
                .context("Failed to call TwitterAPI.io advanced search")?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("TwitterAPI.io advanced search error {}: {}", status, text);
            }

            let response = response
                .json::<TwitterApiSearchResponse>()
                .await
                .context("Failed to parse TwitterAPI.io advanced search response")?;
            let has_next_page = response.has_next_page;
            let next_cursor = response.next_cursor.filter(|cursor| !cursor.is_empty());

            candidates.extend(response.tweets.into_iter().filter_map(|tweet| {
                DateTime::parse_from_str(&tweet.created_at, "%a %b %d %H:%M:%S %z %Y")
                    .ok()
                    .map(|created_at| RewardCampaignCandidate {
                        tweet_id: tweet.id,
                        author_xid: tweet.author.id,
                        author_handle: tweet.author.username,
                        created_at: created_at.with_timezone(&Utc),
                    })
            }));

            if !has_next_page || candidates.len() >= max_results {
                break;
            }
            if next_cursor.as_ref() == cursor.as_ref() {
                break;
            }
            match next_cursor {
                Some(next_cursor) => cursor = Some(next_cursor),
                None => break,
            }
        }

        candidates.truncate(max_results);
        Ok(candidates)
    }

    /// Build a friendly, user-facing error message for transaction failures.
    /// We keep raw error details only in server logs to avoid leaking technical text to users.
    fn friendly_error_message(error_message: &str) -> &'static str {
        let lower = error_message.to_lowercase();

        if lower.contains("insufficient")
            || lower.contains("balance") && lower.contains("insufficient")
        {
            "It looks like the sender may not have enough balance for this action."
        } else if lower.contains("already") && lower.contains("exists") {
            "This action appears to have already been completed."
        } else if lower.contains("already claimed") || lower.contains("already paid") {
            "This reward or payout has already been claimed."
        } else if lower.contains("permission")
            || lower.contains("unauthorized")
            || lower.contains("not authorized")
        {
            "You don't have permission for this action right now."
        } else if lower.contains("not found") || lower.contains("missing") {
            "Some required item couldn't be found. Please check context and try again."
        } else if lower.contains("timeout") || lower.contains("timed out") {
            "The network was busy while processing your request."
        } else if lower.contains("rate limit") || lower.contains("429") {
            "The service is rate-limited. Please try again in a minute."
        } else {
            "Something went wrong while processing your request."
        }
    }

    /// Reply to a tweet with error message
    #[allow(dead_code)]
    pub async fn reply_error(&self, tweet_id: &str, error_message: &str) -> Result<String> {
        let user_friendly = Self::friendly_error_message(error_message);
        let message = format!(
            "Sorry, I couldn't finish this request.\n\n\
            {}\n\n\
            Tip: Double-check the command format and try again in a minute.\n\
            If this keeps happening, check the guide:\n\
            {}/tweet-commands",
            user_friendly, self.docs_url
        );

        info!(
            tweet_id = %tweet_id,
            error = %error_message,
            "Replying to tweet with error"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    /// Post a reply to a specific tweet
    async fn reply_to_tweet(&self, tweet_id: &str, text: &str) -> Result<String> {
        let url = format!("{}/twitter/create_tweet_v2", self.twitterapi_io_base);
        let login_cookies = self.twitterapi_io_login_cookies.as_ref().ok_or_else(|| {
            warn!(
                tweet_id = %tweet_id,
                reply_text = %text,
                "Reply not posted because TWITTERAPI_IO_LOGIN_COOKIES is missing"
            );
            anyhow::anyhow!("TWITTERAPI_IO_LOGIN_COOKIES must be set to post replies")
        })?;
        let proxy = self.twitterapi_io_proxy.as_ref().ok_or_else(|| {
            warn!(
                tweet_id = %tweet_id,
                reply_text = %text,
                "Reply not posted because TWITTERAPI_IO_PROXY is missing"
            );
            anyhow::anyhow!("TWITTERAPI_IO_PROXY must be set to post replies")
        })?;

        let request_body = CreateTweetRequest {
            login_cookies: login_cookies.clone(),
            tweet_text: text.to_string(),
            proxy: proxy.clone(),
            reply_to_tweet_id: Some(tweet_id.to_string()),
        };

        let body_json =
            serde_json::to_string(&request_body).context("Failed to serialize tweet request")?;

        let response = self
            .http_client
            .post(&url)
            .header("X-API-Key", &self.twitterapi_io_api_key)
            .header("Content-Type", "application/json")
            .body(body_json)
            .send()
            .await
            .context("Failed to send tweet request")?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .context("Failed to read response body")?;

        if !status.is_success() {
            warn!(
                tweet_id = %tweet_id,
                reply_text = %text,
                status = %status,
                response = %response_text,
                "Reply not posted because TwitterAPI.io returned an HTTP error"
            );
            return Err(anyhow::anyhow!(
                "TwitterAPI.io create tweet error ({}): {}",
                status,
                response_text
            ));
        }

        let tweet_response: CreateTweetResponse =
            serde_json::from_str(&response_text).context("Failed to parse tweet response")?;
        if !tweet_response.status.eq_ignore_ascii_case("success") {
            warn!(
                tweet_id = %tweet_id,
                reply_text = %text,
                api_status = %tweet_response.status,
                api_message = ?tweet_response.msg,
                raw_response = %response_text,
                "Reply not posted because TwitterAPI.io returned an API error"
            );
            return Err(anyhow::anyhow!(
                "TwitterAPI.io create tweet failed: {}",
                tweet_response
                    .msg
                    .unwrap_or_else(|| format!("unknown error (raw: {response_text})"))
            ));
        }

        let reply_tweet_id = tweet_response.tweet_id.ok_or_else(|| {
            anyhow::anyhow!("TwitterAPI.io create tweet response missing tweet_id")
        })?;

        info!(
            reply_tweet_id = %reply_tweet_id,
            "Successfully posted reply tweet"
        );

        Ok(reply_tweet_id)
    }
}

// ====== Reward Campaign Candidate Search Types ======

#[derive(Debug, Deserialize)]
struct TwitterApiSearchResponse {
    #[serde(default)]
    tweets: Vec<TwitterApiTweet>,
    #[serde(default, rename = "has_next_page", alias = "hasNextPage")]
    has_next_page: bool,
    #[serde(default, rename = "next_cursor", alias = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TwitterApiTweet {
    id: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    author: TwitterApiAuthor,
}

#[derive(Debug, Deserialize)]
struct TwitterApiAuthor {
    id: String,
    #[serde(rename = "userName")]
    username: String,
}

/// Keep at most one candidate per author (first occurrence wins), up to `max_winners`.
fn dedupe_candidates(
    candidates: Vec<RewardCampaignCandidate>,
    max_winners: usize,
) -> Result<Vec<RewardCampaignCandidate>> {
    let mut seen = Vec::<String>::new();
    let mut deduped = Vec::new();

    for candidate in candidates {
        if seen.contains(&candidate.author_xid) {
            continue;
        }
        seen.push(candidate.author_xid.clone());
        deduped.push(candidate);
        if deduped.len() >= max_winners {
            break;
        }
    }

    Ok(deduped)
}
