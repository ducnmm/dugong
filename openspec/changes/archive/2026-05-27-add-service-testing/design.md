## Context

The Dugong workspace is a Cargo workspace of six Rust crates (`core`, `api`, `indexer`, `tools`, `nautilus-server`, `worker`) plus a Vite + React frontend (`apps/web`). Current test state:

- `core`, `api`, `nautilus-server` have only pure-logic unit tests (HMAC, BCS, serde, key formatting).
- `indexer`, `tools`, `worker`, `web` have **zero** tests.
- No CI runs tests; no `.sqlx/` offline cache; `apps/api/tests/common/mod.rs` is an empty stub.

The untested risk surface is the I/O boundary: Postgres writes (sqlx), Redis dedup/queue, Sui RPC, and HTTP clients (Enoki, enclave, TwitterAPI.io). These need test infrastructure — DB fixtures and HTTP mocking — before meaningful tests can exist.

## Goals / Non-Goals

**Goals:**
- One consistent testing convention across all Rust crates.
- Make the I/O boundary testable: HTTP mocking for all reqwest clients, ephemeral Postgres for sqlx code.
- Compile sqlx query macros in CI without a live DB (`SQLX_OFFLINE` + committed `.sqlx/`).
- Establish a web test harness (vitest) and seed it with hook/util tests.
- Cover critical web user flows end-to-end with Playwright against a served build.
- A CI workflow that runs the whole suite on push/PR.

**Non-Goals:**
- 100% coverage. We seed each service with meaningful tests and a repeatable pattern; exhaustive coverage comes later.
- Testing inside a real AWS Nitro Enclave (nsm_api paths stay mocked/feature-gated).
- E2E coverage of wallet-signature flows that require a real browser wallet extension (those steps are stubbed/mocked in Playwright).
- Live integration against real Sui/Twitter/Enoki services (E2E mocks backend routes; it does not hit live infra).

## Decisions

### HTTP mocking: `wiremock` over `httpmock` / `mockito`
`wiremock` is async-native (tokio), runs a real local server, and matches the async reqwest clients used throughout. It avoids the global-state pitfalls of `mockito`. Tests inject the wiremock server's base URL into the client under test — this requires client constructors to accept a configurable base URL rather than reading a hardcoded const.

**Alternative considered:** `httpmock` (sync-leaning, heavier). Rejected for ergonomics with tokio tests.

### Postgres: `sqlx::test` over testcontainers
`#[sqlx::test]` provisions an isolated, migrated database per test and tears it down automatically, reading `DATABASE_URL` for the server. It reuses our existing `sqlx::migrate!` migrations directly. Testcontainers would add a Docker dependency and slower startup.

**Alternative considered:** Shared test DB with transaction rollback. Rejected — `sqlx::test` already gives per-test isolation with less boilerplate.

### sqlx offline mode: commit `.sqlx/`
Run `cargo sqlx prepare --workspace` to generate the `.sqlx/` query cache, commit it, and build CI with `SQLX_OFFLINE=true`. This lets `cargo test` compile the query macros without a DB at compile time; the DB is only needed at test runtime for `#[sqlx::test]` cases.

### Testability refactors are allowed but minimal
Where a client hardcodes its base URL via a const, add a constructor (or builder param) that accepts a base URL, defaulting to the const in production. This is the smallest change that unlocks wiremock. No behavioral change in production paths.

### Test layout convention
- Pure logic → inline `#[cfg(test)] mod tests` next to the code.
- Cross-module / I/O → `tests/<area>.rs` integration files, with shared fixtures in `tests/common/mod.rs`.
- Fixtures (sample payloads, canned JSON) live in `tests/common/` or `tests/fixtures/`.

### Web: `vitest` + `@testing-library/react`
Vitest shares Vite's config/transform pipeline (zero extra build config), is fast, and is the de-facto standard for Vite + React 19. `jsdom` provides the DOM environment; `@testing-library/react` + `@testing-library/jest-dom` for component/hook assertions.

**Alternative considered:** Jest. Rejected — needs separate transform config for Vite/ESM/TSX.

### Web E2E: `@playwright/test` against a served build, backend mocked
Playwright drives a real Chromium against the frontend. To keep E2E hermetic and independent of the Rust backend, the suite runs against `vite preview` (a production build) using Playwright's `webServer` config to boot the server, and intercepts backend API calls via `page.route()` with canned responses. This covers routing, rendering, and client-side flow logic without standing up Postgres/Redis/Sui.

Wallet-signature steps (dapp-kit) that need a real extension are stubbed at the network/route boundary rather than driving a wallet UI — those remain out of scope for E2E.

**Alternative considered:** Cypress. Rejected — Playwright has better multi-browser support, native TS, and a lighter CI footprint; it also pairs cleanly with Vite's preview server.

**Alternative considered:** Running E2E against the real Rust backend. Rejected for this change — couples web CI to backend infra and slows feedback; mocked routes keep the suite fast and deterministic.

### CI: GitHub Actions, three logical suites
A `rust` job spins up a `postgres` service container, runs `SQLX_OFFLINE=true cargo test --workspace`. A `web` job runs `pnpm install` + `pnpm test --run` (vitest) and then `pnpm test:e2e` (Playwright, with browsers installed via `pnpm exec playwright install --with-deps chromium`). Split jobs keep failures legible and allow parallelism.

## Risks / Trade-offs

- **[sqlx `.sqlx/` cache drifts from queries]** → CI runs `cargo sqlx prepare --check` (or rebuilds offline) to fail when the committed cache is stale; document the regen command.
- **[Testability refactors touch production constructors]** → Keep default args so existing call sites compile unchanged; cover with the new tests themselves.
- **[`worker` doesn't depend on `dugong-core`]** → It gets its own wiremock-based client tests; no shared fixture coupling assumed.
- **[Postgres service container slows CI]** → Acceptable; only the DB-touching tests need it, and it runs in parallel with the web job.
- **[React 19 + testing-library version mismatch]** → Pin compatible versions (`@testing-library/react` ≥ 16) when adding deps.
- **[Playwright browser download bloats CI]** → Install only Chromium (`--with-deps chromium`) and cache the browser binaries between runs.
- **[E2E flakiness from timing]** → Use Playwright auto-waiting locators and `webServer.reuseExistingServer` locally; avoid fixed sleeps.
- **[Mocked routes drift from real backend contracts]** → Keep route fixtures minimal and colocated; the Rust integration tests remain the source of truth for backend behavior.

## Migration Plan

1. Add dev-deps to workspace + per-crate `Cargo.toml`; add web dev-deps.
2. Land testability refactors (configurable base URLs) — no behavior change.
3. Add fixtures + tests per crate, verifying `cargo test -p <crate>` locally with a local Postgres.
4. Generate and commit `.sqlx/`.
5. Add web `vitest.config` + `test` script + seed tests.
6. Add Playwright config + `test:e2e` script + critical-flow specs against `vite preview`.
7. Add CI workflow; confirm green.

Rollback: tests and CI are additive; reverting the commit removes them with no production impact.

## Open Questions

- Should `worker` adopt `dugong-core` clients to share test infra, or stay independent? (Default: stay independent for this change.)
- Do we want coverage reporting (`cargo-llvm-cov`, `vitest --coverage`) in CI now or later? (Default: later.)
- Should Playwright E2E eventually run against the real Rust backend in a dedicated integration pipeline? (Default: mocked routes for now.)
