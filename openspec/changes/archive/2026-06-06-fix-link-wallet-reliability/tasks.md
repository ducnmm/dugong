## 1. Foundation (core: crypto, session, config)

- [x] 1.1 Add `aes-gcm` (and a JWT/HMAC dependency, e.g. `jsonwebtoken` or `hmac`+`sha2`) to `apps/core/Cargo.toml` and the workspace.
- [x] 1.2 Add `TOKEN_ENCRYPTION_KEY` and `SESSION_TOKEN_SECRET` to `Config` (`apps/core/src/config.rs`), failing fast at startup if missing or wrong length.
- [x] 1.3 Implement an AES-256-GCM helper in `apps/core` (`encrypt`/`decrypt`) with a random 96-bit nonce stored as a prefix on the ciphertext; never log plaintext.
- [x] 1.4 Implement a session-token helper in `apps/core`: `issue(xid, ttl) -> token` and `verify(token) -> xid`, signed with `SESSION_TOKEN_SECRET`, rejecting expired/malformed/wrong-key tokens.
- [x] 1.5 Unit tests: encrypt→decrypt round-trip, tamper rejection, key-length validation; session issue→verify, expiry rejection, wrong-key rejection.

## 2. Database (token storage)

- [x] 2.1 Add migration `apps/core/migrations/005_twitter_oauth_tokens.sql` creating `twitter_oauth_tokens` (`x_user_id` PK, `refresh_token_enc` BYTEA/TEXT, nullable `access_token_enc`, `expires_at`, `scope`, `created_at`, `updated_at`).
- [x] 2.2 Add a model in `apps/core/src/db/models.rs`: `upsert_refresh_token(x_user_id, enc)`, `find_by_x_user_id(x_user_id)`, `delete`/invalidate — storing/reading only encrypted values.
- [x] 2.3 Integration test (sqlx) for upsert + lookup of an encrypted token row.

## 3. OAuth client: refresh grant

- [x] 3.1 Add `TwitterOAuth2Client::refresh_access_token(refresh_token)` in `apps/core/src/clients/twitter.rs`: POST `{api_base}/2/oauth2/token` with `grant_type=refresh_token` and Basic client credentials; parse into `OAuth2TokenResponse`.
- [x] 3.2 Handle refresh-token **rotation**: return both the fresh access token and the (possibly new) refresh token so callers can persist the rotated value.
- [x] 3.3 Map Twitter `invalid_grant`/4xx to a distinct "re-authentication required" error variant (vs. transient errors).
- [x] 3.4 Tests against a mock token endpoint: success, rotation, `invalid_grant`.

## 4. API: capture at login, refresh before enclave

- [x] 4.1 In `exchange_twitter_token` (`apps/api/src/routes.rs`): after verifying the user, upsert the encrypted refresh token by `x_user_id` and issue a session token; include the session token in `AuthResponse`. Stop relying on returning the raw access token as the authorization proof.
- [x] 4.2 Add a shared "mint fresh access token for a trusted xid" routine (loads stored refresh token → `refresh_access_token` → persists rotated token → returns fresh access token; surfaces "re-auth required" when absent/invalid).
- [x] 4.3 In `secure_link_wallet` (`apps/api/src/routes.rs`): authenticate via the session token to derive a trusted `x_user_id`; reject unauthenticated requests.
- [x] 4.4 Reject the request if the xid in the signed link message differs from the trusted session xid.
- [x] 4.5 Mint a fresh access token (4.2) for the trusted xid and forward **that** to `EnclaveClient::sign_secure_link_wallet`; never forward the browser's token.
- [x] 4.6 Map "re-authentication required" to a distinct, machine-readable response field/status the frontend can detect (not a generic failure); never log token values.
- [x] 4.7 API tests: link succeeds with an expired client token but valid session + stored refresh token; unauthenticated rejected; xid mismatch rejected; missing/invalid refresh token returns the re-auth signal.

## 5. Enclave client resilience

- [x] 5.1 Build `EnclaveClient`'s `reqwest::Client` via `builder()` with `connect_timeout` (~5s) and `timeout` (~30s) in `apps/core/src/clients/enclave.rs`.
- [x] 5.2 Wrap `post` in a bounded retry loop (≤3 attempts, exponential backoff) that retries only connection/transport errors (and optionally `502/503/504`), never a clean 4xx/business response.
- [x] 5.3 Tests: transient connection error then success retries through; `400` business error is not retried; retries are bounded and surface a transport error when exhausted.

## 6. Frontend

- [x] 6.1 Store the backend session token at login (`apps/web/src/contexts/AuthContext.tsx`) and send it on `/api/link-wallet/submit` (and stop depending on the raw Twitter access token for that call) in `apps/web/src/hooks/useLinkWallet.ts`.
- [x] 6.2 Detect the "re-authentication required" response and route the user to re-login (reuse the X OAuth flow), with a clear message; verify a subsequent link succeeds.

## 7. Config, ops, and docs

- [ ] 7.1 Generate and set `TOKEN_ENCRYPTION_KEY` and `SESSION_TOKEN_SECRET` on Railway `api` (production) and `api-dev`, and in local `.env`/`.env.example`.
- [x] 7.2 Document key generation (`openssl rand -base64 32`) and the rotation policy ("rotation invalidates stored tokens → users re-login") in the deployment docs.
- [x] 7.3 Update `scripts/railway-set-env.ts` (if it manages env) to include the new secrets.

## 8. Verification

- [x] 8.1 Run the full workspace test suite and `openspec validate fix-link-wallet-reliability --strict`.
- [ ] 8.2 Manual end-to-end on a deployed env: log in, wait past token expiry (or simulate), link a wallet → succeeds without re-login; then force the re-auth path and confirm the prompt + recovery.
- [x] 8.3 Confirm no token/credential values appear in logs.
