//! Sui GraphQL RPC client (replaces the retired JSON-RPC `suix_*` methods).
//!
//! Schema findings, verified live against `https://graphql.testnet.sui.io/graphql`
//! on 2026-07-13 (the endpoint serves the "beta" schema generation):
//!
//! - `EventFilter` input fields: `type`, `module`, `sender`, `afterCheckpoint`,
//!   `atCheckpoint`, `beforeCheckpoint`. `type` and `module` cannot be combined;
//!   `type` matches by PREFIX (`0xpkg`, `0xpkg::module`, or a full type), which
//!   has the same semantics as the old JSON-RPC `MoveEventModule` filter
//!   (module that *defines* the event type).
//! - Event nodes: `sequenceNumber` (position within the emitting transaction,
//!   same as JSON-RPC `eventSeq`), `timestamp` (ISO-8601 with millisecond
//!   precision), `transaction { digest effects { checkpoint { sequenceNumber } } }`,
//!   `contents { type { repr } json }`, `sender { address }`.
//! - Connections are Relay-style: `edges { cursor node }`, `nodes`,
//!   `pageInfo { hasNextPage endCursor }`. Cursors are opaque base64 strings,
//!   anchored to the endpoint's retention window — they can expire.
//! - Max page size for `Query.events` is 50
//!   (`serviceConfig.maxPageSize(type: "Query", field: "events")`).
//! - A malformed/expired `after` cursor yields a GraphQL `errors` entry like
//!   `Failed to parse "String": Invalid JSON` — surfaced here as [`CursorRejected`].
//! - `transaction(digest:)` with an unknown digest returns `data.transaction: null`.
//! - `coinMetadata(coinType:)` returns `null` when no CoinMetadata object exists.

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DUGONG_MODULE: &str = "events";

/// Hard cap the public Sui GraphQL service places on `Query.events` page size.
pub const MAX_EVENTS_PAGE_SIZE: u64 = 50;

/// The GraphQL service rejected the pagination cursor we sent (malformed for
/// this endpoint, or expired out of its retention window). Callers holding a
/// durable anchor (tx digest + event seq + checkpoint) should re-anchor and
/// retry instead of treating this as a transient failure.
#[derive(Debug, Clone)]
pub struct CursorRejected(pub String);

impl std::fmt::Display for CursorRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sui GraphQL rejected the pagination cursor: {}", self.0)
    }
}

impl std::error::Error for CursorRejected {}

const EVENTS_QUERY: &str = r#"
query Events($filter: EventFilter, $after: String, $first: Int) {
  events(filter: $filter, after: $after, first: $first) {
    edges {
      cursor
      node {
        sequenceNumber
        timestamp
        sender { address }
        transaction {
          digest
          effects { checkpoint { sequenceNumber } }
        }
        contents { type { repr } json }
      }
    }
    pageInfo { hasNextPage endCursor }
  }
}"#;

const COIN_METADATA_QUERY: &str = r#"
query CoinMetadata($coinType: String!) {
  coinMetadata(coinType: $coinType) {
    decimals
    name
    symbol
    description
    iconUrl
    address
  }
}"#;

const TRANSACTION_CHECKPOINT_QUERY: &str = r#"
query TransactionCheckpoint($digest: String!) {
  transaction(digest: $digest) {
    effects { checkpoint { sequenceNumber } }
  }
}"#;

#[derive(Clone)]
pub struct SuiClient {
    graphql_url: String,
    http: Client,
}

impl SuiClient {
    pub fn new(graphql_url: impl Into<String>) -> Self {
        Self {
            graphql_url: graphql_url.into(),
            http: Client::new(),
        }
    }

    /// POST a GraphQL query. Non-2xx responses and GraphQL `errors` entries are
    /// hard errors — never a silently empty result. Returns the `data` object.
    async fn execute(&self, query: &str, variables: Value) -> Result<Value> {
        let resp = self
            .http
            .post(&self.graphql_url)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .context("failed to call Sui GraphQL endpoint")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Sui GraphQL HTTP {}: {}", status, body));
        }

        let body: Value = resp
            .json()
            .await
            .context("failed to parse Sui GraphQL response json")?;

        if let Some(errors) = body.get("errors").and_then(Value::as_array) {
            if !errors.is_empty() {
                let messages: Vec<&str> = errors
                    .iter()
                    .filter_map(|e| e.get("message").and_then(Value::as_str))
                    .collect();
                return Err(anyhow!("Sui GraphQL error: {}", messages.join("; ")));
            }
        }

