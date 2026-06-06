## Why

Wallet linking (`POST /api/link-wallet/submit`) fails in production for two distinct, confirmed reasons:

1. **Expired Twitter token (the user-facing 401).** The enclave verifies the user by calling Twitter `GET /2/users/me` with the OAuth2 access token the browser sends. That token is minted at login, cached in `localStorage`, and reused indefinitely — but Twitter user tokens expire in ~2h. After expiry the enclave gets `401 Unauthorized` and linking fails. We already request `offline.access` and already parse the `refresh_token`, but `exchange_twitter_token` throws it away, so nothing can refresh.
2. **Non-resilient enclave client (the `request failed` / `Connection refused` error).** `EnclaveClient` uses a bare `reqwest::Client::new()` with no timeout and no retry, so any transient enclave unavailability (cold boot, restart, redeploy) surfaces immediately as a hard failure with no recovery.

The infrastructure trigger for #2 (Railway Serverless putting the enclave to sleep) has already been fixed in production by disabling sleep. This change fixes the **code** so both failure modes stop recurring.

## What Changes

- **Persist Twitter OAuth refresh tokens server-side, encrypted at rest.** Introduce an AES-256-GCM helper in `apps/core` keyed off a new `TOKEN_ENCRYPTION_KEY` env var, and a new `twitter_oauth_tokens` table (new migration `005_*.sql`) keyed by `x_user_id`.
- **Capture the refresh token at login.** `exchange_twitter_token` upserts the (encrypted) refresh token after the code exchange instead of discarding it.
- **Add a refresh grant to the OAuth client.** New `TwitterOAuth2Client::refresh_access_token(refresh_token)` (`grant_type=refresh_token`). Twitter rotates the refresh token on each use, so the rotated token is persisted back.
- **Authenticate the caller's identity server-side (security-critical).** Because an expired access token no longer proves anything, the API must independently know *which* xid the caller owns before using that xid's stored refresh token — otherwise anyone could link their own wallet to someone else's X account. `exchange_twitter_token` issues a **backend-signed session token (JWT bound to the xid)** after verifying the user; `secure_link_wallet` authenticates via that session token to derive a trusted xid. The raw Twitter access token is no longer the authorization proof for our own endpoints.
- **Mint a fresh token before the enclave call.** `secure_link_wallet` resolves the trusted xid from the session token, looks up that user's stored refresh token, mints a fresh access token, and forwards **that** to the enclave — never the browser's possibly-stale token. The xid the enclave re-derives from the fresh token must match the xid in the signed message.
- **Graceful expiry UX.** When no valid stored refresh token exists / refresh fails, return a clear "X session expired — please re-login" signal; the frontend surfaces it and prompts re-auth instead of showing a generic failure.
- **Harden the enclave client.** Give `EnclaveClient` a connect timeout + request timeout and a small retry-with-backoff for transport errors. The enclave operations only verify-and-sign (idempotent), so retries are safe; this also protects the worker's `process_tweet` path.

Out of scope (flagged follow-up): `POST /api/auth/twitter/ensure-account` takes a client-supplied access token and has the same staleness risk; it can adopt the same refresh path later.

## Capabilities

### New Capabilities
- `twitter-token-refresh`: Server-side storage (encrypted at rest) and refreshing of Twitter OAuth 2.0 tokens — capture at code-exchange, issuing a backend-signed session token bound to the xid, refresh-grant with rotation handling, and minting a fresh access token for an authenticated xid on demand.
- `secure-wallet-linking`: The `/api/link-wallet/submit` flow authenticates the caller via the backend session token, obtains a freshly-minted Twitter token before contacting the enclave, and returns a clear re-authentication signal (instead of a generic failure) when the user has no valid stored token.
- `enclave-client-resilience`: The API/worker enclave HTTP client applies connect/request timeouts and bounded retry-with-backoff on transport errors so transient enclave unavailability does not hard-fail user actions.

### Modified Capabilities
<!-- None. twitter-auth (ensure-account) requirements are unchanged in this change; the matching staleness fix there is a flagged follow-up. -->

## Impact

- **Code:**
  - `apps/core/src/clients/twitter.rs` — add `refresh_access_token`; rotation handling.
  - `apps/core/src/clients/enclave.rs` — timeouts + retry/backoff on `EnclaveClient`.
  - `apps/core/src/db/models.rs` — new token storage model (encrypted) keyed by `x_user_id`.
  - `apps/core/migrations/005_*.sql` — new `twitter_oauth_tokens` table.
  - `apps/core/src` — new AES-256-GCM encryption helper.
  - `apps/api/src/routes.rs` — `exchange_twitter_token` persists refresh token + issues a session token; `secure_link_wallet` authenticates the session, refreshes-before-enclave, and returns a re-login signal on expiry.
  - `apps/core/src` — new session-token (JWT) sign/verify helper bound to xid.
  - `apps/web/src/contexts/AuthContext.tsx`, `apps/web/src/hooks/useLinkWallet.ts` (+ minor UI) — store/send the backend session token; handle the re-login signal.
- **Config / ops:** new `TOKEN_ENCRYPTION_KEY` (refresh-token encryption) and `SESSION_TOKEN_SECRET` (session JWT signing) secrets must be set in every environment (Railway `api`, plus dev). Document generation and rotation.
- **Dependencies:** add an AEAD crate (e.g. `aes-gcm`) to `apps/core`.
- **Data:** new table only; no changes to existing tables. Existing users simply have no stored refresh token until their next login (handled by the re-login path).
- **Security:** refresh tokens are long-lived credentials — stored encrypted, never logged, never returned to the client.
