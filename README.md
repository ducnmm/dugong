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
- `apps/tools` — one-off CLIs (`dugong-login`, `dugong-test-tweet`).
- `apps/worker` — `dugong-worker` poller: scans X for mentions and posts to
  the API webhook.
- `apps/nautilus-server` — Nautilus enclave-facing service.
- `apps/web` — frontend (Vite + React).
- `contracts/move` — Move contracts.

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
cargo run -p dugong-tools --bin dugong-login
cargo run -p dugong-tools --bin dugong-test-tweet
```

See [docs/local-dev.md](docs/local-dev.md) for the full local development
guide.
