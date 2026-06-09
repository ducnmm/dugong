use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    Json,
};
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::webhook::handler::AppState;
use dugong_core::clients::enclave::EnclaveClient;
use dugong_core::clients::enoki::EnokiClient;
use dugong_core::clients::sui_transaction::SuiTransactionBuilder;
use dugong_core::clients::twitter::{
    OAuth2TokenResponse, RefreshError, TwitterOAuth2Client, TwitterUserInfo,
};
use dugong_core::config::Config;
use dugong_core::db::models::{
    AccountBalance, DugongAccount, Transfer, TransferType, TwitterOAuthToken,
};

/// Lifetime of a backend session token. Long-lived because it tracks the OAuth
/// login cadence (the stored refresh token keeps Twitter access alive underneath).
const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

fn coin_display_metadata(coin_type: &str) -> (String, u8) {
    if coin_type.ends_with("::sui::SUI") {
        ("SUI".to_string(), 9)
    } else if coin_type.ends_with("::wal::WAL") {
        ("WAL".to_string(), 9)
    } else if coin_type.ends_with("::usdc::USDC") {
        ("USDC".to_string(), 6)
    } else if coin_type.ends_with("::dug::DUG") || coin_type.ends_with("::core::CORE") {
        ("DUG".to_string(), 9)
    } else {
        let symbol = coin_type
            .split("::")
            .last()
            .unwrap_or("UNKNOWN")
            .to_string();
        (symbol, 9)
    }
}

