## Why

The prediction-market (Polymarket-style) lifecycle and the existing "send money"
transfer flow are the two highest-value bot commands, yet there is no single way to
exercise them end-to-end against a running stack. Today contributors either post
tweets by hand and re-run `process_tweet_url.sh` per tweet, or rely on Rust unit
tests that stop at the enclave boundary and never touch Sui. A scripted, repeatable
smoke test would catch regressions across the full `webhook → enclave → Sui → reply`
path before they reach deploys.

## What Changes

- Add a **scripted end-to-end test runner** (`scripts/test-flows.ts`, run via `tsx`)
  that drives the bot's two flagship flows against a locally running stack:
  - **Transfer / send money**: `send <amt> <coin> to @<user>`.
  - **Prediction-market lifecycle**: `create market: <question>` → `bet <amt> <coin>
    on yes|no` (one or more bettors) → `resolve yes|no`.
- The runner **posts real command tweets** through the same TwitterAPI.io posting
  path the processor uses (reusing the `dugong-test-tweet` posting logic), threading
  bets/resolutions as replies to the market tweet so parent-tweet lookup works.
- For each posted tweet it **triggers the API `/webhook`** (the same payload shape as
  `process_tweet_url.sh`) and then **polls `webhook_events`** in Postgres for terminal
  status, surfacing `tx_digest` / `error_message`.
- Emits a **pass/fail summary** per step (account init, transfer, market created, each
  bet escrowed, market resolved + payout) with non-zero exit on failure, so it can run
  in CI or locally.
- Add an **npm script** (`pnpm test-flows` in `scripts/package.json`) and a short
  **README/usage section** documenting required env vars and the local-stack
  prerequisite.
- Configurable via env/flags: backend URL, coin type, bet amounts, dry-run (skip real
  tweet posting and inject synthetic webhook payloads), and which flow(s) to run.

## Capabilities

### New Capabilities
- `e2e-command-test-script`: a repeatable script that exercises the bot's transfer and
  prediction-market command lifecycles against a running stack and reports per-step
  pass/fail with on-chain tx results.

### Modified Capabilities
<!-- None. This adds tooling only; it does not change bot command behavior, transfer
     semantics, or the prediction-market spec. Existing testing capabilities
     (rust-service-testing, test-ci) are unaffected. -->

## Impact

- **New file** `scripts/test-flows.ts`: the runner (TypeScript, executed with `tsx`,
  matching the existing `scripts/*.ts` convention).
- **`scripts/package.json`**: add a `test-flows` script entry; reuse existing `tsx`
  dev dependency.
- **Reuses** the `process_tweet_url.sh` webhook payload shape and the `dugong-test-tweet`
  TwitterAPI.io posting flow; no changes to `apps/*` runtime code.
- **Local stack prerequisite**: Postgres + Redis (docker), `nautilus-server`,
  `dugong-api` (API + processor) must be running, as described in
  `docs/local-dev-guide.md`.
- **Secrets**: requires the same TwitterAPI.io env vars (`TWITTERAPI_IO_API_KEY`,
  `TWITTERAPI_IO_PROXY`, `TWITTERAPI_IO_LOGIN_COOKIES`) for the real-post mode; dry-run
  mode needs none.
