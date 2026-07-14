# Design: Migrate Sui data access from JSON-RPC to GraphQL

## Context

Sui fullnodes are dropping JSON-RPC (fullnode.testnet.sui.io already did in July 2026). Two read paths in this codebase depend on it, both via the hand-rolled client in `apps/core/src/clients/sui_client.rs`:

1. **Indexer event polling** — `suix_queryEvents` with a `MoveEventModule { package, module: "events" }` filter, paginated by an `EventId { txDigest, eventSeq }` cursor persisted per package in the `indexer_state` Postgres table (`apps/indexer/src/cursor.rs`). The polling loop (`apps/indexer/src/indexer.rs:57-102`) fetches up to 100 events per tick; a dormant historical-sync path pages 1000 at a time.
2. **API coin-decimals lookup** — `suix_getCoinMetadata`, called per-request from `apps/api/src/routes.rs` (`resolve_coin_decimals`, lines ~603, ~1148, ~1189).

The transaction-building path (`apps/core/src/clients/sui_transaction.rs`) uses the official `sui-sdk` over the same JSON-RPC URL for object reads and gas price, but submits transactions through Enoki sponsorship. It is explicitly out of scope here.

Sui's GraphQL RPC (`https://graphql.{network}.sui.io/graphql`) is the supported replacement: an `events` connection with event-type filtering and Relay-style pagination (`first`/`after`, opaque string cursors, `pageInfo { hasNextPage endCursor }`), and a `coinMetadata(coinType:)` query.

## Goals / Non-Goals

**Goals:**
- Indexer ingests events via GraphQL with no events skipped or double-processed across the cutover.
- API coin-metadata lookups work via GraphQL with unchanged route behavior (same decimals/symbol results).
- One-time, automatic migration of persisted legacy cursors; no manual DB surgery required.
- Downstream event processing (`event_processor.rs`, `handlers/`, `types.rs`) unchanged — the fetch layer keeps returning the same event shape.
- Test suites keep running against wiremock, now mocking GraphQL.

**Non-Goals:**
- Migrating `sui_transaction.rs` (`read_api()` object/gas-price reads) off JSON-RPC.
- Moving to gRPC, or adopting the official `sui-sdk` GraphQL/gRPC clients.
- Real-time event streaming/subscriptions; the polling model stays.
- Caching coin metadata (current per-request behavior is preserved).

## Decisions

### 1. Hand-rolled GraphQL over `reqwest`, no codegen client

Rewrite `SuiClient` to POST `{"query": ..., "variables": ...}` JSON to the GraphQL endpoint using the existing `reqwest` + `serde_json` stack.

- **Why**: only three small queries (events page, coin metadata, cursor re-anchor lookup). Codegen crates (`cynic`, `graphql-client`) add build complexity and a schema-sync burden for no benefit at this scale. This also mirrors the current client's style and keeps wiremock testing trivial.
- **Alternative considered**: `sui-sdk` GraphQL client / `sui-graphql-client` crate — rejected: the workspace pins `sui-sdk` to an old git rev for tx building; pulling a second, newer Sui client stack risks dependency conflicts for two queries' worth of functionality.

### 2. Keep the public `SuiClient` API and event shape stable

`query_events(package_id, module, cursor, limit)` and `get_coin_metadata(coin_type)` keep their signatures; the returned event struct keeps `type`, `parsed_json`, `id { tx_digest, event_seq }`-equivalent identity fields, and `timestamp_ms`. Internally the GraphQL response (`contents { type { repr } json }`, `timestamp`, `transaction { digest }`) is mapped into that shape, converting the ISO-8601 `timestamp` to epoch milliseconds.

- **Why**: `event_fetcher.rs`, `event_processor.rs`, all 13 handlers, and the API routes stay untouched except for construction/config. Blast radius stays in one file plus config.

### 3. Event filter: event-type prefix instead of `MoveEventModule`

Map the current `MoveEventModule { package, module: "events" }` filter to the GraphQL events filter with an event-type prefix of `"<package>::events"`. Sui GraphQL event-type filters match by prefix (package, `package::module`, or full type), which has the same semantics as `MoveEventModule` (module that *defines* the event type). The indexer already iterates one query per defining package id, which maps 1:1.

- **Caveat**: the filter field name differs across GraphQL schema generations (`eventType` in the legacy schema, `type`/`module` in the beta schema, where the docs note `module` and `type` cannot be combined). The implementation MUST verify the exact field name against the live endpoint (introspection or a smoke query) as the first implementation task, and target the schema served by `graphql.testnet.sui.io`.

### 4. Cursor: persist a re-anchorable envelope, not just the opaque cursor

