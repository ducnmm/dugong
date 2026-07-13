//! Shared Twitter OAuth 2.0 token storage and refresh, used by both the API's
//! user-login path and the processor's bot reply-posting path.
//!
//! Tokens live encrypted at rest in `twitter_oauth_tokens` (keyed by X user id).
//! Twitter **rotates** refresh tokens: every successful refresh returns a new
//! refresh token that MUST replace the stored one, which is why minting always
//! persists the response. A definitively-dead refresh token is deleted so it is
//! not retried.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::clients::twitter::{OAuth2TokenResponse, RefreshError, TwitterOAuth2Client};
use crate::config::Config;
use crate::db::models::TwitterOAuthToken;

/// A freshly minted access token plus its absolute expiry (when known), so
/// callers can cache it and avoid refreshing on every request.
#[derive(Debug, Clone)]
pub struct MintedAccessToken {
    pub access_token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Why a fresh access token could not be minted.
#[derive(Debug)]
pub enum MintError {
    /// The stored refresh token is missing/unreadable/rejected — the account
    /// must be re-authorized (for users: re-login; for the bot: rerun
    /// `dugong-bot-authorize`).
    ReauthRequired(String),
    /// A server-side/transient problem unrelated to the stored credential.
    Transient(String),
}

impl std::fmt::Display for MintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MintError::ReauthRequired(msg) => write!(f, "re-authorization required: {msg}"),
            MintError::Transient(msg) => write!(f, "transient token error: {msg}"),
        }
    }
}

impl std::error::Error for MintError {}

/// Persist OAuth credentials (encrypted at rest) for `xid`. No-op when the
/// response carries no refresh token (e.g. `offline.access` not granted).
///
/// Takes the raw 32-byte encryption key rather than a [`Config`] so it can be
/// called from lightweight, out-of-process helpers (e.g. `dugong-bot-authorize`)
/// that do not build a full server config.
pub async fn store_tokens(
    pool: &PgPool,
    key: &[u8; 32],
    xid: &str,
    tokens: &OAuth2TokenResponse,
) -> anyhow::Result<()> {
    let Some(refresh) = tokens.refresh_token.as_deref() else {
        return Ok(());
    };
    let refresh_enc = crate::crypto::seal(key, refresh)?;
    let access_enc = crate::crypto::seal(key, &tokens.access_token)?;
    let expires_at = tokens
        .expires_in
        .map(|s| Utc::now() + chrono::Duration::seconds(s as i64));
    TwitterOAuthToken::upsert(
        pool,
        xid,
        &refresh_enc,
        Some(&access_enc),
        expires_at,
        tokens.scope.as_deref(),
    )
    .await?;
    Ok(())
}

/// Mint a fresh access token for `xid` from the stored refresh token, persisting
/// the rotated refresh token. On a definitively-dead token the stored row is
/// deleted so it is not retried.
pub async fn mint_fresh_access_token(
    pool: &PgPool,
    config: &Config,
    xid: &str,
) -> Result<MintedAccessToken, MintError> {
    let key = config
        .token_encryption_key()
        .map_err(|e| MintError::Transient(e.to_string()))?;

    let stored = TwitterOAuthToken::find_by_x_user_id(pool, xid)
        .await
        .map_err(|e| MintError::Transient(format!("db error: {e}")))?
        .ok_or_else(|| MintError::ReauthRequired("no stored X session for this id".to_string()))?;

    let refresh = crate::crypto::open(key, &stored.refresh_token_enc)
        .map_err(|_| MintError::ReauthRequired("stored X credential unreadable".to_string()))?;

    let oauth = TwitterOAuth2Client::with_base_url(config, config.twitter_api_base.clone());
    match oauth.refresh_access_token(&refresh).await {
        Ok(resp) => {
            let expires_at = resp
                .expires_in
                .map(|s| Utc::now() + chrono::Duration::seconds(s as i64));
            // Twitter rotates the refresh token — persist the new one. A persist
            // failure is non-fatal for THIS request (we still return the fresh
            // access token), but is logged since the rotated refresh token is now
            // the only valid one going forward.
            if let Err(err) = store_tokens(pool, key, xid, &resp).await {
                tracing::warn!("failed to persist rotated Twitter token for {xid}: {err:?}");
            }
            Ok(MintedAccessToken {
                access_token: resp.access_token,
                expires_at,
            })
        }
        Err(RefreshError::ReauthRequired(msg)) => {
            let _ = TwitterOAuthToken::delete(pool, xid).await;
            Err(MintError::ReauthRequired(msg))
        }
        Err(RefreshError::Transient(err)) => Err(MintError::Transient(err.to_string())),
    }
}
