## ADDED Requirements

### Requirement: Shared Rust test conventions

The workspace SHALL follow a single testing convention across all Rust crates: pure-logic tests live inline in `#[cfg(test)]` modules adjacent to the code, while cross-module and I/O tests live in each crate's `tests/` directory with shared fixtures in `tests/common/mod.rs`.

#### Scenario: Pure-logic test placement
- **WHEN** a test exercises only in-memory logic (serialization, formatting, parsing, crypto vectors)
- **THEN** it SHALL be defined in an inline `#[cfg(test)] mod tests` block in the same source file

#### Scenario: Integration test placement
- **WHEN** a test exercises multiple modules or an I/O boundary (DB, HTTP, Redis)
- **THEN** it SHALL be defined in a `tests/<area>.rs` file and reuse fixtures from `tests/common/mod.rs`

### Requirement: HTTP client mocking

All reqwest-based clients SHALL be testable against a local mock HTTP server (`wiremock`) without contacting live services. Client constructors SHALL accept a configurable base URL so tests can point them at the mock server.

#### Scenario: Client tested against mocked response
- **WHEN** a client method is called in a test with the mock server returning a canned response
- **THEN** the client SHALL parse that response and the test SHALL assert on the parsed result without any live network call

#### Scenario: Production default base URL preserved
- **WHEN** a client is constructed without an explicit base URL in production code
- **THEN** it SHALL default to the existing production endpoint, leaving production behavior unchanged

### Requirement: Ephemeral Postgres test infrastructure

Database-touching code SHALL be tested against an isolated, automatically-migrated Postgres database per test using `#[sqlx::test]`, reusing the existing `apps/core/migrations` migrations.

#### Scenario: Isolated database per test
- **WHEN** a `#[sqlx::test]` function runs
- **THEN** it SHALL receive a freshly created database with all migrations applied, isolated from other tests, and torn down afterward

#### Scenario: Model round-trip
- **WHEN** a DB model is inserted and then queried back in a test
- **THEN** the retrieved row SHALL equal the inserted values

### Requirement: Database required only at test runtime

The workspace uses runtime sqlx queries (`sqlx::query_as::<_, T>(...).bind(...)`), not compile-time-checked `query!` macros, so a database is NOT required to compile. A live Postgres SHALL be required only at test runtime for `#[sqlx::test]` cases; the compile step SHALL NOT depend on `DATABASE_URL` or a `.sqlx/` offline cache.

#### Scenario: Compile without database
- **WHEN** the workspace is built with no `DATABASE_URL` set
- **THEN** the build SHALL succeed because no compile-time-checked query macros are used

#### Scenario: DB-backed tests need a running Postgres
- **WHEN** `#[sqlx::test]` tests are executed
- **THEN** a reachable Postgres (via `DATABASE_URL`) SHALL be required, and CI SHALL provide it as a service container

### Requirement: Per-service test coverage

Each Rust crate SHALL have integration tests covering its primary responsibility.

#### Scenario: core clients and models
- **WHEN** the `core` crate test suite runs
- **THEN** it SHALL include client tests against wiremock (sui/enoki/enclave/twitter) and DB model round-trip tests against `sqlx::test`

#### Scenario: api handlers and processor
- **WHEN** the `api` crate test suite runs
- **THEN** it SHALL include webhook/route tests driven through the Axum app and processor-worker dispatch tests with mocked enclave, twitter, and sui dependencies

#### Scenario: indexer handlers and cursor
- **WHEN** the `indexer` crate test suite runs
- **THEN** it SHALL include event-handler tests writing to a `sqlx::test` database, cursor manager tests, and event-fetcher tests against wiremock

#### Scenario: worker poller and clients
- **WHEN** the `worker` crate test suite runs
- **THEN** it SHALL include tweet-to-webhook conversion tests and client tests against wiremock

#### Scenario: tools login flow
- **WHEN** the `tools` crate test suite runs
- **THEN** it SHALL include login-flow client tests against wiremock

#### Scenario: nautilus-server handlers
- **WHEN** the `nautilus-server` crate test suite runs
- **THEN** it SHALL retain its BCS/crypto tests and add HTTP-handler tests for its command endpoints
