# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Dugong is a Twitter/X-based custodial Sui wallet. Users tweet `@DugongWallet send <amount> <coin> to @<receiver>`; the transfer is authorized inside an AWS Nitro **Nautilus enclave** (which independently fetches and verifies the tweet), signed with the enclave's ephemeral Ed25519 key, and executed on Sui where a Move contract verifies the enclave signature and enforces replay protection.

Trust model: the backend never holds signing authority for transfers. The enclave is the only component that fetches tweets from TwitterAPI.io and produces signed payloads, so a malicious/compromised backend cannot forge transfers — it can only relay enclave-signed payloads to chain.

## Monorepo layout

A Cargo workspace (`apps/api`, `apps/nautilus-server`, `apps/worker`) plus a non-workspace frontend and Move packages.

- `apps/api` — Public Axum backend. Two binaries: `dugong-api` (HTTP server + processor worker) and `dugong-indexer` (Sui event indexer). Talks to Postgres, Redis, the enclave, Sui, Enoki, and Twitter.
- `apps/nautilus-server` — The Nautilus enclave service (Mysten Labs framework, vendored). Feature-gated apps under `src/apps/`; the active one is `dugong` (default feature). `seal-example` is an alternate unrelated demo app.
- `apps/worker` — Standalone TwitterAPI.io poller. Polls for mentions and forwards them to the backend webhook as synthesized webhook payloads (an alternative to official Twitter Account Activity webhooks).
- `apps/web` — React 19 + Vite + TypeScript + Tailwind frontend using `@mysten/dapp-kit` and Enoki for sponsored transactions.
- `contracts/move/dugong` — Main Move package. Depends on the local `enclave` package.
- `contracts/move/enclave` — Mysten Nautilus enclave attestation/registration Move package.
- `contracts/move/seal-policy` — Unrelated Seal demo policy package.

## End-to-end transfer flow

1. User tweets a transfer command.
2. Tweet reaches the backend either via the official Twitter webhook (`/webhook`) or via `apps/worker` polling TwitterAPI.io and POSTing a synthesized payload to `/webhook`.
3. `webhook/handler.rs` validates, dedups (Redis + `webhook_events` table), and enqueues onto a Redis queue.
4. `processor::ProcessorWorker` (spawned inside `dugong-api`) pops the queue and calls the enclave's `/process_data` with the tweet URL.
5. The enclave independently fetches the tweet from TwitterAPI.io, parses it, resolves user IDs, builds a `TransferPayload`, and signs it.
6. Backend submits a Sui transaction carrying the payload + enclave signature; the Move contract verifies the signature against the registered enclave pubkey and checks replay protection (timestamp / processed-tweet tracking).
7. `dugong-indexer` polls Sui events and mirrors account/handle/wallet-link/transfer events into Postgres so the API can serve account and transaction data.

`secure_link_wallet` is a separate flow: the frontend proves wallet ownership via signature + X OAuth2 token; the enclave verifies the OAuth token against `api.twitter.com` before signing a `link_wallet` payload.

## Commands

### Rust workspace (run from repo root)

```bash
cargo build                 # build all workspace members
cargo check                 # fast type-check
cargo test                  # run all tests
cargo test -p dugong-api    # test a single crate
cargo test -p dugong-api test_name      # run a single test by name
cargo run -p dugong-api --bin dugong-api      # HTTP server (port 43001)
cargo run -p dugong-api --bin dugong-indexer  # standalone indexer
cargo run -p dugong-worker                    # Twitter poller
```

The API server runs DB migrations on startup. If `ENABLE_INDEXER=false` (recommended for local dev), run `dugong-indexer` as a separate process — it shares the same `.env`, Postgres, and Redis.

### Nautilus enclave (run from `apps/nautilus-server`)

The enclave is feature-gated; **always pass the feature**:

```bash
cargo check --features dugong
cargo test --features dugong
TWITTERAPI_IO_API_KEY=... cargo run --features dugong   # local, NOT in enclave (port 43000)
```

`default = ["dugong"]`. Do not enable `seal-example` for Dugong work — it's a separate demo and pulls different deps. Production builds use a reproducible enclave image (`make enclave FEATURE=dugong` in the Nautilus ref repo); see `apps/nautilus-server/src/apps/dugong/README.md`.

### Frontend (run from `apps/web`)

```bash
npm install
npm run dev      # Vite dev server
npm run build    # tsc -b && vite build
npm run lint     # eslint
```

### Move contracts (run from a package dir, e.g. `contracts/move/dugong`)

```bash
sui move build
sui move test
```

`dugong` depends on the local `enclave` package; build/test from within `contracts/move/dugong`. Deployed object IDs live in each package's `Published.toml` and must be wired into the backend `.env` (`DUGONG_PACKAGE_ID`, `DUGONG_REGISTRY_ID`, `ENCLAVE_ID`, `ENCLAVE_CONFIG_ID`).

## Important cross-cutting concerns

- **Sui SDK pinning**: every Sui-related git dependency in `apps/api/Cargo.toml` is pinned to the same revision (`rev = "94ad8cc..."`). Keep them in lockstep when bumping. The enclave crate and Move packages pin to *different* Sui revs intentionally — do not unify them blindly.
- **Payload struct parity**: the enclave's `TransferPayload` (Rust) must match the Move `TransferCoinPayload` field-for-field, because the contract verifies a BCS-serialized signature over it. Changing one requires changing both. The serde/BCS behavior is covered by enclave unit tests (`test_transfer_payload_serde`, `test_transfer_regex`).
- **Config**: all Rust services load config from environment (`.env` via dotenvy). The backend's required vars are documented in `apps/api/README.md` ("Environment Variables"). There is no `.env` in git; `.env.example` is the source of truth.
- **DB schema**: SQL migrations live in `apps/api/migrations/` and run automatically on `dugong-api`/`dugong-indexer` startup. `sqlx` is used with compile-time-checked queries — a running/migrated database may be needed for `cargo build` of `dugong-api` depending on sqlx offline setup.
- **Indexer is the source of truth for the API**: API read endpoints serve mirrored Postgres state, not live Sui queries. If account/transaction data looks stale, suspect the indexer cursor (`indexer_state` table) before the API.

## Ports

- `dugong-api`: 43001
- `nautilus-server` (enclave, local): 43000
- Postgres / Redis: per `DATABASE_URL` / `REDIS_URL` (README examples use ports 45432 / default)
</content>
</invoke>
