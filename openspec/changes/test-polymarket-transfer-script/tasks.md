## 1. Scaffolding & config

- [x] 1.1 Create `scripts/test-flows.ts` with a `#!/usr/bin/env -S tsx` shebang, matching the style of `deploy-contract.ts`
- [x] 1.2 Add a `test-flows` entry to `scripts/package.json` (`"test-flows": "tsx test-flows.ts"`); confirm `tsx` is already a devDependency
- [x] 1.3 Add `pg` to `scripts/package.json` devDependencies for `webhook_events` polling (or reuse an existing pg client if present)
- [x] 1.4 Implement config resolution: `BACKEND_URL` (default `http://localhost:43001`), `DATABASE_URL` (default local from `docs/local-dev-guide.md`), and CLI flags `--coin`, `--amount`, `--bettors`, `--only=transfer|market`, `--dry-run`, `--timeout`

## 2. Pre-flight checks

- [x] 2.1 Health-check `GET {BACKEND_URL}/` and fail fast with a message pointing to `docs/local-dev-guide.md` if unreachable
- [x] 2.2 Open a Postgres connection and verify connectivity; fail fast on error
- [x] 2.3 In real-post mode, validate `TWITTERAPI_IO_API_KEY`, `TWITTERAPI_IO_PROXY`, `TWITTERAPI_IO_LOGIN_COOKIES` are present and non-placeholder; skip this check under `--dry-run`

## 3. Tweet posting & webhook triggering helpers

- [x] 3.1 Port the TwitterAPI.io `create_tweet_v2` posting logic from `apps/tools/src/bin/test_tweet.rs` into a TS helper that posts a tweet (with optional `reply_to_tweet_id`) and returns its `tweet_id`; embed a unique timestamp to avoid duplicate-content 422s
- [x] 3.2 Implement `triggerWebhook(tweetId, text, screenName)` that POSTs the `process_tweet_url.sh`-shaped payload to `/webhook`
- [x] 3.3 Implement a `postOrInject(text, replyTo?)` wrapper: real-post mode posts via 3.1 then triggers via 3.2; `--dry-run` skips posting and injects synthetic `tweet_create_events`
- [x] 3.4 Ensure no secret values (api key, proxy, cookies) are ever logged

## 4. Verification via webhook_events

- [x] 4.1 Implement `waitForTerminal(tweetId, timeout)` that polls `webhook_events` for `event_id = 'tweet:<id>'` until `status` is terminal or timeout
- [x] 4.2 Return `{ status, tx_digest, error_message }`; on timeout report last-seen status instead of a bare error

## 5. Flow steps

- [x] 5.1 Account step: ensure/create the sender account via a `create account` command and wait for terminal status
- [x] 5.2 Transfer step: run `send <amount> <coin> to @<user>`, wait for terminal, pass only on success + present `tx_digest`
- [x] 5.3 Market create step: run `create market: <question>`, capture the returned market tweet id, wait for terminal
- [x] 5.4 Bet step(s): for `--bettors` count, post `bet <amount> <coin> on yes|no` as replies to the market tweet id, waiting for each terminal status
- [x] 5.5 Resolve step: post `resolve yes` as a reply to the market tweet id, wait for terminal
- [x] 5.6 Sequence steps so each waits for the prior terminal status; respect `--only` to run just one flow

## 6. Reporting & exit

- [x] 6.1 Accumulate per-step results (name, pass/fail, status, tx_digest, error_message)
- [x] 6.2 Print a readable summary table at the end
- [x] 6.3 Exit non-zero if any step failed, zero otherwise

## 7. Docs & verification

- [x] 7.1 Add a usage section (env vars, flags, local-stack prerequisite, real-post vs `--dry-run`) to `scripts`' README or a header comment block in `test-flows.ts`
- [ ] 7.2 Run `pnpm test-flows --dry-run --only=transfer` against a local stack and confirm webhook plumbing works end-to-end
- [ ] 7.3 Run `pnpm test-flows` (real-post) for both flows against a local stack and confirm the summary reports passing steps with `tx_digest`s
