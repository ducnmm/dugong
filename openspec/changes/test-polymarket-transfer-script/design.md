## Context

Dugong's bot commands flow `webhook → Redis queue → API processor → Nautilus
/process_tweet → Sui → reply tweet` (see `docs/local-dev-guide.md` and
`apps/api/process_tweet_url.sh`). The Nautilus enclave parses authoritative tweet
text fetched by id, supporting: `send <amt> <coin> to @<user>`, `create market: <q>`,
`bet <amt> <coin> on|with yes|no`, `resolve|solve yes|no`, and `create account`
(`apps/nautilus-server/src/apps/dugong/mod.rs`). Bets and resolutions must be replies
to the originating market tweet so parent-tweet lookup can associate and authorize them.

Today the only manual trigger is `process_tweet_url.sh`, which takes one real tweet
URL at a time. The `dugong-test-tweet` tool (`apps/tools/src/bin/test_tweet.rs`) already
posts a tweet and a threaded reply through the TwitterAPI.io path the processor uses.
There is no orchestrated script that posts a sequence of command tweets, triggers
processing, and asserts the on-chain outcome for the transfer and prediction-market
flows together.

## Goals / Non-Goals

**Goals:**
- One command (`pnpm test-flows`) that exercises the transfer and prediction-market
  lifecycles against a locally running stack and reports per-step pass/fail.
- Reuse existing payload shapes (webhook) and posting flow (TwitterAPI.io) rather than
  inventing new server endpoints.
- Surface the resulting `tx_digest` / `error_message` from `webhook_events` per step.
- Non-zero exit on any failed step so it is CI-usable.
- A `--dry-run` mode that needs no Twitter secrets, for fast local iteration.

**Non-Goals:**
- Changing any `apps/*` runtime code, bot command grammar, or the prediction-market
  spec.
- Asserting exact payout math on-chain (the script verifies the resolve tx succeeds;
  parimutuel correctness stays covered by Move/Rust tests).
- Replacing Rust unit/integration tests; this is an end-to-end smoke test.
- Provisioning the stack (docker/cargo) — that remains a documented prerequisite.

## Decisions

### Decision: TypeScript + `tsx` runner in `scripts/`
The existing scripts (`deploy-contract.ts`, `railway-set-env.ts`) are TypeScript run
via `tsx`, with `tsx` already in `scripts/package.json` devDependencies. Matching that
keeps one toolchain and lets the script reuse `node:` APIs and `pg` for DB polling.
*Alternative considered:* a Rust binary in `apps/tools`. Rejected — slower edit loop,
and the orchestration (HTTP + DB poll + sequencing) is lighter in TS.

### Decision: Drive through `/webhook`, not a new endpoint
The script posts the same webhook-shaped JSON as `process_tweet_url.sh`
(`for_user_id`, `tweet_create_events[]`) to `POST /webhook`. This keeps the test on the
real production path and avoids new server surface.
*Alternative considered:* calling Nautilus `/process_tweet` directly. Rejected — skips
the queue/processor/reply layers we want covered.

### Decision: Real-post mode vs `--dry-run`
- **Real-post mode (default):** posts actual command tweets via the TwitterAPI.io flow
  (reusing `dugong-test-tweet` logic), capturing each returned `tweet_id`, then
  triggers `/webhook` with that id. Because Nautilus fetches the authoritative tweet by
  id, the posted text is what gets parsed — this is the only mode that fully validates
  parsing + threading.
- **`--dry-run` mode:** skips posting; injects synthetic `tweet_create_events` text
  directly. Useful when secrets are absent or to test webhook plumbing, but note the
  enclave re-fetches by id, so dry-run validates queue/processor wiring, not parse of
  the synthetic text. Documented as such.

### Decision: Verify via `webhook_events` polling
After triggering each tweet, poll Postgres `webhook_events` for
`event_id = 'tweet:<id>'` until `status` is terminal (success/failed) or a timeout,
then report `status`, `tx_digest`, `error_message`. This mirrors how
`process_tweet_url.sh` already inspects that table and needs no new API.
*Alternative considered:* scraping reply tweets. Rejected — slower, flakier, and
duplicates info already in the DB.

### Decision: Step sequencing and threading
Run order: (1) ensure/create account for sender, (2) transfer step, (3) create market
(root tweet), (4) one or more bets as replies to the market tweet, (5) resolve as a
reply to the market tweet. Each step waits for the prior step's terminal status before
proceeding, so escrow exists before resolve. Tweet ids are carried in-memory to build
the reply threads.

### Decision: Configuration
Env + CLI flags: `BACKEND_URL` (default `http://localhost:43001`), `--coin`,
`--amount`, `--bettors`, `--only=transfer|market`, `--dry-run`, `--timeout`. Twitter
secrets read from env/`.env` exactly like the server and `dugong-test-tweet`.

## Risks / Trade-offs

- [Real tweets are side-effecting and rate-limited] → Default to a clearly-labelled
  test handle/text with unique timestamps (as `dugong-test-tweet` already does to dodge
  422 duplicates); document that real-post mode consumes live API quota.
- [Stack not running / partially up] → Pre-flight health check on `GET /` (43001) and a
  DB connectivity check; fail fast with a clear message pointing at
  `docs/local-dev-guide.md`.
- [On-chain settlement latency causes false timeouts] → Configurable `--timeout` with a
  sensible default; report last-seen `status` on timeout rather than a bare error.
- [Dry-run gives false confidence about parsing] → Explicitly document that the enclave
  re-fetches by id, so dry-run does not validate synthetic command text.
- [Secrets in logs] → Never print cookie/proxy/api-key values; redact in any diagnostic
  output.

## Migration Plan

Additive tooling only. Land `scripts/test-flows.ts` and the `package.json` script;
no deploy or data migration. Rollback = delete the file and the script entry. No effect
on running services.

## Open Questions

- Should real-post mode use a dedicated throwaway test account distinct from
  `@DugongWallet`, and where do those credentials live for CI? (Default for now: local
  use with the developer's existing TwitterAPI.io login cookies; CI runs `--dry-run`.)
- Do we want an optional assertion that the sender/receiver balances changed via the
  REST account endpoint, or is tx-success sufficient for the smoke test? (Default:
  tx-success; balance assertion deferred.)
