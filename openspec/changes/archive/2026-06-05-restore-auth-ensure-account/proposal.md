## Why

The web dapp calls `POST /api/auth/twitter/ensure-account` on every dashboard load where the
signed-in X user has no Dugong account yet (`apps/web/src/pages/Dashboard.tsx`,
`ensureDugongAccount` in `apps/web/src/utils/api.ts`). That route **no longer exists on the
backend**: it was dropped when routing moved from `apps/api/src/main.rs` into
`apps/api/src/lib.rs::build_router` during the `f5b1bf7 "Refactor codebase to use dugong-core
library"` refactor. The handler (`ensure_dugong_account`) and its shared auto-init helper
(`find_or_auto_init_dugong_account`) were deleted with the old `apps/api/src/api.rs`.

As a result, the dapp's session-resume account-creation path returns **404** in production. The
only surviving auto-init path is tweet-triggered, inside the processor worker
(`auto_create_recipient_account`). A user who authenticates with X via the web app but has never
been referenced by a tweet command can never get an account through the UI.

The working logic still exists on `codex/twitter-polymarket-flow`. Rather than re-port the dead
codex helpers (`upsert_registered_account`, `wait_for_registered_account`,
`sign_init_account_with_handle` — none of which survive in the current tree), we restore the
endpoint on top of the **already-proven** worker auto-init path, factored into one shared helper.

## What Changes

- Extract the worker's account auto-init logic (`WorkerProcessor::auto_create_recipient_account`,
  which today returns `()` and polls the indexer-mirrored DB until the account exists — the
  `986ada0` "mirror before use" fix) into a **shared helper that returns `DugongAccountInfo`**.
- Re-point `auto_create_recipient_account` at the shared helper so the tweet-triggered path and
  the auth-triggered path share **one** "ensure an account exists for an xid" brain.
- Re-add the `ensure_dugong_account` handler in `apps/api/src/routes.rs`: verify the supplied X
  access token via `get_user_info`, then call the shared helper to find-or-create the account.
- Register `POST /api/auth/twitter/ensure-account` in `apps/api/src/lib.rs::build_router`.
- Keep the response shape the web client already expects
  (`EnsureDugongAccountResponse`: `user`, `accessToken`, `dugongAccount`, optional
  `createdAccountTxDigest`).

Explicit non-changes:
- `exchange_twitter_token` (`/api/auth/twitter/token`) stays a passive lookup; `ensure-account`
  is the single account-creation gate for the auth flow.
- The on-chain reconcile guard (codex's `upsert_registered_account`, for "exists on-chain but
  missing from DB") is **not** restored here — the current worker doesn't have it either, so it
  is a pre-existing gap, not part of this regression fix.

## Capabilities

### New Capabilities
- `twitter-auth`: X (Twitter) OAuth-session account assurance — verify an authenticated X access
  token and guarantee the matching custodial Dugong account exists, auto-initializing it on-chain
  via the Nautilus enclave when absent.

### Modified Capabilities
<!-- None: no existing spec defines the X auth / account-ensure behavior. -->

## Impact

- **Code**:
  - `apps/api/src/routes.rs` — add `EnsureDugongAccountRequest`, `ensure_dugong_account` handler;
    make `AuthResponse` carry the optional `created_account_tx_digest`.
  - `apps/api/src/lib.rs` — register the `ensure-account` route in `build_router`.
  - `apps/api/src/processor/worker.rs` (or a shared module under `apps/api/src` /
    `apps/core`) — extract the auto-init helper to return `DugongAccountInfo`; `auto_create_recipient_account` delegates to it.
- **API contract**: restores `POST /api/auth/twitter/ensure-account` (previously 404). Response
  matches the existing web `EnsureDugongAccountResponse` type — no frontend change required.
- **Dependencies**: none added; reuses `TwitterOAuth2Client`, `EnclaveClient::sign_init_account`,
  `SuiTransactionBuilder::init_account`, `DugongAccount::find_by_x_user_id`.
- **Operations**: first-ever creation for a user blocks the HTTP call until the indexer mirrors
  the new account (bounded poll, ~30s ceiling, typically a few seconds). Repeat logins
  short-circuit on the DB lookup. The dapp already renders an `isEnsuringOwnAccount` loading state.
