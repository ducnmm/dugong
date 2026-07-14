use anyhow::{bail, ensure, Context, Result};
use tracing::{debug, info};

use dugong_core::clients::sui_client::{
    EventPage, SuiClient, DUGONG_MODULE, MAX_EVENTS_PAGE_SIZE,
};
use dugong_core::config::Config;

use crate::cursor::{CursorEnvelope, CURSOR_ENVELOPE_VERSION};

pub struct EventFetcher {
    client: SuiClient,
    // Event types are keyed by the package version that DEFINED them. Across an
    // upgrade, pre-existing events keep the ORIGINAL defining id while events
    // ADDED in the upgrade carry the upgraded id. The GraphQL `type` filter
    // (`<package>::events` prefix) matches a single defining id, so we watch
    // each id in turn. See Config::dugong_event_package_id.
    package_ids: Vec<String>,
}

impl EventFetcher {
    pub async fn new(config: Config) -> Result<Self> {
        let client = SuiClient::new(config.sui_graphql_url.clone());
        let package_ids = config.dugong_event_package_ids();
        ensure!(
            !package_ids.is_empty(),
            "DUGONG_EVENT_PACKAGE_ID must list at least one package id"
        );

        Ok(Self {
            client,
            package_ids,
        })
    }

    /// The defining package ids this fetcher watches, in configured order.
    pub fn package_ids(&self) -> &[String] {
        &self.package_ids
    }

    /// Fetch a page of events for one defining package id (cursor is per-package,
    /// since each event-type filter has its own event stream). `cursor` is the
    /// opaque GraphQL cursor to resume after.
    pub async fn fetch_events(
        &self,
        package_id: &str,
        cursor: Option<&str>,
        limit: u64,
    ) -> Result<EventPage> {
        debug!(
            "Fetching events for package {} with cursor: {:?}, limit: {}",
            package_id, cursor, limit
        );

        let page = self
            .client
            .query_events(package_id, DUGONG_MODULE, cursor, limit)
            .await
            .with_context(|| format!("Failed to query events for package {}", package_id))?;

        debug!(
            "Fetched {} events for package {}, has_next_page: {}",
            page.data.len(),
            package_id,
            page.has_next_page
        );

        Ok(page)
    }

    /// Derive a fresh cursor envelope from a durable anchor — the last event
    /// known to be processed, identified by `(tx_digest, event_seq)` and, when
    /// already known, the checkpoint it was finalized in.
    ///
    /// Used when the stored cursor is a legacy JSON-RPC `txDigest:eventSeq`
    /// value (no GraphQL cursor yet) or when the endpoint rejects a persisted
    /// GraphQL cursor (expired out of retention). Pages the same event filter
    /// from the anchor's checkpoint onward and adopts the anchor event's own
    /// GraphQL cursor, so resuming `after` it neither skips events that follow
    /// the anchor within its checkpoint nor re-processes the anchor itself.
    pub async fn re_anchor(
        &self,
        package_id: &str,
        tx_digest: &str,
        event_seq: &str,
        checkpoint: Option<u64>,
    ) -> Result<CursorEnvelope> {
        let cp = match checkpoint {
            Some(cp) => cp,
            None => self
                .client
                .get_transaction_checkpoint(tx_digest)
                .await
                .with_context(|| {
                    format!("failed to look up anchor transaction {tx_digest} for {package_id}")
                })?
                .with_context(|| out_of_range_message(package_id, tx_digest))?,
        };

        // `afterCheckpoint` is exclusive, so cp-1 starts paging AT the anchor's
        // checkpoint. cp == 0 pages from genesis (no checkpoint bound).
        let after_checkpoint = cp.checked_sub(1);
        let mut page_cursor: Option<String> = None;

        loop {
            let page = self
                .client
                .query_events_filtered(
                    package_id,
                    DUGONG_MODULE,
                    after_checkpoint,
                    page_cursor.as_deref(),
                    MAX_EVENTS_PAGE_SIZE,
                )
                .await
                .with_context(|| {
                    format!("failed to page events while re-anchoring {package_id}")
                })?;

            for event in &page.data {
                if event.id.tx_digest == tx_digest && event.id.event_seq == event_seq {
                    let gql = event.cursor.clone().context(
                        "event node is missing its GraphQL cursor; cannot re-anchor",
                    )?;
                    info!(
                        "Re-anchored cursor for {} at {}:{} (checkpoint {})",
                        package_id, tx_digest, event_seq, cp
                    );
                    return Ok(CursorEnvelope {
                        v: CURSOR_ENVELOPE_VERSION,
                        gql,
                        tx: tx_digest.to_string(),
                        seq: event_seq.to_string(),
                        cp,
                    });
                }
                // Events come back in ascending order; once we are past the
                // anchor's checkpoint it can no longer appear.
                if event.checkpoint.is_some_and(|event_cp| event_cp > cp) {
                    bail!(out_of_range_message(package_id, tx_digest));
                }
            }

            if !page.has_next_page {
                bail!(out_of_range_message(package_id, tx_digest));
            }
            page_cursor = page.next_cursor;
        }
    }
}

fn out_of_range_message(package_id: &str, tx_digest: &str) -> String {
    format!(
        "cannot re-anchor indexer cursor for package {package_id}: anchor transaction \
         {tx_digest} was not found on the configured Sui GraphQL endpoint (likely pruned \
         out of its retention window). Refusing to silently restart from genesis or from \
         the latest checkpoint. Remediation: point SUI_GRAPHQL_URL at a full-history \
         provider endpoint, or — if double/missed processing has been ruled out — \
         manually reset the cursor row in indexer_state."
    )
}
