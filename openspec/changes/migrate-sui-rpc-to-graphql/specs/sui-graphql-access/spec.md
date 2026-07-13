# Spec: sui-graphql-access

## ADDED Requirements

### Requirement: Event querying over GraphQL
The Sui client SHALL fetch on-chain Move events from the Sui GraphQL RPC endpoint using the `events` connection, filtered so that only events whose type is defined in the watched package's `events` module are returned (equivalent semantics to the JSON-RPC `MoveEventModule` filter). Results SHALL be returned in ascending chain order together with pagination state (`hasNextPage`, end cursor).

#### Scenario: Fetch first page of events for a package
- **WHEN** the indexer requests events for a watched package with no stored cursor
- **THEN** the client queries the GraphQL endpoint with an event-type filter of `<package>::events` and returns the first page of matching events in ascending order with the page's end cursor and `hasNextPage` flag

#### Scenario: Fetch subsequent page with cursor
- **WHEN** the indexer requests events with a previously returned GraphQL cursor
- **THEN** the client passes the cursor as the `after` argument and returns only events after that position, with no events skipped or repeated

#### Scenario: GraphQL endpoint returns errors
- **WHEN** the GraphQL response contains an `errors` array or a non-success HTTP status
- **THEN** the client returns an error (it MUST NOT return a partial/empty page as success), and the indexer leaves its persisted cursor unchanged

### Requirement: Event shape compatibility
Events returned by the GraphQL client SHALL expose the same fields the downstream event processor consumes today: the fully qualified event type string, the parsed JSON payload, the emitting transaction digest, the event sequence within the transaction, and the timestamp in epoch milliseconds.

#### Scenario: Event fields are mapped from GraphQL response
- **WHEN** a GraphQL event node is returned with `contents.type.repr`, `contents.json`, transaction digest, sequence number, and an ISO-8601 timestamp
- **THEN** the client maps it to the existing event struct with the type string, parsed JSON, digest, sequence, and the timestamp converted to epoch milliseconds, and all existing event handlers process it without modification

### Requirement: Coin metadata over GraphQL
The Sui client SHALL resolve coin metadata (at minimum decimals and symbol) for a given coin type via the GraphQL `coinMetadata(coinType:)` query, replacing `suix_getCoinMetadata`.

#### Scenario: Coin metadata found
- **WHEN** the API requests metadata for a coin type that has a CoinMetadata object on chain
- **THEN** the client returns the coin's decimals and symbol, and API responses (balance and transaction-history endpoints) are byte-for-byte equivalent to the JSON-RPC-backed behavior

#### Scenario: Coin metadata missing
- **WHEN** the API requests metadata for a coin type with no CoinMetadata object
- **THEN** the client returns a "not found" result (GraphQL `null`) and the API applies the same fallback behavior it used when JSON-RPC returned no metadata

### Requirement: Cursor persistence envelope
The indexer SHALL persist its per-package cursor as a JSON envelope containing the opaque GraphQL cursor plus a durable re-anchor point: the last processed event's transaction digest, event sequence, and checkpoint sequence number. The envelope SHALL only be written after the events of the corresponding page have been successfully processed.

#### Scenario: Cursor saved after successful page processing
- **WHEN** a page of events is fetched and all events in it are processed successfully
- **THEN** the indexer persists the envelope with the page's end cursor and the last event's digest, sequence, and checkpoint

#### Scenario: Processing failure does not advance cursor
- **WHEN** event processing fails partway through a page
- **THEN** the persisted cursor remains at its previous value so the events are re-fetched on the next tick

### Requirement: Legacy cursor migration
On startup or first fetch, the indexer SHALL detect legacy `"txDigest:eventSeq"` cursors persisted by the JSON-RPC implementation and re-anchor them automatically: resolve the anchor transaction's checkpoint, page events with the same filter from that checkpoint onward, skip past the anchor event, and adopt the GraphQL cursor at that position. No events after the anchor SHALL be skipped, and no events at or before the anchor SHALL be re-processed.

#### Scenario: Legacy cursor is re-anchored
- **WHEN** the indexer starts with a stored cursor that does not parse as the JSON envelope
- **THEN** it treats the value as `txDigest:eventSeq`, resolves the transaction's checkpoint via GraphQL, resumes fetching from that checkpoint, skips events up to and including the anchor event, and persists a new-format envelope after the first successfully processed page

#### Scenario: Anchor transaction outside available range
- **WHEN** the anchor transaction cannot be found on the configured GraphQL endpoint (pruned or out of retention)
- **THEN** the indexer fails with a clear error identifying the package, the anchor digest, and remediation options, and does NOT silently restart from genesis or from the latest checkpoint

#### Scenario: Expired GraphQL cursor is re-anchored
- **WHEN** the GraphQL endpoint rejects a persisted opaque cursor (e.g. out of retention after downtime)
- **THEN** the indexer falls back to re-anchoring from the envelope's digest/sequence/checkpoint using the same algorithm as legacy migration

### Requirement: GraphQL endpoint configuration
The system SHALL read the Sui GraphQL endpoint from a `SUI_GRAPHQL_URL` environment variable (config field `sui_graphql_url`), defaulting to the official testnet GraphQL endpoint. The existing `SUI_RPC_URL` SHALL remain in place for the transaction-building path and SHALL NOT be used by the event or coin-metadata paths.

#### Scenario: Default endpoint
- **WHEN** `SUI_GRAPHQL_URL` is not set
- **THEN** the client uses `https://graphql.testnet.sui.io/graphql`

#### Scenario: Explicit endpoint
- **WHEN** `SUI_GRAPHQL_URL` is set (e.g. a provider endpoint for production)
- **THEN** all event and coin-metadata queries go to that URL, and `SUI_RPC_URL` continues to serve only the transaction-building path

### Requirement: Page size clamping and in-tick pagination
The client SHALL clamp requested page sizes to the GraphQL service's maximum page size, and the indexer SHALL continue fetching pages within a poll tick while `hasNextPage` is true, up to a bounded per-tick budget, so that a backlog larger than one page still drains.

#### Scenario: Requested limit exceeds service maximum
- **WHEN** a caller requests more events than the GraphQL service's maximum page size
- **THEN** the client requests the service maximum and reports pagination state so the caller can continue

#### Scenario: Backlog larger than one page
- **WHEN** more events are available than fit in a single page during one poll tick
- **THEN** the indexer fetches subsequent pages (respecting the per-tick budget), processing each page and persisting the cursor after each successfully processed page
