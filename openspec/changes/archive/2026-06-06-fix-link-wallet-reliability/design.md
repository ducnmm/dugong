## Context

`POST /api/link-wallet/submit` is the dapp endpoint that links a Sui wallet to a user's X (Twitter) identity. Today it carries the user's Twitter OAuth2 **access token** from the browser; the API forwards it to the Nautilus enclave, which calls Twitter `GET /2/users/me` to derive the xid, verifies the wallet signature over a message (`Link XID:{xid} to wallet {addr} at {ts}`), and returns an enclave-signed `LinkWalletPayload` that the API submits on-chain.

Two production failures motivated this change:
- The browser caches the access token in `localStorage` and reuses it indefinitely; Twitter user tokens expire in ~2h, so the enclave's Twitter call returns `401` and linking fails. `offline.access` is granted and `refresh_token` is parsed but discarded.
- `EnclaveClient` (`apps/core/src/clients/enclave.rs`) is a bare `reqwest::Client::new()` with no timeout/retry, so any transient enclave unavailability becomes a hard `Connection refused` failure.

Current building blocks: Postgres via `sqlx`, plain numbered SQL migrations under `apps/core/migrations/`, `dugong_accounts` keyed by `x_user_id` (no token columns, and rows may not exist at OAuth time), `TwitterOAuth2Client` with `exchange_code`/`get_user_info` (no refresh grant), and **no encryption-at-rest or session utilities anywhere in the codebase**.

## Goals / Non-Goals

**Goals:**
- Wallet linking succeeds regardless of how long ago the user logged in, without a forced re-login on the happy path.
- Refresh tokens are stored encrypted at rest and never exposed to the client or logs.
- The flow remains secure: only the verified owner of an X account can link a wallet to it.
- Transient enclave unavailability is absorbed (timeout + bounded retry) rather than surfaced as a hard failure.

**Non-Goals:**
- A general-purpose session/auth framework. We add the minimum trusted-identity mechanism this flow needs.
- Fixing `POST /api/auth/twitter/ensure-account` (same staleness class) — flagged follow-up.
- Changing the enclave's verification logic or the on-chain `link_wallet` transaction.
- Refresh-token revocation UI / token management screens.

## Decisions

### D1 — Store refresh tokens in a dedicated table, not on `dugong_accounts`
A new `twitter_oauth_tokens` table keyed by `x_user_id` (PK), with `refresh_token_enc`, optional `access_token_enc`, `expires_at`, `scope`, `created_at`, `updated_at`. Migration `005_*.sql`; upsert on `x_user_id`.
- **Why:** OAuth completes before a Dugong account may exist ("user needs to create one"), so tokens cannot depend on a `dugong_accounts` row. A separate table also keeps a sensitive credential isolated from frequently-read account data.
- **Alternatives:** Columns on `dugong_accounts` (rejected: lifecycle mismatch, leaks secrets into hot reads). Redis-only (rejected: needs durability across restarts).

### D2 — App-level AES-256-GCM encryption with a key from env
Add a small `apps/core` helper: `encrypt(plaintext) -> (nonce || ciphertext)` and `decrypt(...)` using AES-256-GCM (`aes-gcm` crate), key from `TOKEN_ENCRYPTION_KEY` (32 bytes, base64/hex). Random 96-bit nonce per encryption, stored as a prefix of the stored blob.
- **Why:** No KMS in the stack; app-level AEAD is portable across Railway/local, simple, and keeps ciphertext opaque to the DB. GCM gives integrity (tamper-evident).
- **Alternatives:** Postgres `pgcrypto` (rejected: key ends up in SQL/logs, ties crypto to DB). Cloud KMS (rejected: infra not present). Plaintext (rejected: refresh tokens are long-lived credentials).
- **Key handling:** fail fast at startup if `TOKEN_ENCRYPTION_KEY` is missing/wrong length. Document generation (`openssl rand -base64 32`) and a rotation note (decrypt-with-old/encrypt-with-new is out of scope; for now rotation invalidates stored tokens → users re-login).

### D3 — Authenticate the caller with a backend-signed session token (security-critical)
At `exchange_twitter_token`, after verifying the X user, issue a stateless **session token** — a JWT (or compact signed token) containing `{ xid, exp }`, signed with `SESSION_TOKEN_SECRET` (HMAC-SHA256). Return it to the SPA. `secure_link_wallet` requires this token, verifies the signature/expiry, and derives a **trusted** xid from it. The stored refresh token for that xid is then used to mint a fresh access token for the enclave.
- **Why this is required, not optional:** An expired access token proves nothing. If we looked up a refresh token purely by a client-supplied xid, an attacker could submit a valid signature over `Link XID:{victim} to wallet {attacker_wallet} ...` and have the server mint the victim's token — linking the attacker's wallet to the victim's X account (and potentially their custodial funds). The session token re-establishes "this caller owns xid" without a live Twitter call.
- **Why stateless JWT:** no session table/Redis needed; verification is a signature check. Bound lifetime (e.g. matches/repeats the OAuth login cadence).
- **Alternatives considered:**
  - *Client holds the refresh token and sends it* — rejected: puts a long-lived credential in the browser, strictly worse than today.
  - *Server-side sessions in Redis/DB (revocable)* — viable and more revocable, but adds storage + lookup; deferred (see Open Questions). The stateless token can be swapped for this later without changing the enclave contract.
  - *Frontend just-in-time re-auth (the "re-auth on demand" option)* — rejected by product decision (extra redirect each link).
