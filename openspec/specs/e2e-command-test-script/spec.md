## Requirements

### Requirement: Single-command flow runner

The system SHALL provide a script, runnable via `pnpm test-flows` (executing
`scripts/test-flows.ts` with `tsx`), that exercises the bot's transfer and
prediction-market command lifecycles against a running stack and exits non-zero if any
step fails.

#### Scenario: Run all flows successfully

- **WHEN** a developer runs `pnpm test-flows` against a healthy local stack with valid
  Twitter credentials
- **THEN** the script runs the transfer flow and the full prediction-market lifecycle
- **AND** prints a per-step pass/fail summary including each step's `tx_digest`
- **AND** exits with code `0`

#### Scenario: A step fails

- **WHEN** any step ends with a failed `webhook_events` status or an error
- **THEN** the script reports the failing step with its `error_message`
- **AND** exits with a non-zero code

### Requirement: Transfer (send money) flow coverage

The script SHALL exercise the `send <amt> <coin> to @<user>` command end-to-end.

#### Scenario: Successful transfer

- **WHEN** the transfer step runs
- **THEN** the script ensures the sender account exists, posts/triggers a `send`
  command tweet, and waits for the resulting `webhook_events` row to reach a terminal
  status
- **AND** reports the transfer as passed only when the status is success and a
  `tx_digest` is present

### Requirement: Prediction-market lifecycle coverage

The script SHALL exercise the `create market: <q>` → `bet <amt> <coin> on yes|no` →
`resolve yes|no` lifecycle, threading bet and resolve tweets as replies to the market
tweet.

#### Scenario: Create, bet, resolve in order

- **WHEN** the prediction-market flow runs
- **THEN** the script creates a market (capturing its tweet id), places one or more
  bets as replies to that tweet, then resolves the market as a reply to that tweet
- **AND** each step waits for the prior step's terminal `webhook_events` status before
  the next is triggered
- **AND** reports each of market-created, each bet, and resolve as passed only when its
  status is success

### Requirement: Drive the real production path via the webhook

The script SHALL trigger processing by posting the same webhook-shaped payload as
`apps/api/process_tweet_url.sh` to `POST /webhook`, and SHALL verify outcomes by
polling the `webhook_events` table rather than introducing a new server endpoint.

#### Scenario: Webhook trigger and DB verification

- **WHEN** the script processes a command tweet with id `<id>`
- **THEN** it POSTs a `tweet_create_events` payload for `<id>` to `/webhook`
- **AND** polls `webhook_events` for `event_id = 'tweet:<id>'` until terminal status or
  a configurable timeout
- **AND** surfaces the row's `status`, `tx_digest`, and `error_message` in its report

### Requirement: Configuration and modes

The script SHALL be configurable by environment variables and CLI flags, and SHALL
support a `--dry-run` mode that requires no Twitter credentials.

#### Scenario: Default real-post mode

- **WHEN** the script runs without `--dry-run`
- **THEN** it posts real command tweets through the TwitterAPI.io flow (as
  `dugong-test-tweet` does), reusing `TWITTERAPI_IO_*` env vars, and uses the returned
  tweet ids for webhook triggering and reply threading

#### Scenario: Dry-run mode without credentials

- **WHEN** the script runs with `--dry-run` and no Twitter credentials
- **THEN** it skips posting and injects synthetic `tweet_create_events` payloads
- **AND** does not error due to missing `TWITTERAPI_IO_*` variables

#### Scenario: Configurable target and parameters

- **WHEN** the script runs
- **THEN** it honors `BACKEND_URL` (default `http://localhost:43001`) and flags for coin
  type, amount, number of bettors, which flow(s) to run, and the polling timeout

### Requirement: Pre-flight checks and safe diagnostics

The script SHALL verify prerequisites before running and SHALL NOT print secret values.

#### Scenario: Stack not reachable

- **WHEN** the API health endpoint or the database is unreachable at startup
- **THEN** the script fails fast with a clear message pointing to
  `docs/local-dev-guide.md`
- **AND** does not attempt to post tweets or trigger webhooks

#### Scenario: Secrets are never logged

- **WHEN** the script emits diagnostic output
- **THEN** API keys, proxy strings, and login cookies are redacted or omitted
