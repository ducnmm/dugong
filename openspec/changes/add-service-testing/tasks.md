## 1. Test infrastructure setup

- [x] 1.1 Add `wiremock` and `serde_json` to `[dev-dependencies]` in the workspace `Cargo.toml` (as workspace dev-deps) and reference them from each crate that needs them
- [x] 1.2 Enable the `test` feature on `sqlx` for dev (so `#[sqlx::test]` is available) in `core`, `api`, `indexer` — already satisfied by existing `macros`+`migrate` features; use `#[sqlx::test(migrations = "...")]`
- [x] 1.3 Add testability refactor: give each reqwest client constructor a configurable base URL defaulting to its production const — `sui_client.rs`/`enclave.rs` already configurable; added `with_base_url` to `enoki.rs` and both clients in `twitter.rs`; worker clients handled in group 5
- [x] 1.4 Document local test prerequisites (local Postgres + `DATABASE_URL`) in `docs/local-dev-guide.md`

## 2. core crate tests

- [x] 2.1 Add `apps/core/tests/common/mod.rs` with wiremock server helper and sample JSON fixtures
- [x] 2.2 Add `apps/core/tests/clients.rs`: wiremock-backed tests for sui_client, enoki, enclave, twitter happy-path + error-path parsing
- [x] 2.3 Add `apps/core/tests/db_models.rs`: `#[sqlx::test]` round-trip tests for `DugongAccount`, `AccountBalance`, `WebhookEvent`, `Market`, `MarketBet`
- [x] 2.4 Verify `cargo test -p dugong-core` passes against a local Postgres — 22 tests pass (11 clients + 8 db_models + 3 existing)

## 3. api crate tests

- [x] 3.1 Populate `apps/api/tests/common/mod.rs` with an Axum app builder (`build_router` extracted to lib), test config with mock base URLs, Redis helper + shared-queue lock, and a `sqlx::test` pool helper
- [x] 3.2 Add `apps/api/tests/webhook.rs`: CRC challenge response, dedup, and enqueue behavior via `tower::ServiceExt` (signature unit-tested in `unit_tests.rs`)
- [x] 3.3 Add `apps/api/tests/routes.rs`: account lookup, wallet link (enclave-failure path), OAuth token exchange, sponsor/execute routes against the test DB + mocked HTTP
- [x] 3.4 Add `apps/api/tests/processor.rs`: `process_once` early-exit paths (empty/missing/already-done) asserting `ProcessOutcome`. NOTE: full per-`CommandType` dispatch submits real Sui txns via `SuiTransactionBuilder` and isn't wiremock-mockable without a Sui-client trait refactor — left to the live stack
- [x] 3.5 Verify `cargo test -p dugong-api` passes — 19 tests pass (3 processor + 7 routes + 4 webhook + 5 unit)

## 4. indexer crate tests

- [x] 4.1 Promote indexer modules to a `lib.rs` (`pub mod`) so handlers/cursor/fetcher are reachable from `tests/`; `main.rs` now wraps the lib
- [x] 4.2 Add `apps/indexer/tests/common/mod.rs` with a `SuiEvent` JSON fixture builder + a `test_config` for the fetcher
- [x] 4.3 Add `apps/indexer/tests/handlers.rs`: `#[sqlx::test]` tests for each event handler (account_created, bet_placed, coin_deposited/withdrawn/transferred, handle_updated, market_created/resolved, wallet_linked) + missing-parsed_json error path
- [x] 4.4 Add `apps/indexer/tests/cursor.rs` and `apps/indexer/tests/event_fetcher.rs` (wiremock) tests
- [x] 4.5 Verify `cargo test -p dugong-indexer` passes — 12 tests pass (9 handlers + 1 cursor + 2 fetcher)

## 5. worker crate tests

- [x] 5.1 Promote worker modules to a `lib.rs` so `poller`, `twitter_client`, `backend_client` are reachable from `tests/`; also added `TwitterClient::with_base_url` (the deferred task-1.3 worker refactor) and extracted a pure `tweets_to_events`
- [x] 5.2 Add `apps/worker/tests/clients.rs`: wiremock tests for `TwitterClient` search (parse, since_id filter, HTTP error) and `BackendClient` send/health
- [x] 5.3 Add `apps/worker/tests/poller.rs`: `tweets_to_events` conversion tests (pairing, missing-author drop, empty)
- [x] 5.4 Verify `cargo test -p dugong-worker` passes — 8 tests pass (5 clients + 3 poller)

## 6. tools crate tests

