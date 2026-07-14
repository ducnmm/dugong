use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::debug;

use dugong_core::clients::sui_client::EventId;
use dugong_core::db::models::IndexerState;

/// Cursor format version written by the GraphQL indexer.
pub const CURSOR_ENVELOPE_VERSION: u32 = 2;

/// Persisted per-package cursor. The opaque GraphQL cursor (`gql`) is
/// endpoint-specific and can expire out of the endpoint's retention window,
/// so the envelope also carries a durable anchor — the last *processed*
/// event's transaction digest (`tx`), its sequence within that transaction
/// (`seq`), and the checkpoint it was finalized in (`cp`) — from which a
/// fresh cursor can always be re-derived (see `EventFetcher::re_anchor`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorEnvelope {
    pub v: u32,
    pub gql: String,
    pub tx: String,
    pub seq: String,
    pub cp: u64,
}

impl CursorEnvelope {
    pub fn to_stored(&self) -> String {
        serde_json::to_string(self).expect("cursor envelope serializes")
    }
}

/// A cursor string as loaded from `indexer_state`, classified by format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredCursor {
    /// No cursor stored — start from genesis.
    Genesis,
    /// GraphQL-era envelope.
    Envelope(CursorEnvelope),
    /// JSON-RPC-era `txDigest:eventSeq` cursor; must be re-anchored before use.
    Legacy { tx_digest: String, event_seq: String },
}

impl StoredCursor {
    /// Classify a stored cursor string. Unrecognizable values are a hard error:
    /// silently ignoring one would rescan from genesis and double-process
    /// increment-style balance events.
    pub fn parse(raw: Option<&str>) -> Result<Self> {
        let Some(raw) = raw else {
            return Ok(Self::Genesis);
        };
        // JSON-looking input must be an envelope; falling through would misread
        // it as legacy (JSON objects contain `:` too).
        if raw.trim_start().starts_with('{') {
            let envelope = serde_json::from_str::<CursorEnvelope>(raw)
                .with_context(|| format!("invalid cursor envelope: {raw:?}"))?;
            return Ok(Self::Envelope(envelope));
        }
        if let Some(id) = EventId::from_cursor_str(raw) {
            return Ok(Self::Legacy {
                tx_digest: id.tx_digest,
                event_seq: id.event_seq,
            });
        }
        bail!(
            "unrecognized indexer cursor format: {:?} — expected a JSON envelope \
             or a legacy txDigest:eventSeq string; refusing to rescan from genesis",
            raw
        );
    }
}

pub struct CursorManager {
    pool: PgPool,
}

impl CursorManager {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Load cursor from database
    pub async fn load_cursor(&self, name: &str) -> Result<Option<String>> {
        let state = IndexerState::get_by_name(&self.pool, name)
            .await
            .context("Failed to load cursor from database")?;

        match state {
            Some(s) => {
                debug!("Loaded cursor for {}: {:?}", name, s.cursor);
                Ok(s.cursor)
            }
            None => {
                debug!("No cursor found for {}, starting from genesis", name);
                Ok(None)
            }
        }
    }

    /// Save cursor to database
    pub async fn save_cursor(&self, name: &str, cursor: Option<&String>) -> Result<()> {
        let cursor_str = cursor.map(|s| s.as_str());

        IndexerState::upsert_cursor(&self.pool, name, cursor_str)
            .await
            .context("Failed to save cursor to database")?;

        debug!("Saved cursor for {}: {:?}", name, cursor);
        Ok(())
    }

    /// Reset cursor (start from genesis)
    #[allow(dead_code)]
    pub async fn reset_cursor(&self, name: &str) -> Result<()> {
        IndexerState::upsert_cursor(&self.pool, name, None)
            .await
            .context("Failed to reset cursor")?;

        debug!("Reset cursor for {}", name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope() -> CursorEnvelope {
        CursorEnvelope {
            v: CURSOR_ENVELOPE_VERSION,
            gql: "eyJ0IjozLCJlIjowfQ==".to_string(),
            tx: "DIGEST1".to_string(),
            seq: "3".to_string(),
            cp: 42,
        }
    }

    #[test]
    fn envelope_round_trips_through_storage() {
        let stored = envelope().to_stored();
        assert_eq!(
            StoredCursor::parse(Some(&stored)).unwrap(),
            StoredCursor::Envelope(envelope())
        );
    }

    #[test]
    fn missing_cursor_is_genesis() {
        assert_eq!(StoredCursor::parse(None).unwrap(), StoredCursor::Genesis);
    }

    #[test]
    fn legacy_cursor_is_classified() {
        assert_eq!(
            StoredCursor::parse(Some("DIGEST1:3")).unwrap(),
            StoredCursor::Legacy {
                tx_digest: "DIGEST1".to_string(),
                event_seq: "3".to_string(),
            }
        );
    }

    #[test]
    fn unrecognizable_cursor_is_a_hard_error() {
        assert!(StoredCursor::parse(Some("garbage-with-no-separator")).is_err());
        // Valid JSON but not an envelope must not be misread as legacy.
        assert!(StoredCursor::parse(Some("{\"other\": 1}")).is_err());
    }
}
