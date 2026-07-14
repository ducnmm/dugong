# dugong

Dugong monorepo. Rust services live in a single Cargo workspace rooted at
`Cargo.toml`; the frontend is its own pnpm app.

## Structure

- `apps/core` — shared Rust library (`dugong-core`): clients, config, db,
  constants, twitter session helpers, migrations. Every Rust binary depends
  on this crate.
- `apps/api` — `dugong-api` HTTP service: webhooks, routes, processor worker.
- `apps/indexer` — `dugong-indexer` background service: mirrors Sui events
  into Postgres.
- `apps/tools` — one-off CLIs (`dugong-bot-authorize`).
- `apps/worker` — `dugong-worker` poller: scans X for mentions and posts to
  the API webhook.
- `apps/nautilus-server` — Nautilus enclave-facing service.
- `apps/web` — frontend (Vite + React).
- `contracts/move` — Move contracts.

## Quick start

Start the entire local stack (Postgres + Redis, Nautilus, API, indexer,
worker, web) with one command:

```bash
pnpm dev            # or: ./scripts/dev.sh
```

It brings up the Docker infra, seeds any missing `.env` files from their
`.env.example`, builds the Rust services, and runs everything with
colour-coded logs. Ctrl-C stops all services (Postgres/Redis stay up).

```bash
pnpm dev:infra                 # just Postgres + Redis
pnpm dev:down                  # stop Postgres + Redis
./scripts/dev.sh --no-worker   # skip services needing X credentials
./scripts/dev.sh --help        # all flags
```

Fill in the secrets in `apps/api/.env` before the API/worker will do real
work — see [docs/local-dev-guide.md](docs/local-dev-guide.md).

## Quick commands

```bash
# build everything
cargo build --workspace

# run a specific service
cargo run -p dugong-api
cargo run -p dugong-indexer
cargo run -p dugong-worker
cargo run -p nautilus-server

# run a tool
cargo run -p dugong-tools --bin dugong-bot-authorize
```

See [docs/local-dev-guide.md](docs/local-dev-guide.md) for the full local development
guide.
