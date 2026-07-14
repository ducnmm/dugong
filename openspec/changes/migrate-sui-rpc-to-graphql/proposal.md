# Proposal: Migrate Sui data access from JSON-RPC to GraphQL

## Why

Sui is sunsetting the JSON-RPC API on fullnodes (fullnode.testnet.sui.io already stopped serving JSON-RPC in July 2026; other providers will follow). The indexer's event polling and the API's coin-metadata lookups both go through a hand-rolled JSON-RPC client (`suix_queryEvents`, `suix_getCoinMetadata`), so once the remaining public JSON-RPC nodes drop the API, the indexer stops ingesting on-chain events and the product breaks. Sui's GraphQL RPC is the supported replacement for these read paths.

## What Changes

- Replace the hand-rolled JSON-RPC client in `apps/core/src/clients/sui_client.rs` with a GraphQL client that talks to Sui's GraphQL RPC (`events` query with an event-type filter, `coinMetadata` query).
- **BREAKING (internal)**: The persisted indexer cursor format changes from `"txDigest:eventSeq"` to the opaque GraphQL pagination cursor. Existing cursors stored in the `indexer_state` table must be detected and migrated (re-anchored) on first run, without re-processing or skipping events.
- New config: `SUI_GRAPHQL_URL` env var / `sui_graphql_url` config field, defaulting to the official testnet GraphQL endpoint. `SUI_RPC_URL` remains for the transaction-building path (official `sui-sdk` reads in `sui_transaction.rs`), which is out of scope for this change.
- Update `.env.example` files, Railway deployment docs, and the local-dev guide to reference the GraphQL endpoint.
- Update wiremock-based test fixtures in `apps/core`, `apps/api`, and `apps/indexer` to mock GraphQL responses instead of JSON-RPC.

Out of scope: migrating `apps/core/src/clients/sui_transaction.rs` (official `sui-sdk` `read_api()` calls for object refs and gas price). Transaction submission already goes through Enoki sponsorship, and the object-read migration is a separate, larger effort (likely to gRPC or GraphQL later).

## Capabilities

### New Capabilities

- `sui-graphql-access`: Read access to Sui chain data over GraphQL RPC — paginated event querying filtered by defining module (replacing `suix_queryEvents` with `MoveEventModule` filter), coin metadata lookup (replacing `suix_getCoinMetadata`), and migration of persisted JSON-RPC event cursors to GraphQL cursors.

### Modified Capabilities

<!-- none — no existing spec covers Sui data access or the indexer's fetch layer -->

## Impact

- **Code**: `apps/core/src/clients/sui_client.rs` (rewrite), `apps/core/src/config.rs` (new config field), `apps/indexer/src/event_fetcher.rs` + `apps/indexer/src/indexer.rs` + `apps/indexer/src/cursor.rs` (cursor handling), `apps/api/src/routes.rs` (coin decimals lookup, lines ~603, ~1148, ~1189).
- **Data**: `indexer_state` rows hold legacy-format cursors that need one-time re-anchoring.
- **Config/Deploy**: new `SUI_GRAPHQL_URL` in `apps/api/.env.example`, `apps/indexer/.env.example`, Railway variables (`docs/deployment_railway_cli.md`), `docs/local-dev-guide.md`.
- **Tests**: wiremock fixtures in `apps/core/tests/common/mod.rs`, `apps/api/tests/common/mod.rs`, `apps/indexer/tests/common/mod.rs`, `apps/api/tests/routes.rs`.
- **Dependencies**: no new heavy deps required — GraphQL over plain `reqwest` POST + `serde_json`, consistent with the current client style.