- [x] 6.1 Extract the login HTTP logic from `src/bin/login.rs` into a small testable function/module (`dugong_tools::login::fetch_login_cookie`)
- [x] 6.2 Add `apps/tools/tests/login.rs`: wiremock test for the login flow + cookie validation
- [x] 6.3 Verify `cargo test -p dugong-tools` passes — 5 tests pass (success, guest-session reject, error status, HTTP failure, missing cookie)

## 7. nautilus-server tests

- [x] 7.1 Add `apps/nautilus-server/tests/handlers.rs`: HTTP-handler tests for `/process_tweet`, `/process_init_account`, `/process_secure_link_wallet` with mocked TwitterAPI.io (wiremock) and a test keypair. Threaded configurable base URLs through `AppState` + added `build_router`; tests boot the router on an ephemeral port and drive it over real HTTP (including a real Sui-style ed25519 wallet signature)
- [x] 7.2 Verify `cargo test -p nautilus-server` passes — 21 tests pass (6 handler + 15 existing BCS/crypto/parse)

## 8. database-free compilation check

<!-- Adjusted during apply: codebase uses runtime sqlx queries, not query! macros,
     so no .sqlx/ offline cache or SQLX_OFFLINE is needed. DB is only required at
     test runtime for #[sqlx::test]. -->

- [x] 8.1 Confirm the workspace compiles with no `DATABASE_URL` set (`cargo build --workspace`) — builds clean (runtime sqlx queries, no compile-time DB)
- [x] 8.2 Document that `#[sqlx::test]` tests require a running Postgres via `DATABASE_URL` (in `docs/local-dev-guide.md` Testing section, plus a Redis note for api tests)

## 9. web app tests

- [x] 9.1 Add dev-deps to `apps/web/package.json`: `vitest`, `jsdom`, `@testing-library/react` (>=16), `@testing-library/jest-dom`, `@testing-library/user-event`
- [x] 9.2 Add a `test` config (vitest block in `vite.config.ts`) with `environment: jsdom` and a setup file (`src/test/setup.ts`) importing `@testing-library/jest-dom/vitest`
- [x] 9.3 Add `test` and `test:run` scripts to `apps/web/package.json`
- [x] 9.4 Add `src/utils/pkce.test.ts` and `src/utils/api.test.ts`
- [x] 9.5 Add a hook test, `src/hooks/useXAuth.test.tsx`, with mocked fetch (success, CSRF-state mismatch, backend error)
- [x] 9.6 Verify `pnpm test:run` passes in `apps/web` — 21 tests pass across 3 files

## 10. web E2E (Playwright)

- [x] 10.1 Add `@playwright/test` to `apps/web` dev-deps and install Chromium (`pnpm exec playwright install chromium`)
- [x] 10.2 Add `playwright.config.ts` with a `webServer` running `pnpm build && vite preview` (port 4173) and `baseURL`, Chromium project
- [x] 10.3 Add `test:e2e` (and `test:e2e:ui`) scripts to `apps/web/package.json`
- [x] 10.4 Add `e2e/fixtures.ts` with `page.route()` helpers that mock backend API responses (account search, balance, transactions, OAuth token exchange) + localStorage/sessionStorage seeders
- [x] 10.5 Add `e2e/navigation.spec.ts`: home renders, search → result → navigate to account view, unknown-route redirect
- [x] 10.6 Add `e2e/oauth-callback.spec.ts`: callback with mocked token exchange routes to dashboard; CSRF-state + missing-params error paths
- [x] 10.7 Add `e2e/dashboard.spec.ts`: dashboard shows mocked account + balance; unauthenticated redirect
- [x] 10.8 Verify `pnpm test:e2e` passes locally — 8 tests pass (Chromium)

## 11. CI workflow

- [x] 11.1 Add `.github/workflows/test.yml` with a `rust` job (Postgres + Redis services, `cargo test --workspace --locked`, `DATABASE_URL`/`REDIS_URL` set) and a `web` job (`pnpm install` + `pnpm test:run`). NOTE: `SQLX_OFFLINE` is intentionally omitted — the codebase uses runtime sqlx queries (no `query!` macros), so there is no offline cache to gate on
- [x] 11.2 In the `web` job, install Playwright browsers and run `pnpm test:e2e`; upload the Playwright HTML report as an artifact on failure
- [x] 11.3 Ensure the `rust` job sets `DATABASE_URL` to the Postgres service so `#[sqlx::test]` runtime tests connect (plus `REDIS_URL` for the api Redis tests)
- [ ] 11.4 Push a branch and confirm the workflow runs green on PR — awaiting user confirmation to push (outward-facing action)