        body.get("data")
            .filter(|d| !d.is_null())
            .cloned()
            .ok_or_else(|| anyhow!("Sui GraphQL response has no data"))
    }

    /// Fetch coin metadata (decimals, symbol, name, etc.). `Ok(None)` when no
    /// CoinMetadata object exists for the coin type.
    pub async fn get_coin_metadata(&self, coin_type: &str) -> Result<Option<CoinMetadata>> {
        let data = self
            .execute(COIN_METADATA_QUERY, json!({ "coinType": coin_type }))
            .await?;

        let raw = &data["coinMetadata"];
        if raw.is_null() {
            return Ok(None);
        }

        let gql: GqlCoinMetadata = serde_json::from_value(raw.clone())
            .context("failed to parse coinMetadata response")?;
        let decimals = gql
            .decimals
            .context("coinMetadata is missing decimals")?;

        Ok(Some(CoinMetadata {
            decimals,
            name: gql.name.unwrap_or_default(),
            symbol: gql.symbol.unwrap_or_default(),
            description: gql.description,
            icon_url: gql.icon_url,
            id: gql.address,
        }))
    }

    /// Query events whose type is defined in `package_id::module`, ascending,
    /// resuming after the opaque GraphQL `cursor` when given.
    pub async fn query_events(
        &self,
        package_id: &str,
        module: &str,
        cursor: Option<&str>,
        limit: u64,
    ) -> Result<EventPage> {
        self.query_events_filtered(package_id, module, None, cursor, limit)
            .await
    }

    /// [`Self::query_events`] with an optional `afterCheckpoint` bound
    /// (checkpoints strictly greater than the value). Used for cursor
    /// re-anchoring, where paging must restart from a known checkpoint.
    pub async fn query_events_filtered(
        &self,
        package_id: &str,
        module: &str,
        after_checkpoint: Option<u64>,
        cursor: Option<&str>,
        limit: u64,
    ) -> Result<EventPage> {
        let mut filter = json!({ "type": format!("{}::{}", package_id, module) });
        if let Some(cp) = after_checkpoint {
            filter["afterCheckpoint"] = json!(cp);
        }

        let variables = json!({
            "filter": filter,
            "after": cursor,
            "first": clamp_page_size(limit),
        });

        let data = match self.execute(EVENTS_QUERY, variables).await {
            Ok(data) => data,
            Err(err) => {
                // The service reports an unparseable/expired `after` cursor as a
                // generic parse failure; there is no dedicated error code. We only
                // classify when we actually sent a cursor, so filter mistakes
                // (which we build ourselves) can't be mislabelled.
                let msg = err.to_string();
                if cursor.is_some() && msg.contains("Failed to parse") {
                    return Err(anyhow::Error::new(CursorRejected(msg)));
                }
                return Err(err);
            }
        };

        let connection: GqlEventConnection = serde_json::from_value(data["events"].clone())
            .context("failed to parse events response")?;

        let mut events = Vec::with_capacity(connection.edges.len());
        for edge in connection.edges {
            events.push(map_event(edge)?);
        }

        Ok(EventPage {
            data: events,
            next_cursor: connection.page_info.end_cursor,
            has_next_page: connection.page_info.has_next_page,
        })
    }

    /// The checkpoint sequence number a transaction was finalized in.
    /// `Ok(None)` means the endpoint does not know the digest — either it never
    /// existed or it is outside the endpoint's retention window; callers doing
    /// cursor re-anchoring must treat that as a hard stop, not "start over".
    pub async fn get_transaction_checkpoint(&self, digest: &str) -> Result<Option<u64>> {
        let data = self
            .execute(TRANSACTION_CHECKPOINT_QUERY, json!({ "digest": digest }))
            .await?;

        Ok(data
            .pointer("/transaction/effects/checkpoint/sequenceNumber")
            .and_then(Value::as_u64))
    }
}

fn clamp_page_size(limit: u64) -> u64 {
    limit.clamp(1, MAX_EVENTS_PAGE_SIZE)
}

/// Convert an ISO-8601 / RFC 3339 timestamp to epoch milliseconds.
fn iso8601_to_ms(timestamp: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.timestamp_millis())
        .and_then(|ms| u64::try_from(ms).ok())
}

fn map_event(edge: GqlEventEdge) -> Result<SuiEvent> {
    let node = edge.node;
    let tx_digest = node
        .transaction
        .as_ref()
        .map(|tx| tx.digest.clone())
        .context("event node is missing its transaction digest")?;
    let event_type = node
        .contents
        .as_ref()
        .and_then(|c| c.type_.as_ref())
        .map(|t| t.repr.clone())
        .context("event node is missing its type")?;

    Ok(SuiEvent {
        id: EventId {
            tx_digest,
            event_seq: node.sequence_number.to_string(),
        },
        package_id: None,
        transaction_module: None,
        sender: node.sender.map(|s| s.address),
        event_type,
        parsed_json: node.contents.and_then(|c| c.json),
        bcs: None,
        timestamp_ms: node
            .timestamp
            .as_deref()
            .and_then(iso8601_to_ms)
            .map(|ms| ms.to_string()),
        cursor: Some(edge.cursor),
        checkpoint: node
            .transaction
            .and_then(|tx| tx.effects)
            .and_then(|fx| fx.checkpoint)
            .map(|cp| cp.sequence_number),
    })
}

