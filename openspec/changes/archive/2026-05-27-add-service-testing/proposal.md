## Why

The workspace has six Rust crates and one React frontend, but test coverage is sparse and inconsistent: three crates (`indexer`, `tools`, `worker`) and the web app have zero tests, the rest have only pure-logic unit tests, and there is no CI to run anything. The code paths that actually carry risk — DB writes, Redis dedup, Sui RPC calls, HTTP clients, the enclave processor — are entirely untested, and there is no test infrastructure (DB fixtures, HTTP mocking, sqlx offline mode) to make testing them practical.

## What Changes

- Establish a **shared testing convention** across all Rust crates: pure unit tests live inline in `#[cfg(test)]` modules; integration tests live in each crate's `tests/` dir with a `common/` fixtures module.
- Add **HTTP mocking** (`wiremock`) so client code (`sui_client`, `enoki`, `enclave`, `twitter`, worker/tools clients) can be tested against canned responses instead of live services.
- Add **ephemeral Postgres test infrastructure** via `sqlx::test`, plus `sqlx prepare` + `SQLX_OFFLINE` so query macros compile in CI without a live DB.
- Add **integration tests per Rust service**:
  - `core`: client tests against wiremock, DB model round-trips against `sqlx::test`.
  - `api`: webhook handler + route tests via `axum` `ServiceExt`, processor worker dispatch tests with mocked enclave/twitter/sui.
  - `indexer`: event handler tests writing to `sqlx::test` DB, cursor manager tests, event fetcher against wiremock.
  - `worker`: poller conversion logic + client tests against wiremock.
  - `tools`: login-flow client tests against wiremock.
  - `nautilus-server`: extend existing BCS/crypto coverage with HTTP-handler tests.
- Add a **web test harness** (`vitest` + `@testing-library/react`) with a `test` script and initial tests for hooks/utils (`pkce`, `api`, `useXAuth`).
- Add **Playwright E2E** for the web app: a browser-driven suite covering the critical user flows (home → onboarding, OAuth callback, dashboard/account view) against a built/served frontend with mocked backend routes.
- Add a **CI workflow** (`.github/workflows`) running `cargo test --workspace` (with a Postgres service + `SQLX_OFFLINE`), `pnpm test` (vitest), and `pnpm test:e2e` (Playwright) for web.

## Capabilities

### New Capabilities
- `rust-service-testing`: Conventions and infrastructure for unit and integration testing all Rust crates — fixture layout, HTTP mocking, ephemeral Postgres, and per-service coverage expectations.
- `web-app-testing`: Test harness and conventions for the React frontend — unit/component/hook runner (vitest), Playwright E2E for critical user flows, and scripts.
- `test-ci`: Continuous integration that runs the full test suite for both the Rust workspace and the web app on every push/PR.

### Modified Capabilities
<!-- None — no existing specs. -->

## Impact

- **New dev-dependencies**: `wiremock`, `sqlx` (`test` feature), `serde_json` (dev) across Rust crates; `vitest`, `@testing-library/react`, `@testing-library/jest-dom`, `jsdom`, and `@playwright/test` in `apps/web`.
- **New files**: `tests/` dirs and `tests/common/mod.rs` fixtures per crate; `*.test.ts(x)` files in web; `.github/workflows/test.yml`; `apps/web/vitest.config.ts` (or `test` block in `vite.config.ts`); committed `.sqlx/` offline query cache.
- **No production code behavior changes** — additions are test-only, except minor refactors where code must accept injectable base URLs / clients to be testable (e.g. client constructors taking a configurable base URL).
- **CI**: introduces a Postgres service container and `SQLX_OFFLINE=true` build step.