- **Defense in depth:** the enclave still independently re-derives xid from the freshly-minted token and rejects the request unless it matches the xid embedded in the signed message — so a forged/garbled session token cannot by itself produce a valid link.

### D4 — Refresh-before-enclave in `secure_link_wallet`
Flow becomes: authenticate session → load `refresh_token_enc` for xid → `TwitterOAuth2Client::refresh_access_token` → forward the **fresh** access token to the enclave. Twitter **rotates** the refresh token on each refresh, so the response's new `refresh_token` (when present) is persisted back (upsert) before proceeding.
- **Why:** centralizes freshness on the server; the browser never needs a valid Twitter token for this call.
- **Failure mapping:** no stored token, refresh returns `invalid_grant`, or refresh transport error → respond with a distinct, machine-readable "session expired, re-login" result so the SPA can route to re-auth (vs. a generic failure).

### D5 — `EnclaveClient` timeouts + bounded retry
Build the client via `reqwest::Client::builder()` with `connect_timeout(~5s)` and `timeout(~30s)` (the enclave call includes a Twitter round-trip, so the total budget is generous). Wrap `post` in up to 3 attempts with exponential backoff (e.g. 200ms → 400ms → 800ms), retrying **only** connection/transport errors (and optionally `502/503/504`), never on a clean 4xx/business error.
- **Why:** enclave operations only verify-and-sign and are effectively idempotent, so retrying transport failures is safe and also protects the worker's `process_tweet` path. Timeouts prevent unbounded hangs now that no request can wait forever.
- **Alternatives:** infinite/zero retry (rejected); a retry crate (optional — a hand-rolled loop avoids a new dependency).

## Risks / Trade-offs

- **Encryption key management** → If `TOKEN_ENCRYPTION_KEY` is lost/rotated, stored refresh tokens become undecryptable. Mitigation: fail-fast validation, document generation, and treat rotation as "users re-login" (acceptable: a fresh login re-stores tokens).
- **Session-token theft** → A stolen session token authorizes linking for that xid. Mitigation: short-ish expiry, HTTPS only, and the enclave's independent xid/message check (D3 defense-in-depth). If revocability becomes a requirement, switch D3 to server-side sessions.
- **Retrying non-idempotent work** → Mitigation: retry only transport/connection errors and explicitly-safe 5xx, never after a response was produced. Document that enclave ops are verify-and-sign only.
- **Twitter refresh-token rotation race** → Two concurrent link attempts could both refresh, invalidating one rotated token. Mitigation: link is a rare, user-initiated action; acceptable. An upsert on the latest rotated token converges; a failed refresh maps to re-login.
- **Wider blast radius than the original one-line bug** → This adds a table, a crate, and two secrets. Mitigation: net new code is additive (new table/helpers); existing happy path unchanged except where the fresh token is sourced.

## Migration Plan

1. Add `aes-gcm` (and a JWT/HMAC helper) to `apps/core`; add `005_*.sql` (additive table, no backfill).
2. Set `TOKEN_ENCRYPTION_KEY` and `SESSION_TOKEN_SECRET` in Railway (`api`, plus `*-dev`) and local `.env` **before** deploying the code that reads them.
3. Deploy backend: `exchange_twitter_token` starts persisting refresh tokens + issuing session tokens; `secure_link_wallet` reads them.
4. Deploy frontend: store/send the session token; handle the re-login signal.
5. **Backward compatibility:** users who logged in before this ships have no stored refresh token and no session token → their next link returns the re-login signal, they re-auth once, and are fixed. No data backfill needed.
6. **Rollback:** revert the frontend and backend; the new table and secrets can remain unused (harmless). No destructive schema change to undo.

## Open Questions

- **Session strategy:** stateless JWT (chosen for minimal scope) vs. revocable server-side sessions (Redis/DB). Confirm whether link-wallet needs revocation now or can adopt it later. The enclave contract is unaffected either way.
- **Session token transport:** `Authorization: Bearer` from the SPA (simplest given the current fetch code) vs. httpOnly cookie (better XSS posture, needs CORS/credentials wiring). Default: Bearer to match existing calls; revisit.
- **Session lifetime & re-issue:** fixed TTL vs. sliding; and whether `ensure-account` should also issue/refresh the session when it adopts this path.
- **Key/secret rotation procedure:** do we need dual-key decrypt for `TOKEN_ENCRYPTION_KEY` rotation, or is "rotation = re-login" acceptable long-term?
