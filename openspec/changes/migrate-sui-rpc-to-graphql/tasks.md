# Tasks: Migrate Sui data access from JSON-RPC to GraphQL

## 1. Schema verification and config

- [x] 1.1 Introspect `https://graphql.testnet.sui.io/graphql` (or run smoke queries) to confirm the served schema generation: exact `events` filter field names (`eventType` vs `type`/`module`), event node fields (`contents { type { repr } json }`, timestamp, transaction digest, sequence), cursor semantics, max page size, and the transaction-by-digest query needed for re-anchoring. Record findings as comments in the new client module.
- [x] 1.2 Add `sui_graphql_url` field to `apps/core/src/config.rs`, read from `SUI_GRAPHQL_URL`, defaulting to `https://graphql.testnet.sui.io/graphql`; leave `sui_rpc_url` untouched.

## 2. GraphQL client (apps/core)

- [x] 2.1 Rewrite `apps/core/src/clients/sui_client.rs` internals as a GraphQL client: generic `execute(query, variables)` POST helper over `reqwest` + `serde_json` that surfaces GraphQL `errors` and non-2xx responses as `Err` (never a silent empty page).
- [x] 2.2 Implement `query_events(package_id, module, cursor, limit)` against the `events` connection with an event-type prefix filter (`<package>::<module>`), ascending order, `first`/`after` pagination, clamping `limit` to the service max; map nodes to the existing event struct (type string, parsed JSON, tx digest, event sequence, ISO-8601 timestamp → epoch ms) and return page data + end cursor + `hasNextPage`.
- [x] 2.3 Implement `get_coin_metadata(coin_type)` via `coinMetadata(coinType:) { decimals symbol name }`, preserving the existing return type and treating GraphQL `null` as the existing not-found result.
- [x] 2.4 Implement a `get_transaction_checkpoint(digest)` helper (for cursor re-anchoring) returning the checkpoint sequence number, with a distinct error for "not found / out of available range".
- [x] 2.5 Add unit tests: timestamp ms conversion, limit clamping, GraphQL error propagation, and event-node → event-struct mapping.

## 3. Cursor envelope and migration (apps/indexer)

- [x] 3.1 Define the cursor envelope type `{ v, gql, tx, seq, cp }` with serde round-trip, plus a parser that classifies stored strings as envelope vs legacy `"txDigest:eventSeq"`; wire it into `apps/indexer/src/cursor.rs` (write only envelopes; read both).
- [x] 3.2 Implement re-anchoring in the indexer: given an anchor `(tx, seq, cp)` — from a legacy cursor (resolving `cp` via 2.4) or from an envelope whose `gql` cursor the endpoint rejects — page events with the same filter from the anchor checkpoint onward, skip through the anchor event, and adopt the GraphQL cursor at that position; fail loudly (package id, digest, remediation) when the anchor is out of range.
- [x] 3.3 Update `apps/indexer/src/event_fetcher.rs` and `apps/indexer/src/indexer.rs` for the new pagination: per-tick loop on `hasNextPage` with a bounded page budget, persisting the envelope only after each page's events are fully processed (failure leaves the previous cursor in place).
- [x] 3.4 Update `sync_historical()` to the same client/pagination (page size ≤ service max) and document that genesis backfill requires a full-history provider endpoint.
- [x] 3.5 Add indexer tests (wiremock): first fetch with no cursor, cursor round-trip across pages, legacy-cursor re-anchor (no skip/no re-process across the anchor), rejected-cursor re-anchor, processing-failure-does-not-advance-cursor, and out-of-range loud failure.

## 4. API integration (apps/api)

- [x] 4.1 Update `SuiClient` construction in `apps/api/src/routes.rs` (~lines 1148, 1189) to use `config.sui_graphql_url`; confirm `resolve_coin_decimals` behavior is unchanged for found and missing metadata.
- [x] 4.2 Update wiremock fixtures in `apps/core/tests/common/mod.rs`, `apps/api/tests/common/mod.rs`, `apps/indexer/tests/common/mod.rs`, and `apps/api/tests/routes.rs` to serve GraphQL responses (capture one real testnet response per query as the fixture baseline).

## 5. Docs, env, and deployment

- [x] 5.1 Add `SUI_GRAPHQL_URL` to `apps/api/.env.example` and `apps/indexer/.env.example`; fix the stale `SUI_RPC_URL=https://fullnode.testnet.sui.io:443` examples.
- [x] 5.2 Update `docs/local-dev-guide.md`: replace the JSON-RPC provider guidance (blockvision) with GraphQL endpoint guidance, including retention/rate-limit notes and the full-history-provider requirement for backfill.
- [x] 5.3 Update `docs/deployment_railway_cli.md` to set `SUI_GRAPHQL_URL` on Railway services, and document the rollback SQL (restore `indexer_state.cursor` to `"tx:seq"` from the envelope fields) per the design's migration plan.
- [x] 5.4 Spot-check event handlers in `apps/indexer/src/handlers/` for idempotency under a rare double-process (design open question); add an `(tx_digest, event_seq)` idempotency guard if any handler is not idempotent.

## 6. Verification

- [x] 6.1 Run the full workspace test suite (`cargo test`) and clippy; fix regressions.
- [x] 6.2 End-to-end against testnet: run the indexer with a legacy cursor seeded in `indexer_state`, confirm one re-anchor log per package followed by normal paging, and confirm API balance/tx-history endpoints resolve coin decimals via GraphQL.
