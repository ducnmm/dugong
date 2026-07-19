#![allow(dead_code)]

use anyhow::{Context, Result};
use base64::{
    engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD as BASE64_URL},
    Engine,
};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::config::Config;
use crate::oauth::{mint_fresh_access_token, MintError, MintedAccessToken};

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

fn campaign_insufficient_balance_message(
    handle: &str,
    reward_display: &str,
    max_winners: u64,
    total_budget_display: &str,
) -> String {
    format!(
        "@{} — your @DugongWallet account doesn't have enough balance for this reward campaign.\n\n\
        This campaign needs {} total ({} each × {} winners).\n\n\
        Reduce the reward or winner count, or deposit more funds and try again.",
        handle, total_budget_display, reward_display, max_winners
    )
}

fn transfer_insufficient_balance_message(handle: &str, amount_display: &str) -> String {
    format!(
        "@{} — your @DugongWallet account doesn't have enough balance to send {}.\n\n\
        Reduce the amount or deposit more funds and try again.",
        handle, amount_display
    )
}

fn bet_insufficient_balance_message(handle: &str, amount_display: &str) -> String {
    format!(
        "@{} — your @DugongWallet account doesn't have enough balance to place a {} prediction.\n\n\
        Reduce the amount or deposit more funds and try again.",
        handle, amount_display
    )
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

/// User-facing OAuth 2.0 authorization page (NOT `api.twitter.com`). This is
/// where a person grants the app access; the returned `code` is then exchanged
/// for tokens at `{TWITTER_API_BASE_URL}/2/oauth2/token`.
pub const TWITTER_AUTHORIZE_URL: &str = "https://x.com/i/oauth2/authorize";

/// A PKCE (RFC 7636) verifier/challenge pair for the authorization-code flow.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// Generate a fresh PKCE pair: a base64url `verifier` (32 random bytes) and its
/// S256 `challenge` (base64url(SHA-256(verifier))).
pub fn generate_pkce() -> Pkce {
    use aes_gcm::aead::{rand_core::RngCore, OsRng};
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let verifier = BASE64_URL.encode(bytes);
    let challenge = BASE64_URL.encode(Sha256::digest(verifier.as_bytes()));
    Pkce { verifier, challenge }
}

/// Generate a random `state` value (base64url, 16 bytes) for CSRF protection.
pub fn generate_state() -> String {
    use aes_gcm::aead::{rand_core::RngCore, OsRng};
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    BASE64_URL.encode(bytes)
}

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

    /// Construct directly from credentials, for out-of-process helpers that do
    /// not build a full server [`Config`] (e.g. `dugong-bot-authorize`).
    pub fn from_parts(client_id: String, client_secret: String, api_base: String) -> Self {
        Self {
            http_client: Client::new(),
            client_id,
            client_secret,
            api_base,
        }
    }

    /// Build the user-facing authorization URL to open in a browser. `scopes`
    /// are space-joined; PKCE uses S256. Query values are percent-encoded.
    pub fn authorize_url(
        &self,
        redirect_uri: &str,
        scopes: &[&str],
        state: &str,
        code_challenge: &str,
    ) -> String {
        reqwest::Url::parse_with_params(
            TWITTER_AUTHORIZE_URL,
            &[
                ("response_type", "code"),
                ("client_id", self.client_id.as_str()),
                ("redirect_uri", redirect_uri),
                ("scope", &scopes.join(" ")),
                ("state", state),
                ("code_challenge", code_challenge),
                ("code_challenge_method", "S256"),
            ],
        )
        .expect("authorize URL parameters are always serializable")
        .to_string()
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

/// Client for the bot's Twitter interactions.
///
/// - **Reply posting** goes through the official X API (`POST /2/tweets`),
///   authenticating as the bot account via [`BotPoster`].
/// - **Reads** (public user lookup, campaign candidate search) still go through
///   TwitterAPI.io with `X-API-Key`.
pub struct TwitterClient {
    http_client: Client,
    twitterapi_io_api_key: String,
    twitterapi_io_base: String,
    /// Base URL for the official X API (`POST /2/tweets`), from `TWITTER_API_BASE_URL`.
    twitter_api_base: String,
    /// Bot posting credentials. `None` disables official-API reply posting
    /// (reads still work); posting then fails with a clear config error.
    bot: Option<BotAuth>,
    docs_url: String,
    web_url: String,
}

/// How long before an access token's expiry we proactively refresh it, so an
/// in-flight request never races the expiry boundary.
const TOKEN_EXPIRY_SKEW_SECS: i64 = 60;

/// How the bot authenticates `POST /2/tweets` calls. OAuth 1.0a keys win over
/// the OAuth 2.0 path when both are configured (see `Config` field docs).
enum BotAuth {
    /// Static OAuth 1.0a user-context keys — each request is HMAC-SHA1 signed;
    /// nothing is stored, refreshed or rotated.
    OAuth1(OAuth1Credentials),
    /// DB-stored OAuth 2.0 user-context token, minted/refreshed on demand.
    OAuth2(BotPoster),
}

/// OAuth 1.0a user-context credentials from the X developer portal's
/// "Keys and tokens" page: the app's consumer API key/secret plus the bot
/// account's access token/secret.
#[derive(Clone)]
pub struct OAuth1Credentials {
    pub api_key: String,
    pub api_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
}

impl OAuth1Credentials {
    /// All four values from config, or `None` if any is missing (partial
    /// configuration is rejected at startup by `Config::ensure_reply_capable`).
    fn from_config(config: &Config) -> Option<Self> {
        Some(Self {
            api_key: config.twitter_api_key.clone()?,
            api_secret: config.twitter_api_secret.clone()?,
            access_token: config.twitter_access_token.clone()?,
            access_token_secret: config.twitter_access_token_secret.clone()?,
        })
    }

    /// `Authorization: OAuth ...` header value for `method` on `url` with a
    /// fresh nonce and the current timestamp.
    fn authorization_header(&self, method: &str, url: &str) -> String {
        use aes_gcm::aead::{rand_core::RngCore, OsRng};
        let mut nonce_bytes = [0u8; 16];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = hex::encode(nonce_bytes);
        let timestamp = Utc::now().timestamp().to_string();
        self.authorization_header_at(method, url, &nonce, &timestamp)
    }

    /// Deterministic core of [`Self::authorization_header`], split out so the
    /// signature can be tested against a fixed nonce/timestamp.
    ///
    /// `url` must be exactly the URL the request is sent to, with no query
    /// string. The JSON body of `POST /2/tweets` is not form-encoded, so per
    /// the OAuth 1.0a spec only the `oauth_*` protocol parameters enter the
    /// signature base string.
    fn authorization_header_at(
        &self,
        method: &str,
        url: &str,
        nonce: &str,
        timestamp: &str,
    ) -> String {
        // Already in the byte-sorted order the base string requires.
        let params = [
            ("oauth_consumer_key", self.api_key.as_str()),
            ("oauth_nonce", nonce),
            ("oauth_signature_method", "HMAC-SHA1"),
            ("oauth_timestamp", timestamp),
            ("oauth_token", self.access_token.as_str()),
            ("oauth_version", "1.0"),
        ];
        let param_string = params
            .iter()
            .map(|(k, v)| format!("{}={}", oauth1_percent_encode(k), oauth1_percent_encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let base_string = format!(
            "{}&{}&{}",
            method.to_uppercase(),
            oauth1_percent_encode(url),
            oauth1_percent_encode(&param_string)
        );
        let signing_key = format!(
            "{}&{}",
            oauth1_percent_encode(&self.api_secret),
            oauth1_percent_encode(&self.access_token_secret)
        );

        let mut mac = Hmac::<Sha1>::new_from_slice(signing_key.as_bytes())
            .expect("HMAC accepts keys of any length");
        mac.update(base_string.as_bytes());
        let signature = BASE64.encode(mac.finalize().into_bytes());

        let header_params = params
            .iter()
            .map(|(k, v)| (*k, (*v).to_string()))
            .chain(std::iter::once(("oauth_signature", signature)))
            .map(|(k, v)| format!("{}=\"{}\"", k, oauth1_percent_encode(&v)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("OAuth {header_params}")
    }
}

/// Percent-encode per OAuth 1.0a's strict RFC 3986 rules: everything except
/// ALPHA / DIGIT / `-` / `.` / `_` / `~` is `%XX`-escaped (uppercase hex).
fn oauth1_percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Everything needed to post as the bot: DB access to its stored OAuth token,
/// the server config (encryption key + OAuth client credentials + api base), the
/// bot's X user id, and an in-memory access-token cache.
///
/// The cache matters: X access tokens last ~2h, and every refresh **rotates**
/// the refresh token. Refreshing per-reply would both waste calls and risk
/// rotation races, so we mint once and reuse until near expiry.
struct BotPoster {
    pool: PgPool,
    config: Config,
    bot_xid: String,
    cached: Arc<Mutex<Option<MintedAccessToken>>>,
}

impl BotPoster {
    /// Return a valid bot access token, minting a fresh one only when the cache
    /// is empty or within [`TOKEN_EXPIRY_SKEW_SECS`] of expiring.
    async fn access_token(&self) -> Result<String, MintError> {
        let mut guard = self.cached.lock().await;
        if let Some(cached) = guard.as_ref() {
            if !cached_token_is_stale(cached) {
                return Ok(cached.access_token.clone());
            }
        }
        let minted = mint_fresh_access_token(&self.pool, &self.config, &self.bot_xid).await?;
        let access = minted.access_token.clone();
        *guard = Some(minted);
        Ok(access)
    }

    /// Force a refresh regardless of cache state (used after a mid-flight 401),
    /// replacing the cached token.
    async fn force_refresh(&self) -> Result<String, MintError> {
        let mut guard = self.cached.lock().await;
        let minted = mint_fresh_access_token(&self.pool, &self.config, &self.bot_xid).await?;
        let access = minted.access_token.clone();
        *guard = Some(minted);
        Ok(access)
    }
}

/// A cached token is stale when it is unexpiring-unknown (mint again to be safe)
/// or within the skew window of its expiry.
fn cached_token_is_stale(token: &MintedAccessToken) -> bool {
    match token.expires_at {
        Some(expires_at) => {
            Utc::now() + chrono::Duration::seconds(TOKEN_EXPIRY_SKEW_SECS) >= expires_at
        }
        None => true,
    }
}

/// Request body for `POST /2/tweets` when posting a reply.
#[derive(Debug, Serialize)]
struct CreateReplyRequest<'a> {
    text: &'a str,
    reply: ReplyRef<'a>,
}

#[derive(Debug, Serialize)]
struct ReplyRef<'a> {
    in_reply_to_tweet_id: &'a str,
}

/// Success envelope for `POST /2/tweets` (`{ "data": { "id": "...", ... } }`).
#[derive(Debug, Deserialize)]
struct CreateTweetV2Response {
    data: CreatedTweet,
}

#[derive(Debug, Deserialize)]
struct CreatedTweet {
    id: String,
}

/// A create-reply POST failed. `Unauthorized` (HTTP 401) is separated so the
/// caller can refresh the bot token once and retry.
#[derive(Debug)]
enum PostReplyError {
    Unauthorized(String),
    Other(anyhow::Error),
}

/// The create-tweet endpoint under `api_base` (`POST /2/tweets`). OAuth 1.0a
/// signatures cover the exact request URL, so both the signer and the request
/// must derive it from this one place.
fn create_tweet_url(api_base: &str) -> String {
    format!("{}/2/tweets", api_base.trim_end_matches('/'))
}

/// Post a reply to `tweet_id` via the official X API (`POST /2/tweets` at
/// `url`), authenticating with the given `Authorization` header value
/// (`Bearer ...` or a signed `OAuth ...`). Returns the created tweet id.
///
/// Standalone (not a method) so the HTTP/serialization behavior can be tested
/// against a mock server without a database-backed [`BotPoster`].
async fn create_reply_tweet(
    http_client: &Client,
    url: &str,
    authorization: &str,
    tweet_id: &str,
    text: &str,
) -> Result<String, PostReplyError> {
    let body = CreateReplyRequest {
        text,
        reply: ReplyRef {
            in_reply_to_tweet_id: tweet_id,
        },
    };

    let response = http_client
        .post(url)
        .header("Authorization", authorization)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            PostReplyError::Other(anyhow::Error::new(e).context("failed to send create-tweet request"))
        })?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| {
            PostReplyError::Other(
                anyhow::Error::new(e).context("failed to read create-tweet response body"),
            )
        })?;

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(PostReplyError::Unauthorized(format!(
            "X API create tweet unauthorized (401): {response_text}"
        )));
    }
    if !status.is_success() {
        // 403 with a duplicate-content message is expected when the same reply
        // text is posted twice; callers vary text per account to avoid it.
        return Err(PostReplyError::Other(anyhow::anyhow!(
            "X API create tweet error ({status}): {response_text}"
        )));
    }

    let parsed: CreateTweetV2Response = serde_json::from_str(&response_text).map_err(|e| {
        PostReplyError::Other(
            anyhow::Error::new(e)
                .context(format!("failed to parse create-tweet response: {response_text}")),
        )
    })?;
    Ok(parsed.data.id)
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
    /// Reads-only client (no bot posting configured). Posting a reply errors
    /// clearly; used where only lookups/search are needed and in tests.
    pub fn new(config: &Config) -> Self {
        Self::build(config, config.twitterapi_io_base.clone(), None)
    }

    /// Reply-capable client that posts as the bot. Prefers OAuth 1.0a keys
    /// when fully configured (`TWITTER_API_KEY` etc.); otherwise falls back to
    /// the stored OAuth 2.0 token, which requires `TWITTER_BOT_USER_ID`. With
    /// neither, the client still reads, but posting fails with a clear config
    /// error at call time.
    pub fn new_with_bot(config: &Config, pool: PgPool) -> Self {
        let bot = if let Some(creds) = OAuth1Credentials::from_config(config) {
            Some(BotAuth::OAuth1(creds))
        } else {
            config.twitter_bot_user_id.clone().map(|bot_xid| {
                BotAuth::OAuth2(BotPoster {
                    pool,
                    config: config.clone(),
                    bot_xid,
                    cached: Arc::new(Mutex::new(None)),
                })
            })
        };
        Self::build(config, config.twitterapi_io_base.clone(), bot)
    }

    /// Construct a reads-only client pointed at a custom TwitterAPI.io base URL
    /// (used in tests to aim lookups/search at a mock server).
    pub fn with_base_url(config: &Config, twitterapi_io_base: String) -> Self {
        Self::build(config, twitterapi_io_base, None)
    }

    fn build(config: &Config, twitterapi_io_base: String, bot: Option<BotAuth>) -> Self {
        Self {
            http_client: Client::new(),
            twitterapi_io_api_key: config.twitterapi_io_api_key.clone(),
            twitterapi_io_base,
            twitter_api_base: config.twitter_api_base.clone(),
            bot,
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

    /// Reply when the sender cannot fund a transfer.
    pub async fn reply_transfer_insufficient_balance(
        &self,
        tweet_id: &str,
        handle: &str,
        amount_display: &str,
    ) -> Result<String> {
        let message = transfer_insufficient_balance_message(handle, amount_display);
        info!(
            tweet_id = %tweet_id,
            handle = %handle,
            "Replying with transfer insufficient balance message"
        );
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
        // Twitter rejects verbatim-duplicate tweet text (HTTP 403 on the
        // official API), so the reply must vary per account. Include the
        // @handle and the unique account object id (also the useful info to
        // surface to the user).
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

    /// Reply when the user cannot fund a market prediction.
    pub async fn reply_bet_insufficient_balance(
        &self,
        tweet_id: &str,
        handle: &str,
        amount_display: &str,
    ) -> Result<String> {
        let message = bet_insufficient_balance_message(handle, amount_display);
        info!(
            tweet_id = %tweet_id,
            handle = %handle,
            "Replying with bet insufficient balance message"
        );
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

    /// Reply when the creator cannot fund the campaign's full reward budget.
    pub async fn reply_campaign_insufficient_balance(
        &self,
        tweet_id: &str,
        handle: &str,
        reward_display: &str,
        max_winners: u64,
        total_budget_display: &str,
    ) -> Result<String> {
        let message = campaign_insufficient_balance_message(
            handle,
            reward_display,
            max_winners,
            total_budget_display,
        );
        info!(tweet_id = %tweet_id, handle = %handle, "Replying with campaign insufficient balance message");
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

    /// Post a reply to a specific tweet via the official X API, as the bot
    /// account. Mints/reuses the bot's cached access token; on a mid-flight 401
    /// (token revoked/expired between mint and use) it force-refreshes once and
    /// retries before giving up.
    async fn reply_to_tweet(&self, tweet_id: &str, text: &str) -> Result<String> {
        let bot = self.bot.as_ref().ok_or_else(|| {
            warn!(
                tweet_id = %tweet_id,
                reply_text = %text,
                "Reply not posted because official-API posting is not configured"
            );
            anyhow::anyhow!(
                "official X API posting is not configured: set the OAuth 1.0a keys \
                 (TWITTER_API_KEY/TWITTER_API_SECRET + TWITTER_ACCESS_TOKEN/\
                 TWITTER_ACCESS_TOKEN_SECRET), or set TWITTER_BOT_USER_ID and authorize \
                 the bot account with `dugong-bot-authorize`"
            )
        })?;
        let url = create_tweet_url(&self.twitter_api_base);

        match bot {
            // Static keys: sign and send. A 401 is terminal — there is nothing
            // to refresh; the keys themselves are wrong or revoked.
            BotAuth::OAuth1(creds) => {
                let authorization = creds.authorization_header("POST", &url);
                match create_reply_tweet(&self.http_client, &url, &authorization, tweet_id, text)
                    .await
                {
                    Ok(reply_tweet_id) => {
                        info!(reply_tweet_id = %reply_tweet_id, "Successfully posted reply tweet");
                        Ok(reply_tweet_id)
                    }
                    Err(PostReplyError::Unauthorized(msg)) => {
                        warn!(
                            tweet_id = %tweet_id,
                            detail = %msg,
                            "Reply not posted: X API rejected the OAuth 1.0a credentials"
                        );
                        Err(anyhow::anyhow!(
                            "X API rejected the OAuth 1.0a credentials (401). Check \
                             TWITTER_API_KEY/TWITTER_API_SECRET and TWITTER_ACCESS_TOKEN/\
                             TWITTER_ACCESS_TOKEN_SECRET, and regenerate them if they were \
                             revoked: {msg}"
                        ))
                    }
                    Err(PostReplyError::Other(err)) => {
                        warn!(
                            tweet_id = %tweet_id,
                            reply_text = %text,
                            error = %err,
                            "Reply not posted because the X API create-tweet call failed"
                        );
                        Err(err)
                    }
                }
            }
            BotAuth::OAuth2(poster) => {
                let access_token = poster.access_token().await.map_err(anyhow::Error::new)?;

                match create_reply_tweet(
                    &self.http_client,
                    &url,
                    &format!("Bearer {access_token}"),
                    tweet_id,
                    text,
                )
                .await
                {
                    Ok(reply_tweet_id) => {
                        info!(reply_tweet_id = %reply_tweet_id, "Successfully posted reply tweet");
                        Ok(reply_tweet_id)
                    }
                    Err(PostReplyError::Unauthorized(msg)) => {
                        warn!(
                            tweet_id = %tweet_id,
                            detail = %msg,
                            "Bot access token rejected (401); refreshing and retrying once"
                        );
                        let access_token =
                            poster.force_refresh().await.map_err(anyhow::Error::new)?;
                        let reply_tweet_id = create_reply_tweet(
                            &self.http_client,
                            &url,
                            &format!("Bearer {access_token}"),
                            tweet_id,
                            text,
                        )
                        .await
                        .map_err(|e| match e {
                            PostReplyError::Unauthorized(msg) => {
                                anyhow::anyhow!(
                                    "X API rejected bot token even after refresh: {msg}"
                                )
                            }
                            PostReplyError::Other(err) => err,
                        })?;
                        info!(
                            reply_tweet_id = %reply_tweet_id,
                            "Successfully posted reply tweet after token refresh"
                        );
                        Ok(reply_tweet_id)
                    }
                    Err(PostReplyError::Other(err)) => {
                        warn!(
                            tweet_id = %tweet_id,
                            reply_text = %text,
                            error = %err,
                            "Reply not posted because the X API create-tweet call failed"
                        );
                        Err(err)
                    }
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn campaign_insufficient_balance_message_explains_required_budget() {
        let message = campaign_insufficient_balance_message("Z3ro_0102", "5 DUG", 3, "15 DUG");

        assert_eq!(
            message,
            "@Z3ro_0102 — your @DugongWallet account doesn't have enough balance for this reward campaign.\n\n\
            This campaign needs 15 DUG total (5 DUG each × 3 winners).\n\n\
            Reduce the reward or winner count, or deposit more funds and try again."
        );
    }

    #[test]
    fn transfer_insufficient_balance_message_explains_required_amount() {
        let message = transfer_insufficient_balance_message("sender", "5 DUG");

        assert_eq!(
            message,
            "@sender — your @DugongWallet account doesn't have enough balance to send 5 DUG.\n\n\
            Reduce the amount or deposit more funds and try again."
        );
    }

    #[test]
    fn bet_insufficient_balance_message_explains_required_stake() {
        let message = bet_insufficient_balance_message("predictor", "2.5 USDC");

        assert_eq!(
            message,
            "@predictor — your @DugongWallet account doesn't have enough balance to place a 2.5 USDC prediction.\n\n\
            Reduce the amount or deposit more funds and try again."
        );
    }

    #[tokio::test]
    async fn create_reply_tweet_posts_and_parses_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/2/tweets"))
            .and(header("Authorization", "Bearer test-token"))
            .and(body_json(serde_json::json!({
                "text": "hi there",
                "reply": { "in_reply_to_tweet_id": "123" }
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "data": { "id": "1899999999999999999", "text": "hi there" }
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let url = create_tweet_url(&server.uri());
        let id = create_reply_tweet(&client, &url, "Bearer test-token", "123", "hi there")
            .await
            .expect("reply should succeed");
        assert_eq!(id, "1899999999999999999");
    }

    #[tokio::test]
    async fn create_reply_tweet_maps_401_to_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/2/tweets"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "title": "Unauthorized",
                "status": 401
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let url = create_tweet_url(&server.uri());
        let err = create_reply_tweet(&client, &url, "Bearer bad-token", "123", "hi")
            .await
            .expect_err("401 must be an error");
        assert!(matches!(err, PostReplyError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn create_reply_tweet_surfaces_other_errors() {
        // X returns 403 for duplicate content — surfaced as a generic Other error.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/2/tweets"))
            .respond_with(ResponseTemplate::new(403).set_body_string("duplicate content"))
            .mount(&server)
            .await;

        let client = Client::new();
        let url = create_tweet_url(&server.uri());
        let err = create_reply_tweet(&client, &url, "Bearer tok", "123", "dup")
            .await
            .expect_err("403 must be an error");
        assert!(matches!(err, PostReplyError::Other(_)));
    }

    /// Reference signature computed independently (Python hmac/sha1) for the
    /// same credentials, nonce and timestamp.
    #[test]
    fn oauth1_authorization_header_matches_reference_signature() {
        let creds = OAuth1Credentials {
            api_key: "test-consumer-key".to_string(),
            api_secret: "test-consumer-secret".to_string(),
            access_token: "test-access-token".to_string(),
            access_token_secret: "test-token-secret".to_string(),
        };
        let header = creds.authorization_header_at(
            "post",
            "https://api.twitter.com/2/tweets",
            "abc123nonce",
            "1752400000",
        );
        assert!(header.starts_with("OAuth "), "header was: {header}");
        assert!(
            header.contains(r#"oauth_signature="I5JOgZqPHoigfAzfzXom%2B1lDsR8%3D""#),
            "header was: {header}"
        );
        assert!(header.contains(r#"oauth_consumer_key="test-consumer-key""#));
        assert!(header.contains(r#"oauth_token="test-access-token""#));
        assert!(header.contains(r#"oauth_signature_method="HMAC-SHA1""#));
    }

    #[test]
    fn oauth1_percent_encode_escapes_reserved_bytes() {
        assert_eq!(oauth1_percent_encode("Ab1-._~"), "Ab1-._~");
        assert_eq!(oauth1_percent_encode("a b+c/d="), "a%20b%2Bc%2Fd%3D");
    }

    #[test]
    fn generate_pkce_produces_distinct_url_safe_pairs() {
        let a = generate_pkce();
        let b = generate_pkce();
        assert_ne!(a.verifier, b.verifier, "verifiers must be random");
        assert_ne!(a.challenge, b.challenge, "challenges must differ");
        for s in [&a.verifier, &a.challenge] {
            assert!(
                !s.contains('+') && !s.contains('/') && !s.contains('='),
                "PKCE values must be base64url (no +, /, or padding): {s}"
            );
        }
    }

    #[test]
    fn authorize_url_includes_pkce_scopes_and_state() {
        let oauth = TwitterOAuth2Client::from_parts(
            "my-client-id".to_string(),
            "secret".to_string(),
            TWITTER_API_BASE_URL.to_string(),
        );
        let url = oauth.authorize_url(
            "https://app.example/callback",
            &["tweet.read", "tweet.write", "offline.access"],
            "state-xyz",
            "challenge-abc",
        );
        assert!(url.starts_with("https://x.com/i/oauth2/authorize?"));
        assert!(url.contains("client_id=my-client-id"));
        assert!(url.contains("code_challenge=challenge-abc"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=state-xyz"));
        assert!(url.contains("response_type=code"));
        // Scopes are space-joined then query-encoded; each token must appear.
        assert!(url.contains("tweet.read"));
        assert!(url.contains("tweet.write"));
        assert!(url.contains("offline.access"));
    }

    #[test]
    fn cached_token_stale_logic() {
        let fresh = MintedAccessToken {
            access_token: "a".to_string(),
            expires_at: Some(Utc::now() + chrono::Duration::seconds(3600)),
        };
        assert!(!cached_token_is_stale(&fresh), "far-future token is fresh");

        let expiring = MintedAccessToken {
            access_token: "a".to_string(),
            expires_at: Some(Utc::now() + chrono::Duration::seconds(10)),
        };
        assert!(
            cached_token_is_stale(&expiring),
            "token within skew window is stale"
        );

        let unknown = MintedAccessToken {
            access_token: "a".to_string(),
            expires_at: None,
        };
        assert!(cached_token_is_stale(&unknown), "unknown expiry is stale");
    }
}
