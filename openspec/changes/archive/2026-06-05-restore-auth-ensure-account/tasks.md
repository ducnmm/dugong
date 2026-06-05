## 1. Extract the shared auto-init helper

- [x] 1.1 In `apps/api/src/routes.rs`, add `pub(crate) async fn ensure_dugong_account_for_xid(state: &Arc<AppState>, enclave: &EnclaveClient, xid: &str) -> anyhow::Result<(DugongAccountInfo, Option<String>)>` containing: find-by-xid short-circuit → `enclave.sign_init_account(xid)` → decode `xid`/`handle` → `SuiTransactionBuilder::new(state.config.clone()).init_account(...)` → bounded poll on `find_by_x_user_id` until mirrored → return `(info, Some(digest))`. Port the poll constants/error message from the current `auto_create_recipient_account`.
- [x] 1.2 Build `DugongAccountInfo` from the `DugongAccount` row in one place (reuse the existing field mapping used by `exchange_twitter_token`).
- [x] 1.3 In `apps/api/src/processor/worker.rs`, replace the body of `auto_create_recipient_account` with a thin delegation to `crate::routes::ensure_dugong_account_for_xid(&self.state, &self.enclave, to_xid)` (discard the return). Remove the now-duplicated sign/init/poll code.
- [x] 1.4 Confirm the worker's callers still compile and behave the same (return `()` on success).

## 2. Restore the response field

- [x] 2.1 In `apps/api/src/routes.rs`, add `created_account_tx_digest: Option<String>` to `AuthResponse` with `#[serde(rename = "createdAccountTxDigest", skip_serializing_if = "Option::is_none")]`.
- [x] 2.2 Update `exchange_twitter_token` to construct `AuthResponse { …, created_account_tx_digest: None }`.

## 3. Add the ensure-account handler

- [x] 3.1 Add `#[derive(Debug, Deserialize)] pub struct EnsureDugongAccountRequest { pub access_token: String }` in `apps/api/src/routes.rs`.
- [x] 3.2 Add `pub async fn ensure_dugong_account(State(state): State<Arc<AppState>>, Json(req): Json<EnsureDugongAccountRequest>) -> Result<Json<AuthResponse>, (StatusCode, Json<AuthErrorResponse>)>`: verify token via `TwitterOAuth2Client::…get_user_info` (map failure → `401`), build `EnclaveClient::new(&state.config.enclave_url)`, call `ensure_dugong_account_for_xid` (map `anyhow::Error` → `500`), return `AuthResponse` with `dugong_account: Some(info)` and the digest.

## 4. Wire the route

- [x] 4.1 In `apps/api/src/lib.rs::build_router`, register `.route("/api/auth/twitter/ensure-account", axum::routing::post(routes::ensure_dugong_account))` adjacent to the existing `/api/auth/twitter/token` route.

## 5. Tests

- [x] 5.1 In `apps/api/tests/routes.rs`, add a test: seed a `DugongAccount`, mock `/2/users/me` to return that X user id, POST `/api/auth/twitter/ensure-account` → assert `200`, `dugongAccount.sui_object_id` matches the seeded row, and `createdAccountTxDigest` is absent.
- [x] 5.2 Add a test: mock `/2/users/me` to return `401`, POST a token → assert the endpoint responds `401` and submits no transaction.

## 6. Validation

- [x] 6.1 `cargo build -p dugong-api` compiles clean (no dead-code warnings from the removed worker block).
- [x] 6.2 `cargo test -p dugong-api --test routes` passes the new + existing route tests.
- [x] 6.3 Manually confirm the route is reachable: a `curl` POST to `/api/auth/twitter/ensure-account` with an invalid token returns `401` (not `404`).
