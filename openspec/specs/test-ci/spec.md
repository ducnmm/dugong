### Requirement: Continuous integration test runs

A GitHub Actions workflow SHALL run the full test suite for both the Rust workspace and the web app on every push and pull request.

#### Scenario: Rust suite in CI
- **WHEN** a push or pull request triggers CI
- **THEN** a job SHALL provision a Postgres service container and run `cargo test --workspace` with `SQLX_OFFLINE=true`

#### Scenario: Web unit suite in CI
- **WHEN** a push or pull request triggers CI
- **THEN** a job SHALL install dependencies and run the web unit/component suite (`pnpm test --run`)

#### Scenario: Web E2E suite in CI
- **WHEN** a push or pull request triggers CI
- **THEN** a job SHALL install Playwright browsers and run the E2E suite (`pnpm test:e2e`) against a served build

#### Scenario: Failure blocks merge
- **WHEN** any test in either job fails
- **THEN** the workflow SHALL report failure so the pull request is marked as not passing

### Requirement: Reproducible CI environment

The CI workflow SHALL pin the toolchain and provide the database connection required by `#[sqlx::test]` runtime tests.

#### Scenario: Database available to tests
- **WHEN** the Rust job runs DB-touching tests
- **THEN** `DATABASE_URL` SHALL point at the workflow's Postgres service so `#[sqlx::test]` can create per-test databases
