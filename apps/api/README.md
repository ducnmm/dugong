# dugong-api

HTTP service for Dugong. Receives Twitter webhooks, exposes REST endpoints
for the web app, and runs the in-process transaction processor that signs
and submits Sui transactions via the Nautilus enclave.

This crate is one piece of a Cargo workspace; shared code (db, clients,
config, migrations) lives in `dugong-core`, and the on-chain event indexer
runs as a separate binary `dugong-indexer`. See the
[repo-level README](../../README.md) and
[local-dev guide](../../docs/local-dev.md) for the full picture.

## What this crate contains

```
apps/api/
├── Cargo.toml
├── docker-compose.yml      # local Postgres + Redis
├── process_tweet_url.sh    # manual webhook trigger
├── src/
│   ├── main.rs             # binary entry point
│   ├── lib.rs
│   ├── routes.rs           # REST endpoints (account, wallet link, OAuth, sponsor)
│   ├── error.rs            # API error type / axum IntoResponse
│   ├── processor/
│   │   └── worker.rs       # Redis queue -> enclave -> Sui -> reply
│   └── webhook/
│       ├── handler.rs      # /webhook + CRC challenge
│       └── signature.rs    # Twitter signature validation
└── tests/
    └── unit_tests.rs
```

Shared modules used by this crate (lifted into `dugong-core`):

- `dugong_core::config` — env-driven `Config`
- `dugong_core::db` — Postgres pool, models, embedded migrations
- `dugong_core::clients` — Redis, Sui, Twitter, Enclave, Enoki, Sui-tx builder
- `dugong_core::constants` — Redis key helpers, event ids, enclave consts
- `dugong_core::twitter_session` — login cookie validation

## Quick start

### 1. Prerequisites

```bash
# Spin up local Postgres + Redis
cd apps/api
docker compose up -d
```

(Or install Postgres 14 + Redis natively; defaults in `.env.example` match
the docker-compose ports.)

### 2. Configure

```bash
cp apps/api/.env.example apps/api/.env
# fill in real values - see docs/local-dev.md for which keys are required
```

### 3. Run

From the repo root:

```bash
cargo run -p dugong-api
# embedded migrations run on startup; server listens on http://localhost:43001
```

The API refuses to start without `TWITTERAPI_IO_LOGIN_COOKIES` +
`TWITTERAPI_IO_PROXY`, because the processor posts a reply to every tweet
it handles. If you only need the read-side, run `dugong-indexer` instead
(see below).

### Running alongside the indexer

The indexer is its own binary in a sibling crate and shares this crate's
`.env`:

```bash
cargo run -p dugong-indexer
```

Both processes read `apps/api/.env` (same Postgres + Sui RPC) so the
indexer cursor and the API's webhook state stay consistent.

### Helper CLIs

The one-off scripts moved to the `dugong-tools` crate:

```bash
cargo run -p dugong-tools --bin dugong-login        # mint TwitterAPI.io login cookie
cargo run -p dugong-tools --bin dugong-test-tweet   # smoke-test posting + self-reply
```

## API endpoints

### Health check

```bash
curl http://localhost:43001/
```

### CRC challenge (GET)

```bash
curl "http://localhost:43001/webhook?crc_token=test123"
```

### Webhook event (POST)

```bash
curl -X POST http://localhost:43001/webhook \
  -H "Content-Type: application/json" \
  -d '{
    "for_user_id": "123456",
    "tweet_create_events": [{
      "id_str": "1234567890",
      "text": "@DugongWallet send 5 SUI to @alice",
      "user": {
        "id_str": "123456",
        "screen_name": "bob"
      }
    }]
  }'
```

Account / wallet / OAuth / sponsor routes are wired in `src/main.rs`; see
`src/routes.rs` for the handlers.

## Twitter webhook setup (optional)

In production you can wire the official X Account Activity webhook. Locally
the worker poller is usually enough.

```bash
ngrok http 43001
```

Register `https://<your-ngrok>.ngrok.io/webhook` in the X developer portal
and set `TWITTER_WEBHOOK_SECRET` in `.env` for CRC signing.

## Database schema

Embedded migrations live in `apps/core/migrations/`. Run them manually with:

```bash
sqlx migrate run \
  --source apps/core/migrations \
  --database-url postgres://postgres:password@localhost:45432/dugong
```

Highlights:

- `dugong_accounts` — Twitter ↔ on-chain DugongAccount mapping
  (`twitter_user_id`, `twitter_handle`, `sui_object_id`, `owner_address`).
- `webhook_events` — Received webhook events with `processed` flag and the
  raw `payload` (jsonb), used for dedup and replay.
- `indexer_state` — Per-stream cursor for `dugong-indexer`.

## Development

```bash
# Type-check / test the whole workspace
cargo check --workspace
cargo test  --workspace

# Just this crate
cargo check -p dugong-api
cargo test  -p dugong-api
```

## Environment variables

See `.env.example` for everything. Key variables:

- `DATABASE_URL` — Postgres connection string
- `REDIS_URL` — Redis connection string
- `SUI_RPC_URL` — Sui fullnode RPC endpoint
- `ENCLAVE_URL` — Nautilus enclave endpoint
- `ENCLAVE_ID` — Enclave shared object id created by `register_enclave`
  (the Enclave object, not the config)
- `DUGONG_PACKAGE_ID` — Deployed Move package id
- `DUGONG_REGISTRY_ID` — `DugongRegistry` shared object id
- `ENCLAVE_CONFIG_ID` — Enclave config object id (signature verification)
- `TWITTERAPI_IO_LOGIN_COOKIES`, `TWITTERAPI_IO_PROXY` — TwitterAPI.io
  credentials for reply tweets
- `TWITTER_BOT_TOTP_SECRET` — X 2FA base32 seed used by `dugong-login`;
  guest login cookies cannot post replies
- `INDEXER_POLL_INTERVAL_MS`, `INDEXER_BATCH_SIZE` — read by
  `dugong-indexer` only

> `ENABLE_INDEXER` was previously used to co-run the indexer inside the API
> process. The indexer is now always a separate binary; the variable is
> retained in `Config` for backwards compatibility but has no effect.

## Security notes

- Never commit `.env`.
- Keep TwitterAPI.io, X OAuth2, and signing credentials secure.
- Validate all webhook signatures in production.
- Use proper database credentials in production.
