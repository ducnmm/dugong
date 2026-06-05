use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::webhook::handler::AppState;
use dugong_core::clients::enclave::EnclaveClient;
use dugong_core::clients::enoki::EnokiClient;
use dugong_core::clients::sui_transaction::SuiTransactionBuilder;
use dugong_core::clients::twitter::{TwitterOAuth2Client, TwitterUserInfo};
use dugong_core::db::models::{AccountBalance, DugongAccount, Transfer, TransferType};

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

/// Request to link wallet with Twitter access token and wallet signature
#[derive(Debug, Deserialize)]
pub struct SecureLinkWalletApiRequest {
    pub access_token: String,     // Twitter OAuth2 access token
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
}

/// Secure link wallet endpoint
///
/// Flow:
/// 1. Receive request from Dapp with access_token + wallet_signature
/// 2. Forward to Nautilus enclave for verification and signing
/// 3. Submit link_wallet transaction to Sui blockchain
/// 4. Return transaction digest
pub async fn secure_link_wallet(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SecureLinkWalletApiRequest>,
) -> Result<Json<LinkWalletResponse>, StatusCode> {
    tracing::info!(
        "Secure link wallet request for address: {}",
        request.wallet_address
    );

    // 1. Create enclave client and get signed payload
    let enclave_client = EnclaveClient::new(&state.config.enclave_url);

    let signed_result = enclave_client
        .sign_secure_link_wallet(
            &request.access_token,
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
            return Ok(Json(LinkWalletResponse {
                success: false,
                tx_digest: None,
                error: Some(format!("Verification failed: {}", err)),
            }));
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

    // 2. Build and submit transaction
    let tx_builder = match SuiTransactionBuilder::new(state.config.clone()).await {
        Ok(builder) => builder,
        Err(err) => {
            tracing::error!("Failed to create transaction builder: {:?}", err);
            return Ok(Json(LinkWalletResponse {
                success: false,
                tx_digest: None,
                error: Some(format!("Failed to initialize: {}", err)),
            }));
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
            Ok(Json(LinkWalletResponse {
                success: true,
                tx_digest: Some(digest),
                error: None,
            }))
        }
        Err(err) => {
            tracing::error!("Link wallet transaction failed: {:?}", err);
            Ok(Json(LinkWalletResponse {
                success: false,
                tx_digest: None,
                error: Some(format!("Transaction failed: {}", err)),
            }))
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
        let (symbol, decimals) = if coin_type.ends_with("::sui::SUI") {
            sui_balance = balance;
            ("SUI".to_string(), 9u8)
        } else if coin_type.ends_with("::wal::WAL") {
            ("WAL".to_string(), 9u8)
        } else if coin_type.ends_with("::usdc::USDC") {
            ("USDC".to_string(), 6u8)
        } else {
            // Unknown token - extract symbol from coin_type
            let symbol = coin_type
                .split("::")
                .last()
                .unwrap_or("UNKNOWN")
                .to_string();
            (symbol, 9u8) // Default to 9 decimals for unknown tokens
        };

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
        .sign_init_account(xid)
        .await
        .context("Failed to sign init account")?;

    let signed_xid =
        String::from_utf8(signed.response.data.xid.clone()).context("Invalid xid encoding from enclave")?;
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
            tracing::error!("Failed to verify access token for ensure-account: {:?}", err);
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
        ensure_dugong_account_for_xid(&state, &enclave, &user_info.id)
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

    Ok(Json(AuthResponse {
        user: user_info,
        access_token: request.access_token,
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
