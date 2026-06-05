## Context

`apps/api` is an Axum binary. Routing is built in `apps/api/src/lib.rs::build_router` (extracted
from `main.rs` for testability). HTTP handlers live in `apps/api/src/routes.rs`; the tweet
processor lives in `apps/api/src/processor/worker.rs`. Shared state is
`AppState { config: Config, db: PgPool, redis: RedisClient }` (`apps/api/src/webhook/handler.rs`).

Account auto-init exists today **only** in the worker:
`WorkerProcessor::auto_create_recipient_account(&self, to_xid)` →
`enclave.sign_init_account(to_xid)` (the enclave response returns both `xid` and `handle`) →
`SuiTransactionBuilder::new(config).init_account(xid, handle, ts, sig)` → poll
`DugongAccount::find_by_x_user_id` until the indexer mirrors the on-chain `AccountCreated` event
(bounded: 20 × 1.5s ≈ 30s). It returns `()`.

The web dapp expects `POST /api/auth/twitter/ensure-account` to return a fully-formed account
(`EnsureDugongAccountResponse.dugongAccount` is non-optional;
`apps/web/src/pages/Dashboard.tsx` bails unless present). The route and its handler were deleted
in the `f5b1bf7` refactor and never re-added to `build_router`.

`SuiTransactionBuilder::init_account` returns **only the tx digest** — not the created object id.
The `sui_object_id` is only knowable once the indexer mirrors the account into `dugong_accounts`.
So a synchronous handler must poll-until-mirrored exactly like the worker already does; there is
no cheap "read object id from tx effects" path in the current code.

## Goals / Non-Goals

**Goals:**
- Restore `POST /api/auth/twitter/ensure-account` with the response shape the web client already
  consumes — zero frontend change.
- One shared "ensure account for xid" helper used by both the worker (tweet-triggered) and the
  new handler (auth-triggered). Single source of truth.
- Reuse the proven worker path, including the indexer-mirror poll (the `986ada0` fix).

**Non-Goals:**
- Re-adding auto-init to `exchange_twitter_token` (`/api/auth/twitter/token`) — it stays a passive
  lookup; `ensure-account` is the single auth-side creation gate.
- Restoring codex's on-chain reconcile (`upsert_registered_account`) / `wait_for_registered_account`
  / `sign_init_account_with_handle` — absent from the current tree; the worker doesn't reconcile
  either, so it is out of scope for this regression fix.
- Async "kickoff + poll" endpoint (return pending, frontend polls). Better worst-case latency but
  requires frontend rework for a one-time, first-login-only event. Deferred.

## Decisions

### Decision: One shared helper, returning the account
Factor the worker's auto-init into:

```rust
// apps/api/src/routes.rs  (co-located with the handler and DugongAccountInfo it returns)
pub(crate) async fn ensure_dugong_account_for_xid(
    state: &Arc<AppState>,
    enclave: &EnclaveClient,
    xid: &str,
) -> anyhow::Result<(DugongAccountInfo, Option<String>)> // (account, created_tx_digest)
```

Flow:
1. `DugongAccount::find_by_x_user_id(&state.db, xid)` → `Some` ⇒ return `(info, None)` (idempotent
   short-circuit; also covers repeat logins cheaply).
2. `enclave.sign_init_account(xid)` → decode `xid` + `handle` from the signed response.
3. `SuiTransactionBuilder::new(state.config.clone()).init_account(xid, handle, ts, sig)` → digest.
4. Poll `find_by_x_user_id` (20 × 1.5s) until the indexer mirrors it ⇒ return `(info, Some(digest))`;
   bail with the existing "not mirrored within …" error if it never appears.

`auto_create_recipient_account` becomes a thin wrapper:
```rust
async fn auto_create_recipient_account(&self, to_xid: &str) -> Result<()> {
    ensure_dugong_account_for_xid(&self.state, &self.enclave, to_xid).await?;
    Ok(())
}
```

**Why co-locate in `routes.rs` rather than a new module:** the helper returns `DugongAccountInfo`
(defined in `routes.rs`) and is consumed by the handler right beside it. Keeping it there is the
smallest correct change and avoids moving the type. The worker imports
`crate::routes::ensure_dugong_account_for_xid` — acceptable intra-crate coupling in a single
binary. A dedicated `account_init` module was considered but deferred as churn without payoff.

**Why return `anyhow::Result`:** matches the worker's idiom. The HTTP handler maps the error to
`(StatusCode::INTERNAL_SERVER_ERROR, Json(AuthErrorResponse))`; token-verification failures are a
separate `UNAUTHORIZED` mapping handled in the handler before the helper runs.

### Decision: Handler verifies the token, then ensures
```rust
pub async fn ensure_dugong_account(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EnsureDugongAccountRequest>, // { access_token: String }
) -> Result<Json<AuthResponse>, (StatusCode, Json<AuthErrorResponse>)>
```
1. `TwitterOAuth2Client::…(&state.config).get_user_info(&req.access_token)` — failure ⇒ `401`.
2. Build `EnclaveClient::new(&state.config.enclave_url)`.
3. `ensure_dugong_account_for_xid(&state, &enclave, &user_info.id)` — failure ⇒ `500`.
4. Return `AuthResponse { user, access_token, dugong_account: Some(info), created_account_tx_digest }`.

### Decision: Restore `created_account_tx_digest` on `AuthResponse`
Add `#[serde(rename = "createdAccountTxDigest", skip_serializing_if = "Option::is_none")]
pub created_account_tx_digest: Option<String>`. `exchange_twitter_token` constructs it as `None`.
Matches the optional `createdAccountTxDigest` in the web `EnsureDugongAccountResponse`.

### Decision: Synchronous (blocking) is acceptable
First-ever creation blocks on the indexer-mirror poll (~30s ceiling, usually seconds). It only
happens on a user's first account; later logins short-circuit at step 1. The dapp already renders
`isEnsuringOwnAccount` as a loading state. Async kickoff is deferred (see Non-Goals).

## Risks / Trade-offs

- **Latency on first login** — bounded by the poll (≈30s worst case). Mitigated by the existing
  loading UI and the fact that it is one-time per user. If it proves painful, revisit async kickoff.
- **No on-chain reconcile** — if an account exists on-chain but not in the DB (indexer down/behind),
  the helper will attempt a second `init_account` that may fail. Pre-existing behavior in the
  worker; explicitly out of scope. The bounded poll surfaces a clear error rather than hanging.
- **Worker behavior change** — adding the find-first short-circuit makes `auto_create_recipient_account`
  idempotent. This is strictly safer than today's unconditional sign+init and does not change the
  contract for its callers (they re-read the account afterward).