// ====== Public types (shape kept compatible with the JSON-RPC era so the ======
// ====== indexer's event processor and handlers are untouched)           ======

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinMetadata {
    pub decimals: u8,
    pub name: String,
    pub symbol: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPage {
    pub data: Vec<SuiEvent>,
    /// Opaque GraphQL cursor for the next page (`pageInfo.endCursor`).
    pub next_cursor: Option<String>,
    pub has_next_page: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiEvent {
    pub id: EventId,
    pub package_id: Option<String>,
    pub transaction_module: Option<String>,
    pub sender: Option<String>,
    #[serde(rename = "type")]
    pub event_type: String,
    pub parsed_json: Option<Value>,
    pub bcs: Option<String>,
    pub timestamp_ms: Option<String>,
    /// This event's own opaque GraphQL cursor (`edges.cursor`); resuming
    /// `after` it yields the events that follow this one.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Checkpoint the emitting transaction was finalized in.
    #[serde(default)]
    pub checkpoint: Option<u64>,
}

impl SuiEvent {
    #[allow(dead_code)]
    pub fn timestamp(&self) -> Option<u64> {
        self.timestamp_ms
            .as_ref()
            .and_then(|ts| ts.parse::<u64>().ok())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventId {
    pub tx_digest: String,
    pub event_seq: String,
}

impl EventId {
    /// Legacy JSON-RPC-era cursor encoding (`txDigest:eventSeq`). Still needed
    /// to recognize cursors persisted before the GraphQL migration.
    pub fn to_cursor(&self) -> String {
        format!("{}:{}", self.tx_digest, self.event_seq)
    }

    pub fn from_cursor_str(cursor: &str) -> Option<Self> {
        let (tx_digest, event_seq) = cursor.split_once(':')?;
        Some(Self {
            tx_digest: tx_digest.to_string(),
            event_seq: event_seq.to_string(),
        })
    }
}

// ====== GraphQL wire types ======

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlEventConnection {
    edges: Vec<GqlEventEdge>,
    page_info: GqlPageInfo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlPageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GqlEventEdge {
    cursor: String,
    node: GqlEventNode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlEventNode {
    sequence_number: u64,
    timestamp: Option<String>,
    sender: Option<GqlAddress>,
    transaction: Option<GqlTransaction>,
    contents: Option<GqlMoveValue>,
}

#[derive(Debug, Deserialize)]
struct GqlAddress {
    address: String,
}

#[derive(Debug, Deserialize)]
struct GqlTransaction {
    digest: String,
    effects: Option<GqlEffects>,
}

#[derive(Debug, Deserialize)]
struct GqlEffects {
    checkpoint: Option<GqlCheckpoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlCheckpoint {
    sequence_number: u64,
}

#[derive(Debug, Deserialize)]
struct GqlMoveValue {
    #[serde(rename = "type")]
    type_: Option<GqlMoveType>,
    json: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct GqlMoveType {
    repr: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlCoinMetadata {
    decimals: Option<u8>,
    name: Option<String>,
    symbol: Option<String>,
    description: Option<String>,
    icon_url: Option<String>,
    address: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_converts_to_epoch_ms() {
        assert_eq!(
            iso8601_to_ms("2026-07-13T12:42:55.432Z"),
            Some(1_783_946_575_432)
        );
        assert_eq!(iso8601_to_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso8601_to_ms("not-a-timestamp"), None);
    }

    #[test]
    fn page_size_is_clamped_to_service_max() {
        assert_eq!(clamp_page_size(1000), MAX_EVENTS_PAGE_SIZE);
        assert_eq!(clamp_page_size(100), MAX_EVENTS_PAGE_SIZE);
        assert_eq!(clamp_page_size(10), 10);
        assert_eq!(clamp_page_size(0), 1);
    }

    #[test]
    fn event_edge_maps_to_sui_event() {
        let edge: GqlEventEdge = serde_json::from_value(serde_json::json!({
            "cursor": "OPAQUE",
            "node": {
                "sequenceNumber": 3,
                "timestamp": "2026-07-13T12:42:55.432Z",
                "sender": { "address": "0xsender" },
                "transaction": {
                    "digest": "DIGEST1",
                    "effects": { "checkpoint": { "sequenceNumber": 42 } }
                },
                "contents": {
                    "type": { "repr": "0x9::events::AccountCreated" },
                    "json": { "xid": "1" }
                }
            }
        }))
        .expect("edge parses");

        let event = map_event(edge).expect("maps");
        assert_eq!(event.id.tx_digest, "DIGEST1");
        assert_eq!(event.id.event_seq, "3");
        assert_eq!(event.event_type, "0x9::events::AccountCreated");
        assert_eq!(event.sender.as_deref(), Some("0xsender"));
        assert_eq!(event.timestamp_ms.as_deref(), Some("1783946575432"));
        assert_eq!(event.cursor.as_deref(), Some("OPAQUE"));
        assert_eq!(event.checkpoint, Some(42));
        assert_eq!(event.parsed_json.unwrap()["xid"], "1");
    }

    #[test]
    fn event_missing_digest_is_an_error() {
        let edge: GqlEventEdge = serde_json::from_value(serde_json::json!({
            "cursor": "OPAQUE",
            "node": {
                "sequenceNumber": 0,
                "contents": { "type": { "repr": "0x9::events::X" }, "json": {} }
            }
        }))
        .expect("edge parses");
        assert!(map_event(edge).is_err());
    }
}
