# Local Development

This guide explains how to run the full Dugong stack on your machine.

All Rust services live in one Cargo workspace rooted at the repo's
`Cargo.toml`. Run `cargo` commands from the repo root.

## Components & Ports

| Service          | Crate                  | Local URL                | Port  |
| ---------------- | ---------------------- | ------------------------ | ----- |
| PostgreSQL       | docker (`apps/api`)    | `localhost:45432`        | 45432 |
| Redis            | docker (`apps/api`)    | `localhost:46379`        | 46379 |
| Nautilus enclave | `apps/nautilus-server` | `http://localhost:43000` | 43000 |
| API + processor  | `apps/api`             | `http://localhost:43001` | 43001 |
| Indexer          | `apps/indexer`         | n/a                      | n/a   |
| Worker (poller)  | `apps/worker`          | n/a                      | n/a   |
| Web (Vite)       | `apps/web`             | `http://localhost:43173` | 43173 |

Shared library: `apps/core` (`dugong-core`) — every Rust binary depends on
it for config, db, clients, and migrations.

Data flow: `worker poller` (or `process_tweet_url.sh`) → API `/webhook` →
Redis queue → API processor → Nautilus `/process_tweet` → Sui. The indexer
mirrors Sui events back into Postgres.

## Prerequisites

- Rust (stable) + Cargo
- Node 20+ and pnpm (or npm)
- Docker (for Postgres + Redis)
- `jq` (used by `apps/api/process_tweet_url.sh`)
- Optional: `sqlx-cli` for running migrations manually, `ngrok` for exposing
  the API to the real X Account Activity webhook

## 1. Start infrastructure (Postgres + Redis)

```bash
cd apps/api
docker compose up -d
```

This brings up `dugong-postgres` (DB `dugong`, user `postgres`, password
`password`) on port 45432 and `dugong-redis` on port 46379, matching the
defaults in `apps/api/.env.example`.

## 2. Configure environment

Each binary reads `apps/api/.env` (the API, indexer, and tools all share
this file because they share the same Postgres, Redis, Sui RPC, and Twitter
config). The worker and web app have their own `.env`:

```bash
cp apps/api/.env.example     apps/api/.env
cp apps/worker/.env.example  apps/worker/.env
cp apps/web/.env.example     apps/web/.env
# apps/nautilus-server/.env already exists; set ENCLAVE_PORT + TWITTERAPI_IO_API_KEY
```

Key values you must supply in `apps/api/.env`:

- `TWITTERAPI_IO_API_KEY` — tweet/user lookup (reads)
- `TWITTER_API_KEY`/`TWITTER_API_SECRET` + `TWITTER_ACCESS_TOKEN`/
  `TWITTER_ACCESS_TOKEN_SECRET` — OAuth 1.0a keys for posting replies via the
  official X API (app "Keys and tokens" page; the indexer does not need these)
- `TWITTER_OAUTH2_CLIENT_ID`, `TWITTER_OAUTH2_CLIENT_SECRET` — X OAuth2 from
  https://developer.twitter.com (user login; also the alternative bot-posting
  path via `dugong-bot-authorize`)
- `DUGONG_PACKAGE_ID`, `DUGONG_REGISTRY_ID`, `MARKET_REGISTRY_ID`,
  `ENCLAVE_CONFIG_ID`, `ENCLAVE_ID` — from the deployed Move contracts
  (`ENCLAVE_ID` is the `Enclave` shared object from `register_enclave`, **not**
  the config object)
- `MARKET_TREASURY_ACCOUNT_ID` — shared `DugongAccount` that receives the
  protocol fee on `markets::resolve`; created once per network via
  `scripts/create-treasury.ts` (see below)
- `ENOKI_API_KEY` — gas sponsorship
- `BACKEND_SIGNER_PRIVATE_KEY` — base64-encoded BCS `SuiKeyPair`