GraphQL cursors are opaque, endpoint-specific strings that can expire (they are checkpoint-anchored and only valid within the RPC's retention window). Persisting only the opaque cursor would make restarts fragile and migration impossible. Instead, the `indexer_state.cursor` column (already a string) stores a JSON envelope:

```json
{ "v": 2, "gql": "<opaque endCursor>", "tx": "<digest>", "seq": "<eventSeq>", "cp": <checkpoint> }
```

`tx`/`seq`/`cp` describe the last *processed* event and serve as a durable re-anchor point independent of any endpoint's cursor encoding.

- **Legacy detection**: a stored value that does not parse as this JSON envelope is treated as a legacy `"txDigest:eventSeq"` cursor.
- **Re-anchoring algorithm** (used both for legacy migration and for expired/rejected GraphQL cursors): resolve the anchor transaction's checkpoint (from `cp`, or by querying the transaction by digest for legacy cursors), then page events with the same filter starting from that checkpoint (e.g. `afterCheckpoint: cp - 1`) and skip events until the one matching `(tx, seq)` is found; adopt that event's GraphQL cursor and resume normally. This guarantees no skipped and no re-processed events.
- **Failure mode**: if the anchor transaction is outside the endpoint's available range (pruned), the indexer MUST fail loudly with a clear operator message rather than silently restarting from latest or genesis.
- **Alternative considered**: resume with `afterCheckpoint: cp` alone — rejected: skips events that follow the anchor event *within* the same checkpoint.

### 5. Config: new `SUI_GRAPHQL_URL`, keep `SUI_RPC_URL`

Add `sui_graphql_url` to `apps/core/src/config.rs`, from `SUI_GRAPHQL_URL`, defaulting to `https://graphql.testnet.sui.io/graphql`. `SUI_RPC_URL` stays for `sui_transaction.rs`.

- **Why not reuse `SUI_RPC_URL`**: the two endpoints have different URLs and protocols, and the tx path is out of scope; overloading one variable would force both migrations at once and break Railway deploys mid-rollout.

### 6. Page-size clamp and in-tick pagination

Public GraphQL endpoints cap page size (~50) — below the current poll limit (100) and historical page size (1000). `query_events` clamps the requested limit to the service max, and the indexer's per-tick fetch loops on `hasNextPage` until it either drains the backlog or hits the old per-tick budget. Public endpoints are also rate-limited; docs will recommend a provider endpoint or self-hosted GraphQL stack for production, replacing the current blockvision JSON-RPC recommendation in `docs/local-dev-guide.md`.

## Risks / Trade-offs

- [GraphQL schema drift between legacy and beta generations (filter/field names)] → Pin the implementation to what `graphql.testnet.sui.io` actually serves, verified by introspection at implementation start; keep all query strings in one module so a schema change is a one-file fix.
- [Cursor expiry due to retention window on public endpoints] → Re-anchor envelope (Decision 4) makes recovery automatic while the anchor is within range; loud failure with operator guidance when it is not.
- [Historical backfill (`sync_historical`) may exceed public endpoints' retention/rate limits] → It is already dead code; document that backfill from genesis requires a provider endpoint with full history.
- [Timestamp semantics change (ISO-8601 string vs `timestampMs`)] → Convert at the client boundary; add a unit test asserting millisecond equivalence.
- [Rate limits on public GraphQL endpoints throttle the poll loop] → Poll interval is already configurable (`indexer_poll_interval_ms`); per-tick page budget bounds request volume; production guidance updated in docs.
- [Wiremock tests may drift from the real schema] → Capture one real testnet response per query as the fixture baseline during implementation.

## Migration Plan

1. Ship code that reads both cursor formats (envelope + legacy) and writes only the new envelope.
2. Deploy with `SUI_GRAPHQL_URL` set (Railway + local `.env`); indexer re-anchors legacy cursors automatically on first tick.
3. Verify: indexer logs show re-anchor once per package, then normal paging; API balance/tx-history endpoints still resolve decimals.
4. Rollback: revert the deploy — legacy JSON-RPC client and old cursor values are only overwritten after the first successful GraphQL page is processed, and the envelope retains `tx`/`seq`, which the old code's `"txDigest:eventSeq"` parser cannot read — so rollback additionally requires restoring the cursor string from the envelope's `tx`/`seq` fields (a one-line SQL update documented in the tasks). Acceptable given handlers' upsert-style writes.

## Open Questions

- Does the pinned deployment target (`graphql.testnet.sui.io`) serve the legacy or beta schema at implementation time? (Determines exact filter field names; resolve via introspection in the first task.)
- Is every event handler idempotent under a rare double-process during re-anchoring edge cases? Spot-check `handlers/` during implementation; if any are not, add an idempotency guard keyed on `(tx_digest, event_seq)`.
