-- ============================================================================
-- Twitter OAuth 2.0 token storage
-- ============================================================================
-- Stores per-user Twitter OAuth credentials so the backend can mint a fresh
-- access token on demand (e.g. for wallet linking) instead of trusting the
-- browser's possibly-expired token.
--
-- Credentials are ENCRYPTED AT REST (AES-256-GCM; see apps/core/src/crypto.rs):
-- the *_enc columns hold base64(nonce || ciphertext||tag), never plaintext.
--
-- Keyed by X user id (xid). A row may exist BEFORE a dugong_accounts row (OAuth
-- completes before account init), so this table is intentionally independent of
-- dugong_accounts (no FK).
CREATE TABLE IF NOT EXISTS twitter_oauth_tokens (
    x_user_id         VARCHAR(64) PRIMARY KEY,    -- xid; matches dugong_accounts.x_user_id
    refresh_token_enc TEXT        NOT NULL,        -- encrypted Twitter refresh token
    access_token_enc  TEXT,                        -- optional encrypted access token
    expires_at        TIMESTAMPTZ,                 -- access-token expiry, if known
    scope             TEXT,                        -- granted OAuth scopes
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