`DATABASE_URL`, `REDIS_URL`, `SUI_RPC_URL`, and `ENCLAVE_URL` already point at
the local stack in the example file.

### Deploying Move contracts

`scripts/deploy-contract.ts` builds, publishes/upgrades the Move packages and
patches the contract IDs above into `apps/api/.env`, `apps/indexer/.env`, and
`apps/web/.env`. Run it from the repo root:

```bash
# Default — deploys only `dugong` (DugongRegistry, MarketRegistry).
scripts/deploy-contract.ts --network testnet

# All three packages in dependency order (enclave → seal-policy → dugong).
scripts/deploy-contract.ts --package all --network testnet

# Preview without touching the chain.
scripts/deploy-contract.ts --package all --dry-run
```

Each package has its own `Published.toml`; if it contains an
`upgrade-capability` for the target network the script runs `sui client
upgrade`, otherwise `sui client publish`. Make sure `sui client active-env`
matches `--network` and your active address is funded.

### Creating the treasury account

`MARKET_TREASURY_ACCOUNT_ID` is a regular shared `DugongAccount` — there is no
dedicated treasury type. Create it once per network with:

```bash
# Requires DUGONG_PACKAGE_ID + DUGONG_REGISTRY_ID in apps/api/.env (i.e. run
# the deploy script first).
scripts/create-treasury.ts --network testnet
```

The script calls `dugong::account::init_account_no_signature`, parses the
created `DugongAccount` from the JSON output, and records it in
`contracts/move/dugong/Treasury.toml` keyed by network. Subsequent runs of
`deploy-contract.ts` read that file automatically and write the id back into
`MARKET_TREASURY_ACCOUNT_ID`. Pass `--xid` / `--handle` to customise, or
`--force` to overwrite an existing entry. Commit `Treasury.toml` alongside
`Published.toml`.

## 3. Run the Nautilus enclave server

```bash
cargo run -p nautilus-server
# listens on http://localhost:43000 (ENCLAVE_PORT)
```

## 4. Run the API + processor

```bash
cargo run -p dugong-api
# runs migrations and serves http://localhost:43001
```

Health check: `curl http://localhost:43001/`

The API + processor binary requires reply credentials (either the OAuth 1.0a
key set, or `TWITTER_BOT_USER_ID` + a stored OAuth 2.0 token); it refuses to
start otherwise so replies never get silently dropped.

## 5. Run the indexer

The indexer is now its own binary (`dugong-indexer`). Run it as a separate
process — it does not need reply credentials and can run on a host that
only has Postgres + Sui RPC reachable:

```bash
cargo run -p dugong-indexer
# mirrors Sui events into the indexer_state + dugong_accounts tables
```

The indexer loads its **own** `apps/indexer/.env` (resolved relative to the
crate, so the command above works from any directory). Keep its
`DUGONG_PACKAGE_ID` / `DUGONG_EVENT_PACKAGE_ID` / `SUI_GRAPHQL_URL` in sync
with `apps/api/.env` when the package is republished — a stale file here means
the current package's events silently never get indexed. Real environment
variables still take precedence, which is how it picks up Railway-injected
config in production.

**Endpoint choice matters for the indexer**: it reads events via Sui's
GraphQL RPC (`SUI_GRAPHQL_URL`; JSON-RPC was retired for reads). The default
`https://graphql.testnet.sui.io/graphql` is fine for a warm indexer that only
tails new events, but it is rate-limited and retains a bounded history
window. Two consequences:

- **Backfill from genesis** (fresh database) or resuming from a very old
  cursor needs a full-history GraphQL provider endpoint — the public endpoint
  will not have the early events, and the indexer fails loudly with a
  `cannot re-anchor` error rather than silently skipping history.
- If the indexer was down long enough for its stored cursor to fall out of
  the endpoint's retention window, it automatically re-anchors from the last
  processed event's transaction — which again must still be within the
  endpoint's history.