fn configured_tx_base_url() -> String {
    std::env::var("TX_BASE_URL")
        .or_else(|_| std::env::var("TX_EXPLORER_URL"))
        .unwrap_or_else(|_| "http://127.0.0.1:3004".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn configured_tx_card_image_url() -> String {
    std::env::var("TX_CARD_IMAGE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            let web_url = std::env::var("WEB_URL")
                .or_else(|_| std::env::var("FRONTEND_URL"))
                .or_else(|_| std::env::var("PUBLIC_WEB_URL"))
                .unwrap_or_else(|_| "https://dugong.up.railway.app".to_string());
            format!("{}/android-chrome-512x512.png", web_url.trim_end_matches('/'))
        })
        .trim_end_matches('/')
        .to_string()
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&#39;")
}

fn format_amount(amount: i64, decimals: u8) -> String {
    let divisor = 10f64.powi(decimals as i32);
    let formatted = (amount as f64 / divisor).to_string();
    let mut value = format!("{:.*}", decimals as usize, formatted.parse::<f64>().unwrap_or(0.0))
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string();

    if value.is_empty() {
        value.push('0');
    }
    value
}

fn short_digest(digest: &str) -> String {
    if digest.len() <= 12 {
        digest.to_string()
    } else {
        format!("{}...{}", &digest[0..6], &digest[digest.len() - 6..])
    }
}

fn format_tx_time_ms(timestamp_ms: i64) -> String {
    if timestamp_ms <= 0 {
        return "Unknown".to_string();
    }
    let ts = if timestamp_ms > 1_000_000_000_000 {
        timestamp_ms
    } else {
        timestamp_ms.saturating_mul(1000)
    };
    Utc.timestamp_millis_opt(ts)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn build_tx_card_html(transfer: &Transfer, decimals: u8) -> String {
    let tx_type = match transfer.transfer_type {
        TransferType::Transfer => "Transfer",
        TransferType::Deposit => "Deposit",
        TransferType::Withdraw => "Withdraw",
    };
    let amount = format_amount(transfer.amount, decimals);
    let (symbol, _) = coin_display_metadata(&transfer.coin_type);
    let from_label = transfer
        .from_xid
        .as_deref()
        .unwrap_or("Unknown");
    let to_label = transfer.to_xid.as_deref().unwrap_or("Unknown");
    let digest = short_digest(&transfer.transaction_digest);
    let time = format_tx_time_ms(transfer.timestamp);
    let base_url = configured_tx_base_url();
    let canonical = format!("{}/tx/{}", base_url, transfer.transaction_digest);
    let card_image = configured_tx_card_image_url();

    let title = format!("Dugong • {tx_type} {amount} {symbol}");
    let description = format!(
        "{tx_type} transaction\nAmount: {amount} {symbol}\nFrom: {from_label}\nTo: {to_label}\nTx: {digest}\nTime: {time}"
    );

    let body = format!(
        r#"
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{title}</title>
    <meta name="description" content="{description_attr}" />
    <meta property="og:type" content="website" />
    <meta property="og:title" content="{title_attr}" />
    <meta property="og:description" content="{description_attr}" />
    <meta property="og:url" content="{canonical}" />
    <meta name="twitter:card" content="summary_large_image" />
    <meta name="twitter:title" content="{title_attr}" />
    <meta name="twitter:description" content="{description_attr}" />
    <meta name="twitter:image" content="{card_image}" />
    <meta property="og:image" content="{card_image}" />
    <meta property="og:image:secure_url" content="{card_image}" />
    <meta property="og:image:type" content="image/png" />
    <meta name="twitter:site" content="@DugongWallet" />
    <meta name="twitter:label1" content="Amount" />
    <meta name="twitter:data1" content="{amount} {symbol}" />
    <meta name="twitter:label2" content="Type" />
    <meta name="twitter:data2" content="{tx_type}" />
    <style>
      :root {{
        color-scheme: dark;
      }}
      body {{
        margin: 0;
        font-family: "Inter", "Segoe UI", Arial, sans-serif;
        background: radial-gradient(circle at 10% 10%, #172554 0%, #0b1020 30%, #020617 100%);
        color: #f8fafc;
        min-height: 100vh;
        display: grid;
        place-items: center;
        padding: 24px;
      }}
      .card {{
        width: min(720px, calc(100vw - 32px));
        background: #0f172a;
        border: 1px solid #1e293b;
        border-radius: 24px;
        padding: 28px;
        box-shadow: 0 22px 56px rgba(0, 0, 0, 0.5);
      }}
      h1 {{
        margin: 0 0 8px;
        font-size: 34px;
        letter-spacing: 0.01em;
        line-height: 1.1;
      }}
      .muted {{
        color: #94a3b8;
        font-size: 14px;
      }}
      .grid {{
        margin-top: 16px;
        border-top: 1px solid #334155;
      }}
      .row {{
        padding: 10px 0;
        border-bottom: 1px solid #334155;
        display: flex;
        justify-content: space-between;
        gap: 12px;
      }}
      .row:last-child {{
        border-bottom: 0;
      }}
      .label {{
        color: #cbd5e1;
        font-weight: 600;
      }}
      .value {{
        font-weight: 700;
        text-align: right;
        word-break: break-all;
      }}
      .hero {{
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
      }}
      .hero-pill {{
        background: #0ea5e9;
        color: #0b1220;
        border-radius: 999px;
        font-weight: 800;
        padding: 7px 12px;
        font-size: 13px;
        text-transform: uppercase;
      }}
      .footer {{
        margin-top: 16px;
        display: flex;
        justify-content: space-between;
        gap: 12px;
        font-size: 12px;
        color: #94a3b8;
      }}
      a {{
        color: #7dd3fc;
      }}
    </style>
  </head>
  <body>
    <main class="card">
      <div class="hero">
        <h1>{title_display}</h1>
        <span class="hero-pill">Tx</span>
      </div>
      <p class="muted">{short_digest_display}</p>
      <div class="grid">
        <div class="row"><span class="label">Type</span><span class="value">{tx_type}</span></div>
        <div class="row"><span class="label">Amount</span><span class="value">{amount_display} {symbol}</span></div>
        <div class="row"><span class="label">From</span><span class="value">{from_label}</span></div>
        <div class="row"><span class="label">To</span><span class="value">{to_label}</span></div>
        <div class="row"><span class="label">Time</span><span class="value">{time}</span></div>
      </div>
      <div class="footer">
        <span>Created: {created_at}</span>
        <a href="{canonical}">Open card</a>
      </div>
    </main>
  </body>
</html>
"#,
        title = escape_html(&title),
        title_attr = escape_html(&title),
        description_attr = escape_html(&description),
        amount = escape_html(&amount),
        symbol = escape_html(&symbol),
        base_url = base_url,
        canonical = escape_html(&canonical),
        tx_type = tx_type,
        from_label = escape_html(from_label),
        to_label = escape_html(to_label),
        amount_display = escape_html(&amount),
        title_display = escape_html(&title),
        short_digest_display = escape_html(&digest),
        time = escape_html(&time),
        card_image = escape_html(&card_image),
        created_at = escape_html(&transfer.created_at.to_rfc3339()),
    );

    body
}

/// Issue a backend session token binding a request to a verified `xid`.
fn issue_session(config: &Config, xid: &str) -> anyhow::Result<String> {
    dugong_core::session::issue(config.session_token_secret()?, xid, SESSION_TTL)
}

/// Recover the trusted `xid` from a request's `Authorization: Bearer <session>`
/// header, or `None` if absent/invalid/expired.
fn session_xid(config: &Config, headers: &HeaderMap) -> Option<String> {
    let secret = config.session_token_secret().ok()?;
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?
        .trim();
    dugong_core::session::verify(secret, token).ok()
}

/// Extract the xid embedded in a link message: `Link XID:{xid} to wallet {addr} at {ts}`.
fn message_xid(message: &str) -> Option<&str> {
    let rest = message.strip_prefix("Link XID:")?;
    let end = rest.find(" to wallet ")?;
    Some(&rest[..end])
}

/// Persist a user's Twitter OAuth credentials (encrypted at rest). No-op when the
/// response carries no refresh token (e.g. `offline.access` not granted).
async fn store_oauth_tokens(
    state: &AppState,
    xid: &str,
    tokens: &OAuth2TokenResponse,
) -> anyhow::Result<()> {
    let Some(refresh) = tokens.refresh_token.as_deref() else {
        return Ok(());
    };
    let key = state.config.token_encryption_key()?;
    let refresh_enc = dugong_core::crypto::seal(key, refresh)?;
    let access_enc = dugong_core::crypto::seal(key, &tokens.access_token)?;
    let expires_at = tokens
        .expires_in
        .map(|s| Utc::now() + chrono::Duration::seconds(s as i64));
    TwitterOAuthToken::upsert(
        &state.db,
        xid,
        &refresh_enc,
        Some(&access_enc),
        expires_at,
        tokens.scope.as_deref(),
    )
    .await?;
    Ok(())
}

/// Why a fresh access token could not be minted.
enum FreshTokenError {
    /// The user must re-authenticate with X (no stored token, or Twitter rejected it).
    ReauthRequired(String),
    /// A server-side/transient problem unrelated to the user's credentials.
    Internal(String),
}

/// Mint a fresh Twitter access token for a trusted `xid` using the stored refresh
/// token. Persists the rotated refresh token; drops a definitively-dead one.
async fn mint_fresh_access_token(state: &AppState, xid: &str) -> Result<String, FreshTokenError> {
    let key = state
        .config
        .token_encryption_key()
        .map_err(|e| FreshTokenError::Internal(e.to_string()))?;

    let stored = TwitterOAuthToken::find_by_x_user_id(&state.db, xid)
        .await
        .map_err(|e| FreshTokenError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| {
            FreshTokenError::ReauthRequired("no stored X session for this user".to_string())
        })?;

    let refresh = dugong_core::crypto::open(key, &stored.refresh_token_enc).map_err(|_| {
        FreshTokenError::ReauthRequired("stored X credential unreadable".to_string())
    })?;

    let oauth =
        TwitterOAuth2Client::with_base_url(&state.config, state.config.twitter_api_base.clone());
    match oauth.refresh_access_token(&refresh).await {
        Ok(resp) => {
            // Twitter rotates the refresh token — persist the new one.
            if let Err(err) = store_oauth_tokens(state, xid, &resp).await {
                tracing::warn!("failed to persist rotated Twitter token: {err:?}");
            }
            Ok(resp.access_token)
        }
        Err(RefreshError::ReauthRequired(msg)) => {
            // Dead refresh token: remove it so it is not retried.
            let _ = TwitterOAuthToken::delete(&state.db, xid).await;
            Err(FreshTokenError::ReauthRequired(msg))
        }
        Err(RefreshError::Transient(err)) => Err(FreshTokenError::Internal(err.to_string())),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountResponse {
    pub x_user_id: String,
    pub x_handle: String,
    pub sui_object_id: String,
    pub owner_address: Option<String>,
}

impl From<DugongAccount> for AccountResponse {
    fn from(account: DugongAccount) -> Self {
        Self {
            x_user_id: account.x_user_id,
            x_handle: account.x_handle,
            sui_object_id: account.sui_object_id,
            owner_address: account.owner_address,
        }
    }
}

/// Get account by wallet address (owner_address)
pub async fn get_account_by_wallet(
    State(state): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> Result<Json<AccountResponse>, StatusCode> {
    match DugongAccount::find_by_owner_address(&state.db, &address).await {
        Ok(Some(account)) => Ok(Json(account.into())),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(err) => {
            tracing::error!("Failed to query account by wallet: {:?}", err);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub accounts: Vec<AccountResponse>,
    pub count: usize,
}

/// Search accounts by Twitter handle, user ID, or Sui address
pub async fn search_accounts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, StatusCode> {
    let query = params.q.trim();

    if query.is_empty() {
        return Ok(Json(SearchResponse {
            accounts: vec![],
            count: 0,
        }));
    }

    // Search by multiple criteria
    let accounts = match DugongAccount::search(&state.db, query).await {
        Ok(accounts) => accounts,
        Err(err) => {
            tracing::error!("Failed to search accounts: {:?}", err);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let count = accounts.len();
    let accounts: Vec<AccountResponse> = accounts.into_iter().map(|a| a.into()).collect();

    Ok(Json(SearchResponse { accounts, count }))
}

// ====== Account Detail API ======

#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    pub coin_type: String,
    pub balance: String,
}

#[derive(Debug, Serialize)]
pub struct AccountDetailResponse {
    pub account: AccountResponse,
    pub balances: Vec<BalanceResponse>,
}

/// Get account by Twitter user ID with balances
pub async fn get_account_by_twitter_id(
    State(state): State<Arc<AppState>>,
    Path(twitter_user_id): Path<String>,
) -> Result<Json<AccountDetailResponse>, StatusCode> {
    // Get account info
    let account = match DugongAccount::find_by_x_user_id(&state.db, &twitter_user_id).await {
        Ok(Some(account)) => account,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(err) => {
            tracing::error!("Failed to query account by twitter_id: {:?}", err);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get balances
    let balances = match AccountBalance::find_by_x_user_id(&state.db, &twitter_user_id).await {
        Ok(balances) => balances,
        Err(err) => {
            tracing::error!("Failed to query balances: {:?}", err);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    Ok(Json(AccountDetailResponse {
        account: account.into(),
        balances: balances
            .into_iter()
            .map(|b| BalanceResponse {
                coin_type: b.coin_type,
                balance: b.balance.to_string(),
            })
            .collect(),
    }))
}

#[derive(Debug, Serialize)]
pub struct AccountTransactionResponse {
    pub id: i32,
    pub transaction_digest: String,
    pub transfer_type: String,
    pub from_xid: Option<String>,
    pub to_xid: Option<String>,
    pub coin_type: String,
    pub amount: String,
    pub tweet_id: Option<String>,
    pub timestamp: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct AccountTransactionsResponse {
    pub transactions: Vec<AccountTransactionResponse>,
}

/// Get transactions for an account by Twitter user ID
pub async fn get_account_transactions(
    State(state): State<Arc<AppState>>,
    Path(twitter_user_id): Path<String>,
) -> Result<Json<AccountTransactionsResponse>, StatusCode> {
    let transfers = match Transfer::find_by_x_user_id(&state.db, &twitter_user_id, 50).await {
        Ok(transfers) => transfers,
        Err(err) => {
            tracing::error!("Failed to query transfers: {:?}", err);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    Ok(Json(AccountTransactionsResponse {
        transactions: transfers
            .into_iter()
            .map(|t| AccountTransactionResponse {
                id: t.id,
                transaction_digest: t.transaction_digest,
                transfer_type: match t.transfer_type {
                    TransferType::Transfer => "transfer".to_string(),
                    TransferType::Deposit => "deposit".to_string(),
                    TransferType::Withdraw => "withdraw".to_string(),
                },
                from_xid: t.from_xid,
                to_xid: t.to_xid,
                coin_type: t.coin_type,
                amount: t.amount.to_string(),
                tweet_id: t.tweet_id,
                timestamp: t.timestamp,
                created_at: t.created_at.to_rfc3339(),
            })
            .collect(),
    }))
}

// ====== Secure Link Wallet API ======

/// Request to link a wallet. The caller is authenticated via the backend session
/// token (Authorization header); the Twitter access token is no longer trusted
/// from the client (it may be expired) — it is accepted but ignored for back-compat.
#[derive(Debug, Deserialize)]
pub struct SecureLinkWalletApiRequest {
    #[serde(default)]
    #[allow(dead_code)]
    pub access_token: Option<String>, // ignored; kept for back-compat with older clients
    pub wallet_address: String,   // Sui wallet address (0x...)
    pub wallet_signature: String, // Signature of the message by wallet (base64)
    pub message: String,          // The message that was signed
    pub timestamp: u64,           // Timestamp when message was created
}

/// Response for link wallet
#[derive(Debug, Serialize)]
pub struct LinkWalletResponse {
    pub success: bool,
    pub tx_digest: Option<String>,
    pub error: Option<String>,
    /// When true, the user's X session has expired and they must re-login before
    /// linking can succeed. The frontend routes to re-auth on this signal.
    pub reauth_required: bool,
}

impl LinkWalletResponse {
    fn success(tx_digest: String) -> Self {
        Self {
            success: true,
            tx_digest: Some(tx_digest),
            error: None,
            reauth_required: false,
        }
    }

    fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            tx_digest: None,
            error: Some(error.into()),
            reauth_required: false,
        }
    }

    fn reauth(error: impl Into<String>) -> Self {
        Self {
            success: false,
            tx_digest: None,
            error: Some(error.into()),
            reauth_required: true,
        }
    }
}

/// Secure link wallet endpoint
///
/// Flow:
/// 1. Authenticate the caller via the backend session token → trusted xid
/// 2. Verify the signed message is for that same xid
/// 3. Mint a FRESH Twitter access token from the stored refresh token
/// 4. Forward the fresh token to the Nautilus enclave for verification and signing
/// 5. Submit link_wallet transaction to Sui blockchain and return the digest
pub async fn secure_link_wallet(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SecureLinkWalletApiRequest>,
) -> Result<Json<LinkWalletResponse>, StatusCode> {
    tracing::info!(
        "Secure link wallet request for address: {}",
        request.wallet_address
    );

    // 1. Authenticate the caller. An expired Twitter access token is no longer
    //    proof of identity, so we trust only the backend session token.
    let xid = match session_xid(&state.config, &headers) {
        Some(xid) => xid,
        None => {
            tracing::warn!("Link wallet rejected: missing or invalid session token");
            return Ok(Json(LinkWalletResponse::reauth(
                "Your X session has expired. Please sign in with X again.",
            )));
        }
    };

    // 2. The signed message embeds the xid; it must match the authenticated user,
    //    so a caller cannot link a wallet on behalf of another X account.
    match message_xid(&request.message) {
        Some(msg_xid) if msg_xid == xid => {}
        Some(_) => {
            tracing::warn!("Link wallet rejected: message xid does not match session xid");
            return Ok(Json(LinkWalletResponse::failure(
                "Signed message does not match the authenticated X account.",
            )));
        }
        None => {
            return Ok(Json(LinkWalletResponse::failure("Malformed link message.")));
        }
    }

    // 3. Mint a fresh Twitter access token from the stored refresh token. Never
    //    trust the (possibly stale) token the browser may have sent.
    let access_token = match mint_fresh_access_token(&state, &xid).await {
        Ok(token) => token,
        Err(FreshTokenError::ReauthRequired(msg)) => {
            tracing::info!("Link wallet needs re-auth for xid {xid}: {msg}");
            return Ok(Json(LinkWalletResponse::reauth(
                "Your X session has expired. Please sign in with X again.",
            )));
        }
        Err(FreshTokenError::Internal(msg)) => {
            tracing::error!("Failed to mint fresh Twitter token for xid {xid}: {msg}");
            return Ok(Json(LinkWalletResponse::failure(
                "Could not verify your X session. Please try again.",
            )));
        }
    };

    // 4. Forward the FRESH token to the enclave for verification and signing.
    let enclave_client = EnclaveClient::new(&state.config.enclave_url);

    let signed_result = enclave_client
        .sign_secure_link_wallet(
            &access_token,
            &request.wallet_address,
            &request.wallet_signature,
            &request.message,
            request.timestamp,
        )
        .await;

    let signed_payload = match signed_result {
        Ok(payload) => payload,
        Err(err) => {
            tracing::error!("Enclave verification failed: {:?}", err);
            return Ok(Json(LinkWalletResponse::failure(format!(
                "Verification failed: {}",
                err
            ))));
        }
    };

    tracing::info!(
        "Enclave signed link wallet for XID: {:?}",
        String::from_utf8_lossy(&signed_payload.response.data.xid)
    );
    tracing::info!(
        intent = signed_payload.response.intent,
        timestamp = signed_payload.response.timestamp_ms,
        "Received secure link wallet signature from enclave"
    );

    // 5. Build and submit transaction
    let tx_builder = match SuiTransactionBuilder::new(state.config.clone()).await {
        Ok(builder) => builder,
        Err(err) => {
            tracing::error!("Failed to create transaction builder: {:?}", err);
            return Ok(Json(LinkWalletResponse::failure(format!(
                "Failed to initialize: {}",
                err
            ))));
        }
    };

    let xid = String::from_utf8_lossy(&signed_payload.response.data.xid).to_string();
    let owner_address = format!(
        "0x{}",
        hex::encode(signed_payload.response.data.owner_address)
    );

    let tx_result = tx_builder
        .link_wallet(
            &xid,
            &owner_address,
            signed_payload.response.timestamp_ms,
            &signed_payload.signature,
        )
        .await;

    match tx_result {
        Ok(digest) => {
            tracing::info!("Link wallet transaction successful: {}", digest);
            Ok(Json(LinkWalletResponse::success(digest)))
        }
        Err(err) => {
            tracing::error!("Link wallet transaction failed: {:?}", err);
            Ok(Json(LinkWalletResponse::failure(format!(
                "Transaction failed: {}",
                err
            ))))
        }
    }
}

/// Helper endpoint to generate the message that should be signed by the wallet
/// This is called by the Dapp to get the correct message format
#[derive(Debug, Deserialize)]
pub struct GenerateLinkMessageRequest {
    pub xid: String, // Twitter user ID (from access token verification on frontend)
    pub wallet_address: String, // Sui wallet address
}

#[derive(Debug, Serialize)]
pub struct GenerateLinkMessageResponse {
    pub message: String,
    pub timestamp: u64,
}

pub async fn generate_link_message(
    Json(request): Json<GenerateLinkMessageRequest>,
) -> Json<GenerateLinkMessageResponse> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let message = format!(
        "Link XID:{} to wallet {} at {}",
        request.xid, request.wallet_address, timestamp
    );

    Json(GenerateLinkMessageResponse { message, timestamp })
}

// ====== Transaction History API ======

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionResponse {
    pub tx_digest: String,
    pub tx_type: String,
    pub from_xid: Option<String>,
    pub to_xid: Option<String>,
    pub coin_type: String,
    pub amount: String,   // Amount in SUI (converted from MIST)
    pub amount_mist: i64, // Raw amount in MIST
    pub tweet_id: Option<String>,
    pub timestamp: i64,
    pub created_at: String,
}

impl TransactionResponse {
    /// Convert Transfer to TransactionResponse with the given decimals
    fn from_transfer_with_decimals(tx: Transfer, decimals: u8) -> Self {
        let divisor = 10f64.powi(decimals as i32);
        let formatted_amount = tx.amount as f64 / divisor;

        let tx_type = match tx.transfer_type {
            TransferType::Transfer => "transfer",
            TransferType::Deposit => "deposit",
            TransferType::Withdraw => "withdraw",
        };
        Self {
            tx_digest: tx.transaction_digest,
            tx_type: tx_type.to_string(),
            from_xid: tx.from_xid,
            to_xid: tx.to_xid,
            coin_type: tx.coin_type,
            amount: format!("{:.9}", formatted_amount)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string(),
            amount_mist: tx.amount,
            tweet_id: tx.tweet_id,
            timestamp: tx.timestamp,
            created_at: tx.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TransactionQuery {
    pub limit: Option<i64>,
    pub page: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedTransactionsResponse {
    pub data: Vec<TransactionResponse>,
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    pub total_pages: i64,
}

/// Get transaction history by sui_object_id with pagination
pub async fn get_transactions_by_account(
    State(state): State<Arc<AppState>>,
    Path(sui_object_id): Path<String>,
    Query(query): Query<TransactionQuery>,
) -> Result<Json<PaginatedTransactionsResponse>, StatusCode> {
    let limit = query.limit.unwrap_or(5).min(100); // Default 5, max 100
    let page = query.page.unwrap_or(1).max(1); // Default page 1
    let offset = (page - 1) * limit;

    // Get total count
    let total = match Transfer::count_by_sui_object_id(&state.db, &sui_object_id).await {
        Ok(count) => count,
        Err(err) => {
            tracing::error!("Failed to count transactions: {:?}", err);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get paginated transfers
    let transfers =
        match Transfer::find_by_sui_object_id_paginated(&state.db, &sui_object_id, limit, offset)
            .await
        {
            Ok(transfers) => transfers,
            Err(err) => {
                tracing::error!("Failed to query transactions: {:?}", err);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };

    // Collect unique coin types
    let unique_coin_types: std::collections::HashSet<String> =
        transfers.iter().map(|t| t.coin_type.clone()).collect();

    // Fetch decimals for each coin type from Sui RPC
    let sui_client = dugong_core::clients::sui_client::SuiClient::new(&state.config.sui_rpc_url);
    let mut decimals_map: std::collections::HashMap<String, u8> = std::collections::HashMap::new();
    for coin_type in unique_coin_types {
        // Ensure coin_type has 0x prefix for RPC query
        let query_coin_type = if coin_type.starts_with("0x") {
            coin_type.clone()
        } else {
            format!("0x{}", coin_type)
        };
        let decimals = match sui_client.get_coin_metadata(&query_coin_type).await {
            Ok(Some(metadata)) => metadata.decimals,
            Ok(None) => {
                tracing::debug!(
                    "No metadata found for coin type: {}, defaulting to 9 decimals",
                    coin_type
                );
                9 // Default to 9 decimals if metadata not found
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to fetch metadata for coin type {}: {:?}, defaulting to 9 decimals",
                    coin_type,
                    err
                );
                9 // Default to 9 decimals on error
            }
        };
        decimals_map.insert(coin_type, decimals);
    }

    // Convert transfers to responses with correct decimals
    let data: Vec<TransactionResponse> = transfers
        .into_iter()
        .map(|t| {
            let decimals = *decimals_map.get(&t.coin_type).unwrap_or(&9);
            TransactionResponse::from_transfer_with_decimals(t, decimals)
        })
        .collect();

    let total_pages = (total as f64 / limit as f64).ceil() as i64;

    Ok(Json(PaginatedTransactionsResponse {
        data,
        total,
        page,
        limit,
        total_pages,
    }))
}

/// Get one transaction by transaction digest
pub async fn get_transaction_by_digest(
    State(state): State<Arc<AppState>>,
    Path(tx_digest): Path<String>,
) -> Result<Json<TransactionResponse>, StatusCode> {
    let transfer = match Transfer::find_by_transaction_digest(&state.db, &tx_digest).await {
        Ok(Some(transfer)) => transfer,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(err) => {
            tracing::error!("Failed to query transaction by digest: {:?}", err);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let query_coin_type = if transfer.coin_type.starts_with("0x") {
        transfer.coin_type.clone()
    } else {
        format!("0x{}", transfer.coin_type)
    };
    let sui_client = dugong_core::clients::sui_client::SuiClient::new(&state.config.sui_rpc_url);
    let decimals = match sui_client.get_coin_metadata(&query_coin_type).await {
        Ok(Some(metadata)) => metadata.decimals,
        Ok(None) | Err(_) => {
            if transfer.coin_type.ends_with("::usdc::USDC") {
                6
            } else {
                9
            }
        }
    };

    Ok(Json(TransactionResponse::from_transfer_with_decimals(
        transfer, decimals,
    )))
}

/// Serve a crawler-friendly transaction card page for Twitter link preview.
pub async fn get_transaction_card(
    State(state): State<Arc<AppState>>,
    Path(tx_digest): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let transfer = match Transfer::find_by_transaction_digest(&state.db, &tx_digest).await {
        Ok(Some(transfer)) => transfer,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(err) => {
            tracing::error!("Failed to query transaction by digest: {:?}", err);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let query_coin_type = if transfer.coin_type.starts_with("0x") {
        transfer.coin_type.clone()
    } else {
        format!("0x{}", transfer.coin_type)
    };
    let sui_client = dugong_core::clients::sui_client::SuiClient::new(&state.config.sui_rpc_url);
    let decimals = match sui_client.get_coin_metadata(&query_coin_type).await {
        Ok(Some(metadata)) => metadata.decimals,
        Ok(None) | Err(_) => {
            if transfer.coin_type.ends_with("::usdc::USDC") {
                6
            } else {
                9
            }
        }
    };

    Ok(Html(build_tx_card_html(&transfer, decimals)))
}

/// Token balance info
#[derive(Debug, Serialize)]
pub struct TokenBalance {
    pub symbol: String,
    pub coin_type: String,
    pub balance_raw: i64,
    pub balance_formatted: String,
    pub decimals: u8,
}

/// Get account balance by sui_object_id
pub async fn get_account_balance(
    State(state): State<Arc<AppState>>,
    Path(sui_object_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // First get the account to find x_user_id
    let account = match DugongAccount::find_by_sui_object_id(&state.db, &sui_object_id).await {
        Ok(Some(acc)) => acc,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(err) => {
            tracing::error!("Failed to find account: {:?}", err);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Query all balances from account_balances table
    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"
        SELECT coin_type, COALESCE(balance, 0)
        FROM account_balances
        WHERE x_user_id = $1
        "#,
    )
    .bind(&account.x_user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    // Build token balances list
    let mut balances: Vec<TokenBalance> = Vec::new();
    let mut sui_balance: i64 = 0;

    for (coin_type, balance) in rows {
        if coin_type.ends_with("::sui::SUI") {
            sui_balance = balance;
        };
        let (symbol, decimals) = coin_display_metadata(&coin_type);

        let divisor = 10f64.powi(decimals as i32);
        let formatted = format!("{:.9}", balance as f64 / divisor)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();

        balances.push(TokenBalance {
            symbol,
            coin_type,
            balance_raw: balance,
            balance_formatted: formatted,
            decimals,
        });
    }

    // Ensure SUI is always first if present
    balances.sort_by(|a, b| {
        if a.symbol == "SUI" {
            std::cmp::Ordering::Less
        } else if b.symbol == "SUI" {
            std::cmp::Ordering::Greater
        } else {
            a.symbol.cmp(&b.symbol)
        }
    });

    Ok(Json(serde_json::json!({
        // Legacy fields for backward compatibility
        "balance_mist": sui_balance,
        "balance_sui": format!("{:.9}", sui_balance as f64 / 1_000_000_000.0).trim_end_matches('0').trim_end_matches('.').to_string(),
        // New multi-token fields
        "balances": balances,
        "x_user_id": account.x_user_id,
        "sui_object_id": sui_object_id,
    })))
}

// ====== X OAuth 2.0 Authentication API ======

/// Request to exchange OAuth code for token
#[derive(Debug, Deserialize)]
pub struct TokenExchangeRequest {
    pub code: String,
    pub code_verifier: String,
    pub redirect_uri: String,
}

/// Dugong account info in auth response
#[derive(Debug, Serialize)]
pub struct DugongAccountInfo {
    pub sui_object_id: String,
    pub x_user_id: String,
    pub x_handle: String,
    pub owner_address: Option<String>,
}

impl From<DugongAccount> for DugongAccountInfo {
    fn from(account: DugongAccount) -> Self {
        Self {
            sui_object_id: account.sui_object_id,
            x_user_id: account.x_user_id,
            x_handle: account.x_handle,
            owner_address: account.owner_address,
        }
    }
}

/// Auth response after successful token exchange
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub user: TwitterUserInfo,
    #[serde(rename = "accessToken")]
    pub access_token: String,
    /// Backend session token. The SPA stores this and sends it as
    /// `Authorization: Bearer <sessionToken>` on endpoints that act on the user's
    /// behalf (e.g. wallet linking), where it — not the Twitter token — is the proof
    /// of identity.
    #[serde(rename = "sessionToken")]
    pub session_token: String,
    #[serde(rename = "dugongAccount")]
    pub dugong_account: Option<DugongAccountInfo>,
    #[serde(
        rename = "createdAccountTxDigest",
        skip_serializing_if = "Option::is_none"
    )]
    pub created_account_tx_digest: Option<String>,
}

/// Error response for auth failures
#[derive(Debug, Serialize)]
pub struct AuthErrorResponse {
    pub error: String,
}

/// Exchange OAuth code for access token and get user info
///
/// Flow:
/// 1. Exchange code for access_token with Twitter OAuth 2.0 API
/// 2. Get user info from Twitter
/// 3. Look up existing Dugong account (if any)
/// 4. Return user info + dugong account
pub async fn exchange_twitter_token(
    State(state): State<Arc<AppState>>,
    Json(request): Json<TokenExchangeRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    tracing::info!("Token exchange request received");

    // 1. Create OAuth2 client and exchange code
    let oauth2_client =
        TwitterOAuth2Client::with_base_url(&state.config, state.config.twitter_api_base.clone());

    let token_response = oauth2_client
        .exchange_code(&request.code, &request.code_verifier, &request.redirect_uri)
        .await
        .map_err(|err| {
            tracing::error!("Token exchange failed: {:?}", err);
            (
                StatusCode::BAD_REQUEST,
                Json(AuthErrorResponse {
                    error: format!("Token exchange failed: {}", err),
                }),
            )
        })?;

    // 2. Get user info from Twitter
    let user_info = oauth2_client
        .get_user_info(&token_response.access_token)
        .await
        .map_err(|err| {
            tracing::error!("Failed to get user info: {:?}", err);
            (
                StatusCode::BAD_REQUEST,
                Json(AuthErrorResponse {
                    error: format!("Failed to get user info: {}", err),
                }),
            )
        })?;

    tracing::info!(
        x_user_id = %user_info.id,
        username = %user_info.username,
        "User authenticated successfully"
    );

    // 2b. Persist the refresh token (encrypted) so the backend can mint fresh
    //     access tokens later (e.g. for wallet linking) without forcing re-login.
    //     A storage failure must not block login — the user can still re-auth if a
    //     later action finds no stored token.
    if let Err(err) = store_oauth_tokens(&state, &user_info.id, &token_response).await {
        tracing::error!(
            x_user_id = %user_info.id,
            "Failed to persist Twitter refresh token: {err:?}"
        );
    }

    // 2c. Issue the backend session token that authenticates this user on our own
    //     endpoints. This is required to act on the user's behalf, so fail if it
    //     cannot be issued (a misconfiguration caught at startup by
    //     `ensure_token_security`, so this should not happen in practice).
    let session_token = issue_session(&state.config, &user_info.id).map_err(|err| {
        tracing::error!("Failed to issue session token: {err:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthErrorResponse {
                error: "Failed to establish session".to_string(),
            }),
        )
    })?;

    // 3. Look up existing Dugong account
    let dugong_account = DugongAccount::find_by_x_user_id(&state.db, &user_info.id)
        .await
        .map_err(|err| {
            tracing::error!("Database error looking up account: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AuthErrorResponse {
                    error: "Database error".to_string(),
                }),
            )
        })?
        .map(DugongAccountInfo::from);

    if dugong_account.is_some() {
        tracing::info!(
            x_user_id = %user_info.id,
            "Found existing Dugong account"
        );
    } else {
        tracing::info!(
            x_user_id = %user_info.id,
            "No Dugong account found - user needs to create one"
        );
    }

    // 4. Return auth response
    Ok(Json(AuthResponse {
        user: user_info,
        access_token: token_response.access_token,
        session_token,
        dugong_account,
        created_account_tx_digest: None,
    }))
}

/// Ensure-account request for already-authenticated dapp sessions.
#[derive(Debug, Deserialize)]
pub struct EnsureDugongAccountRequest {
    pub access_token: String,
}

/// Find the Dugong account for `xid`, or auto-initialize one on-chain via the Nautilus enclave.
///
/// Idempotent: returns an existing account without submitting a transaction. When no account
/// exists it signs an `init_account` intent in the enclave (whose response carries the canonical
/// xid + handle), submits the transaction, then polls until the indexer mirrors the new
/// `dugong_accounts` row — the on-chain `AccountCreated` event is not visible the instant
/// `init_account` returns. Returns the account and, when one was created, the init tx digest.
///
/// Shared by the `/api/auth/twitter/ensure-account` handler and the tweet-triggered recipient
/// auto-creation in the processor worker, so both paths create accounts the same way.
pub(crate) async fn ensure_dugong_account_for_xid(
    state: &Arc<AppState>,
    enclave: &EnclaveClient,
    xid: &str,
    handle: Option<&str>,
) -> anyhow::Result<(DugongAccountInfo, Option<String>)> {
    use anyhow::Context;

    if let Some(account) = DugongAccount::find_by_x_user_id(&state.db, xid)
        .await
        .context("Failed to look up Dugong account")?
    {
        tracing::info!(xid = %xid, "Found existing Dugong account");
        return Ok((account.into(), None));
    }

    tracing::info!(xid = %xid, "No Dugong account found; auto-initializing via Nautilus enclave");

    let signed = enclave
        .sign_init_account(xid, handle)
        .await
        .context("Failed to sign init account")?;

    let signed_xid = String::from_utf8(signed.response.data.xid.clone())
        .context("Invalid xid encoding from enclave")?;
    let handle = String::from_utf8(signed.response.data.handle.clone())
        .context("Invalid handle encoding from enclave")?;

    tracing::info!(
        xid = %signed_xid,
        handle = %handle,
        timestamp = signed.response.timestamp_ms,
        "Submitting auto-created account initialization to Sui with enclave signature"
    );

    let tx_builder = SuiTransactionBuilder::new(state.config.clone())
        .await
        .context("Failed to initialize Sui transaction builder")?;

    let digest = tx_builder
        .init_account(
            &signed_xid,
            &handle,
            signed.response.timestamp_ms,
            &signed.signature,
        )
        .await
        .context("Failed to submit auto-created account init transaction")?;

    tracing::info!(
        tx_digest = %digest,
        xid = %xid,
        "Account init submitted; waiting for the indexer to mirror it"
    );

    // The `dugong_accounts` row is written by the indexer from the on-chain `AccountCreated`
    // event, so it is NOT visible the instant `init_account` returns. Poll until the indexer
    // mirrors it (bounded) so this function has "ensure account exists" semantics.
    const POLL_INTERVAL: Duration = Duration::from_millis(1500);
    const MAX_POLLS: u32 = 20; // ~30s — comfortably over the indexer poll interval
    for attempt in 1..=MAX_POLLS {
        if let Some(account) = DugongAccount::find_by_x_user_id(&state.db, xid)
            .await
            .context("Failed to poll for auto-created account")?
        {
            tracing::info!(xid = %xid, attempt, "Auto-created account mirrored by indexer");
            return Ok((account.into(), Some(digest)));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    anyhow::bail!(
        "Auto-created account for xid {} (init tx {}) was not mirrored by the indexer within \
         {:?}; the init_account transaction landed but the indexer has not caught up (is it \
         running?)",
        xid,
        digest,
        POLL_INTERVAL * MAX_POLLS
    )
}

/// Verify an existing X access token and ensure the matching Dugong account exists.
///
/// Used by the dapp on already-authenticated sessions, where the OAuth callback does not run
/// again but the account may still need to be initialized.
pub async fn ensure_dugong_account(
    State(state): State<Arc<AppState>>,
    Json(request): Json<EnsureDugongAccountRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let oauth2_client =
        TwitterOAuth2Client::with_base_url(&state.config, state.config.twitter_api_base.clone());

    let user_info = oauth2_client
        .get_user_info(&request.access_token)
        .await
        .map_err(|err| {
            tracing::error!(
                "Failed to verify access token for ensure-account: {:?}",
                err
            );
            (
                StatusCode::UNAUTHORIZED,
                Json(AuthErrorResponse {
                    error: format!("Failed to verify access token: {}", err),
                }),
            )
        })?;

    tracing::info!(
        x_user_id = %user_info.id,
        username = %user_info.username,
        "Ensure-account request for authenticated X session"
    );

    let enclave = EnclaveClient::new(state.config.enclave_url.clone());

    let (dugong_account, created_account_tx_digest) =
        ensure_dugong_account_for_xid(&state, &enclave, &user_info.id, Some(&user_info.username))
            .await
            .map_err(|err| {
                tracing::error!(
                    x_user_id = %user_info.id,
                    "Failed to ensure Dugong account: {:?}",
                    err
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(AuthErrorResponse {
                        error: format!("Failed to ensure Dugong account: {}", err),
                    }),
                )
            })?;

    // Issue a fresh backend session token for the already-authenticated user, so
    // dapp sessions restored on reload (no OAuth callback) can still link wallets.
    let session_token = issue_session(&state.config, &user_info.id).map_err(|err| {
        tracing::error!("Failed to issue session token: {err:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthErrorResponse {
                error: "Failed to establish session".to_string(),
            }),
        )
    })?;

    Ok(Json(AuthResponse {
        user: user_info,
        access_token: request.access_token,
        session_token,
        dugong_account: Some(dugong_account),
        created_account_tx_digest,
    }))
}

// ====== Transaction Sponsorship API (Enoki) ======

/// Request body for sponsoring a transaction
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SponsorTxRequest {
    pub network: String,  // "mainnet" | "testnet"
    pub tx_bytes: String, // base64 encoded transaction kind bytes
    pub sender: String,   // Sui address
    #[serde(default)]
    pub allowed_addresses: Vec<String>, // Optional: allowed addresses for execution
}

/// Response for sponsored transaction creation
#[derive(Debug, Serialize)]
pub struct SponsorTxResponse {
    pub bytes: String,  // base64 encoded sponsored transaction bytes
    pub digest: String, // transaction digest
}

/// Error response for sponsorship failures
#[derive(Debug, Serialize)]
pub struct SponsorErrorResponse {
    pub error: String,
}

/// Create a sponsored transaction using Enoki
///
/// This endpoint receives transaction kind bytes from the frontend,
/// sponsors them using Enoki, and returns the sponsored transaction
/// bytes + digest for signing.
pub async fn sponsor_transaction(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SponsorTxRequest>,
) -> Result<Json<SponsorTxResponse>, (StatusCode, Json<SponsorErrorResponse>)> {
    tracing::info!(
        sender = %request.sender,
        network = %request.network,
        allowed_addresses = ?request.allowed_addresses,
        "Sponsor transaction request received"
    );

    // Create Enoki client
    let enoki_client = EnokiClient::with_base_url(
        state.config.enoki_api_key.clone(),
        request.network.clone(),
        state.config.enoki_base_url.clone(),
    );

    // Create sponsored transaction
    match enoki_client
        .create_sponsored_transaction(request.tx_bytes, request.sender, request.allowed_addresses)
        .await
    {
        Ok(response) => {
            tracing::info!(digest = %response.digest, "Transaction sponsored successfully");
            Ok(Json(SponsorTxResponse {
                bytes: response.bytes,
                digest: response.digest,
            }))
        }
        Err(err) => {
            tracing::error!("Failed to sponsor transaction: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SponsorErrorResponse {
                    error: format!("Could not create sponsored transaction: {}", err),
                }),
            ))
        }
    }
}

/// Request body for executing a sponsored transaction
#[derive(Debug, Deserialize)]
pub struct ExecuteSponsoredTxRequest {
    pub digest: String,    // Transaction digest from sponsor response
    pub signature: String, // User's signature (base64)
}

/// Response for sponsored transaction execution
#[derive(Debug, Serialize)]
pub struct ExecuteSponsoredTxResponse {
    pub digest: String, // Final transaction digest
}

/// Execute a sponsored transaction using Enoki
///
/// After the user signs the sponsored transaction bytes,
/// this endpoint submits the signature to Enoki which
/// executes the transaction on-chain.
pub async fn execute_sponsored_transaction(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ExecuteSponsoredTxRequest>,
) -> Result<Json<ExecuteSponsoredTxResponse>, (StatusCode, Json<SponsorErrorResponse>)> {
    tracing::info!(
        digest = %request.digest,
        "Execute sponsored transaction request received"
    );

    // Create Enoki client - use configured network
    let enoki_client = EnokiClient::with_base_url(
        state.config.enoki_api_key.clone(),
        state.config.enoki_network.clone(),
        state.config.enoki_base_url.clone(),
    );

    // Execute sponsored transaction
    match enoki_client
        .execute_sponsored_transaction(request.digest, request.signature)
        .await
    {
        Ok(response) => {
            tracing::info!(digest = %response.digest, "Sponsored transaction executed successfully");
            Ok(Json(ExecuteSponsoredTxResponse {
                digest: response.digest,
            }))
        }
        Err(err) => {
            tracing::error!("Failed to execute sponsored transaction: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SponsorErrorResponse {
                    error: format!("Could not execute sponsored transaction: {}", err),
                }),
            ))
        }
    }
}
