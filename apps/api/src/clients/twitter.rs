#![allow(dead_code)]

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::config::Config;

const MAX_CAMPAIGN_SEARCH_RESULTS: usize = 50;

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
}

impl TwitterOAuth2Client {
    pub fn new(config: &Config) -> Self {
        Self {
            http_client: Client::new(),
            client_id: config.twitter_oauth2_client_id.clone(),
            client_secret: config.twitter_oauth2_client_secret.clone(),
        }
    }

    /// Exchange authorization code for access token (OAuth 2.0 with PKCE)
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<OAuth2TokenResponse> {
        let url = "https://api.twitter.com/2/oauth2/token";

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
            .post(url)
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

    /// Get authenticated user info using access token
    pub async fn get_user_info(&self, access_token: &str) -> Result<TwitterUserInfo> {
        let url = "https://api.twitter.com/2/users/me";

        let response = self
            .http_client
            .get(url)
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
    replies_enabled: bool,
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
    #[serde(default, rename = "profilePicture")]
    pub profile_picture: Option<String>,
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

#[derive(Debug, Clone)]
pub struct RewardCampaignCandidate {
    pub tweet_id: String,
    pub author_xid: String,
    pub author_handle: String,
    pub created_at: DateTime<Utc>,
}

impl TwitterClient {
    pub fn new(config: &Config) -> Self {
        Self {
            http_client: Client::new(),
            twitterapi_io_api_key: config.twitterapi_io_api_key.clone(),
            twitterapi_io_login_cookies: config.twitterapi_io_login_cookies.clone(),
            twitterapi_io_proxy: config.twitterapi_io_proxy.clone(),
            replies_enabled: config.enable_twitter_replies,
        }
    }

    fn format_token_amount(amount: u64, coin_type: &str) -> String {
        let (decimals, coin_symbol) =
            if coin_type.to_uppercase() == "SUI" || coin_type.contains("sui::SUI") {
                (9, "SUI")
            } else if coin_type.to_uppercase() == "USDC" || coin_type.contains("usdc::USDC") {
                (6, "USDC")
            } else if coin_type.to_uppercase() == "WAL" || coin_type.contains("wal::WAL") {
                (9, "WAL")
            } else {
                let symbol = coin_type.split("::").last().unwrap_or(coin_type);
                (9, symbol)
            };

        let divisor = 10_u64.pow(decimals);
        let amount_float = amount as f64 / divisor as f64;
        format!(
            "{:.precision$} {}",
            amount_float,
            coin_symbol,
            precision = decimals as usize
        )
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
    }

    fn short_text(value: &str, max_chars: usize) -> String {
        if value.chars().count() <= max_chars {
            return value.to_string();
        }

        let mut truncated = value
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect::<String>();
        truncated.push_str("...");
        truncated
    }

    pub async fn fetch_top_reply_candidates(
        &self,
        campaign_tweet_id: &str,
        max_winners: usize,
    ) -> Result<Vec<RewardCampaignCandidate>> {
        let query = format!("conversation_id:{}", campaign_tweet_id);
        let mut candidates = self
            .search_campaign_candidates(&query, "Top", max_winners)
            .await
            .with_context(|| format!("Failed to search replies for {}", campaign_tweet_id))?;

        candidates.retain(|candidate| candidate.tweet_id != campaign_tweet_id);
        dedupe_candidates(candidates, max_winners)
    }

    pub async fn fetch_first_hashtag_candidates(
        &self,
        hashtag: &str,
        max_winners: usize,
    ) -> Result<Vec<RewardCampaignCandidate>> {
        let query = hashtag.trim().to_string();
        let mut candidates = self
            .search_campaign_candidates(&query, "Latest", max_winners)
            .await
            .with_context(|| format!("Failed to search hashtag candidates for {}", hashtag))?;

        candidates.sort_by_key(|candidate| candidate.created_at);
        dedupe_candidates(candidates, max_winners)
    }

    async fn search_campaign_candidates(
        &self,
        query: &str,
        query_type: &str,
        max_results: usize,
    ) -> Result<Vec<RewardCampaignCandidate>> {
        let max_results = max_results.clamp(1, MAX_CAMPAIGN_SEARCH_RESULTS);
        let mut cursor: Option<String> = None;
        let mut candidates = Vec::new();

        while candidates.len() < max_results {
            let mut params = vec![("query", query), ("queryType", query_type)];
            if let Some(cursor) = cursor.as_deref() {
                params.push(("cursor", cursor));
            }

            let response = self
                .http_client
                .get("https://api.twitterapi.io/twitter/tweet/advanced_search")
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
        let display_amount = format!(
            "{:.precision$} {}",
            amount_float,
            coin_symbol,
            precision = decimals as usize
        )
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string();

        // Build success message
        let message = format!(
            "Transaction successful!\n\n\
            Sent {} from @{} to @{}\n\n\
            View on Suiscan:\n\
            https://suiscan.xyz/testnet/tx/{}",
            display_amount, result.from_handle, result.to_handle, result.tx_digest
        );

        info!(
            tweet_id = %result.original_tweet_id,
            tx_digest = %result.tx_digest,
            "Replying to tweet with transaction success"
        );

        self.reply_to_tweet(&result.original_tweet_id, &message)
            .await
    }

    pub async fn reply_prediction_market_created(
        &self,
        tweet_id: &str,
        creator_handle: &str,
        question: &str,
    ) -> Result<String> {
        let question = Self::short_text(question, 140);
        let message = format!(
            "Prediction market opened by @{}.\n\n{}\n\nReply with:\n@DugongWallet bet 5 SUI with yes/no",
            creator_handle, question
        );

        info!(
            tweet_id = %tweet_id,
            creator = %creator_handle,
            "Replying with prediction market created message"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_prediction_market_already_exists(
        &self,
        tweet_id: &str,
        question: &str,
    ) -> Result<String> {
        let question = Self::short_text(question, 160);
        let message = format!("Prediction market already exists.\n\n{}", question);

        info!(
            tweet_id = %tweet_id,
            "Replying with prediction market already exists message"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_prediction_bet_placed(
        &self,
        tweet_id: &str,
        question: &str,
        bettor_handle: &str,
        choice: &str,
        amount: u64,
        coin_type: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let question = Self::short_text(question, 90);
        let display_amount = Self::format_token_amount(amount, coin_type);
        let message = format!(
            "Bet placed: @{} {} on {}\n\n{}\n\nhttps://suiscan.xyz/testnet/tx/{}",
            bettor_handle, display_amount, choice, question, tx_digest
        );

        info!(
            tweet_id = %tweet_id,
            bettor = %bettor_handle,
            choice = %choice,
            tx_digest = %tx_digest,
            "Replying with prediction bet placed message"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_prediction_bet_insufficient_balance(
        &self,
        tweet_id: &str,
        bettor_handle: &str,
        requested_amount: u64,
        available_amount: u64,
        coin_type: &str,
    ) -> Result<String> {
        let requested = Self::format_token_amount(requested_amount, coin_type);
        let available = Self::format_token_amount(available_amount, coin_type);
        let message = format!(
            "@{} your Dugong account is ready, but it only has {} available.\n\nBet requested: {}.\nDeposit funds in the dApp, then send a new bet reply.",
            bettor_handle, available, requested
        );

        info!(
            tweet_id = %tweet_id,
            bettor = %bettor_handle,
            requested = %requested,
            available = %available,
            "Replying with insufficient prediction bet balance message"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_prediction_market_resolved(
        &self,
        tweet_id: &str,
        question: &str,
        outcome: &str,
        winner_count: usize,
        total_pot: u64,
        coin_type: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let question = Self::short_text(question, 80);
        let display_pot = Self::format_token_amount(total_pot, coin_type);
        let message = format!(
            "Market resolved: {}\n\n{}\n\n{} winner(s) can claim from the {} pool by replying @DugongWallet claim to the market tweet.\n\nhttps://suiscan.xyz/testnet/tx/{}",
            outcome, question, winner_count, display_pot, tx_digest
        );

        info!(
            tweet_id = %tweet_id,
            outcome = %outcome,
            tx_digest = %tx_digest,
            "Replying with prediction market resolved message"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_prediction_market_resolved_no_bets(
        &self,
        tweet_id: &str,
        question: &str,
        outcome: &str,
    ) -> Result<String> {
        let question = Self::short_text(question, 140);
        let message = format!(
            "Market resolved: {}\n\n{}\n\nNo bets were placed.",
            outcome, question
        );

        info!(
            tweet_id = %tweet_id,
            outcome = %outcome,
            "Replying with no-bets prediction resolve message"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_prediction_market_resolved_no_winners(
        &self,
        tweet_id: &str,
        question: &str,
        outcome: &str,
        total_pot: u64,
        coin_type: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let question = Self::short_text(question, 100);
        let display_pot = Self::format_token_amount(total_pot, coin_type);
        let message = format!(
            "Market resolved: {}\n\n{}\n\nNo winning bets. Bettors can claim refunds from the {} pool by replying @DugongWallet claim to the market tweet.\n\nhttps://suiscan.xyz/testnet/tx/{}",
            outcome, question, display_pot, tx_digest
        );

        info!(
            tweet_id = %tweet_id,
            outcome = %outcome,
            "Replying with no-winners prediction resolve message"
        );

        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_prediction_payout_claimed(
        &self,
        tweet_id: &str,
        question: &str,
        outcome: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let question = Self::short_text(question, 100);
        let message = format!(
            "Prediction payout claimed.\n\nMarket: {}\nOutcome: {}\n\nhttps://suiscan.xyz/testnet/tx/{}",
            question, outcome, tx_digest
        );

        info!(tweet_id = %tweet_id, "Replying with prediction payout claimed message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_reward_campaign_created(
        &self,
        tweet_id: &str,
        campaign_type: &str,
        reward_amount: u64,
        coin_type: &str,
        max_winners: i64,
    ) -> Result<String> {
        let display_reward = Self::format_token_amount(reward_amount, coin_type);
        let message = format!(
            "Reward campaign created.\n\nType: {}\nReward: {} each\nMax winners: {}\n\nReply with @DugongWallet solve! to select winners.",
            campaign_type, display_reward, max_winners
        );

        info!(tweet_id = %tweet_id, "Replying with reward campaign created message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_reward_campaign_already_exists(
        &self,
        tweet_id: &str,
        campaign_type: &str,
    ) -> Result<String> {
        let message = format!(
            "Reward campaign already exists for this tweet.\n\nType: {}",
            campaign_type
        );

        info!(tweet_id = %tweet_id, "Replying with reward campaign exists message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_reward_campaign_resolved(
        &self,
        tweet_id: &str,
        winner_count: usize,
        total_budget: u64,
        coin_type: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let display_budget = Self::format_token_amount(total_budget, coin_type);
        let message = format!(
            "Reward campaign solved.\n\nSelected {} winner(s) from {} budget. Winners can claim by replying @DugongWallet claim to the campaign tweet.\n\nhttps://suiscan.xyz/testnet/tx/{}",
            winner_count, display_budget, tx_digest
        );

        info!(tweet_id = %tweet_id, "Replying with reward campaign resolved message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_reward_campaign_claimed(
        &self,
        tweet_id: &str,
        reward_amount: u64,
        coin_type: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let display_reward = Self::format_token_amount(reward_amount, coin_type);
        let message = format!(
            "Reward claimed.\n\nAmount: {}\n\nhttps://suiscan.xyz/testnet/tx/{}",
            display_reward, tx_digest
        );

        info!(tweet_id = %tweet_id, "Replying with reward campaign claimed message");
        self.reply_to_tweet(tweet_id, &message).await
    }

    pub async fn reply_reward_campaign_no_winners(
        &self,
        tweet_id: &str,
        total_budget: u64,
        coin_type: &str,
        tx_digest: &str,
    ) -> Result<String> {
        let display_budget = Self::format_token_amount(total_budget, coin_type);
        let message = format!(
            "Reward campaign solved.\n\nNo eligible winners found. {} was refunded to the creator.\n\nhttps://suiscan.xyz/testnet/tx/{}",
            display_budget, tx_digest
        );

        info!(tweet_id = %tweet_id, "Replying with reward campaign no-winners message");
        self.reply_to_tweet(tweet_id, &message).await
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
            View on Suiscan:\n\
            https://suiscan.xyz/testnet/tx/{}",
            handle, tx_digest
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
            View on Suiscan:\n\
            https://suiscan.xyz/testnet/tx/{}",
            handle, short_address, tx_digest
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
        let url = "https://api.twitterapi.io/twitter/user/info";

        let response = self
            .http_client
            .get(url)
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

    /// Reply to a tweet with error message
    #[allow(dead_code)]
    pub async fn reply_error(&self, tweet_id: &str, error_message: &str) -> Result<String> {
        let message = format!(
            "Transaction failed\n\n\
            Error: {}\n\n\
            Please check your command and try again.",
            error_message
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
        if !self.replies_enabled {
            info!(
                tweet_id = %tweet_id,
                reply_text = %text,
                "Twitter replies disabled; skipping reply post"
            );
            return Ok(format!("twitter-replies-disabled:{tweet_id}"));
        }

        let url = "https://api.twitterapi.io/twitter/create_tweet_v2";
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
            .post(url)
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