The API server is fine on the public endpoint — it only looks up coin
metadata. (`SUI_RPC_URL` still exists but is only used for transaction
building via the sui-sdk.)

## 6. Run the worker (poller)

Polls X for mentions of `@DugongWallet` and forwards them to the API webhook:

```bash
cargo run -p dugong-worker
```

Configured via `apps/worker/.env` (`BACKEND_URL`, `POLL_INTERVAL_SECONDS`,
`MAX_TWEETS_PER_POLL`, `TWITTER_MENTION`). `MAX_TWEETS_PER_POLL` defaults to
`1` and controls how many matching tweets are sent to the backend per poll.

## 7. Run the web app

```bash
cd apps/web
pnpm install        # or: npm install
pnpm dev            # http://localhost:43173
```

Vite proxies `/api` to `http://localhost:43001`. Set the `VITE_*` contract
addresses in `apps/web/.env` to match `apps/api/.env`.

## Helper tools

The one-off CLIs live in the `dugong-tools` crate:

```bash
# Authorize the bot for OAuth 2.0 posting (not needed with OAuth 1.0a keys)
cargo run -p dugong-tools --bin dugong-bot-authorize
```

Both read from `apps/api/.env` (run them from the repo root so the workspace
target dir is reused).

## Triggering a tweet without the poller

Useful for testing the full pipeline against a specific tweet:

```bash
cd apps/api
./process_tweet_url.sh 'https://x.com/DugongWallet/status/2055661622073676261'

# Re-process a tweet (clears local dedup + webhook_events row):
FORCE=1 ./process_tweet_url.sh '<tweet_url>'
```

Watch the API logs for `Pushed tweet … to queue`, `Calling unified
/process_tweet endpoint`, and `Account initialized successfully`.

## Common tasks

```bash
# Build / type-check / test the whole workspace
cargo build --workspace
cargo check  --workspace
cargo test   --workspace

# Run migrations manually (migrations live in apps/core/migrations)
sqlx migrate run \
  --source apps/core/migrations \
  --database-url postgres://postgres:password@localhost:45432/dugong

# Inspect local DB / Redis
docker exec -it dugong-postgres psql -U postgres -d dugong
docker exec -it dugong-redis redis-cli

# Tear down infra
cd apps/api && docker compose down          # add -v to wipe volumes
```

## Testing

The Rust suites split into two kinds:

- **Pure-logic and HTTP-client tests** (wiremock-backed) need no external
  services. They run with a plain `cargo test`.
- **Database tests** use `#[sqlx::test]`, which provisions an isolated,
  migrated Postgres database per test. These need a reachable Postgres via
  `DATABASE_URL`. The codebase uses runtime sqlx queries (not `query!`
  macros), so a database is **not** needed to *compile* — only to *run* the
  DB-backed tests.
- **Redis-backed api tests** (webhook enqueue / processor) need a reachable
  Redis via `REDIS_URL` (default `redis://127.0.0.1:56379`). They *skip
  themselves* if Redis is unreachable, so they never fail a DB-only run.

```bash
# Spin up a throwaway Postgres for tests
docker run -d --name dugong-test-pg \
  -e POSTGRES_PASSWORD=postgres -p 55432:5432 postgres:16-alpine

# Point #[sqlx::test] at it and run everything
export DATABASE_URL="postgres://postgres:postgres@localhost:55432/postgres"
cargo test --workspace

# A single crate
cargo test -p dugong-core
```

Frontend checks live in `apps/web`:

```bash
cd apps/web
pnpm lint
pnpm build
```

## Suggested startup order

1. `docker compose up -d` in `apps/api` (Postgres + Redis)
2. `cargo run -p nautilus-server`
3. `cargo run -p dugong-api` (API + processor)
4. `cargo run -p dugong-indexer` (Sui events → Postgres)
5. `cargo run -p dugong-worker` (or use `process_tweet_url.sh`)
6. `pnpm dev` in `apps/web`
