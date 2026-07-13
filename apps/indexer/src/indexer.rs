use anyhow::{Context, Result};
use tokio::time::{interval, Duration};
use tracing::{info, warn};

use dugong_core::clients::sui_client::{CursorRejected, MAX_EVENTS_PAGE_SIZE};
use dugong_core::config::Config;

use crate::cursor::{CursorEnvelope, CursorManager, StoredCursor, CURSOR_ENVELOPE_VERSION};
use crate::event_fetcher::EventFetcher;
use crate::event_processor::EventProcessor;
use crate::types::EventPage;

/// Upper bound on pages fetched per package per poll tick. The GraphQL service
/// caps pages at `MAX_EVENTS_PAGE_SIZE` (50) events, so this bounds a tick at
/// 500 events per package while still letting a backlog drain across ticks.
const MAX_PAGES_PER_TICK: usize = 10;

/// Cursor state-row name for a watched package. The first (primary) package keeps
/// the legacy `dugong_events` name so existing deployments resume from their saved
/// cursor instead of re-scanning history (re-processing increment-style balance
/// events would double-count). Additional packages get their own namespaced row
/// and start from genesis — which for an upgrade package only replays the newly
/// added (idempotent) events.
fn cursor_state_name(index: usize, package_id: &str) -> String {
    if index == 0 {
        "dugong_events".to_string()
    } else {
        format!("dugong_events:{}", package_id)
    }
}

/// In-memory cursor for one watched event stream.
enum CursorState {
    /// No cursor yet — fetch from genesis.
    Genesis,
    /// Normal operation: resume after the envelope's GraphQL cursor.
    Envelope(CursorEnvelope),
    /// Pre-migration JSON-RPC cursor; re-anchored on the first fetch.
    Legacy { tx_digest: String, event_seq: String },
}

/// One watched event stream: a defining package id, its cursor state-row name,
/// and the cursor advanced as events are processed.
struct PackageCursor {
    package_id: String,
    state_name: String,
    cursor: CursorState,
}

pub struct Indexer {
    event_fetcher: EventFetcher,
    event_processor: EventProcessor,
    cursor_manager: CursorManager,
    poll_interval: Duration,
    streams: Vec<PackageCursor>,
}

impl Indexer {
    pub async fn new(config: Config, pool: sqlx::PgPool) -> Result<Self> {
        let event_fetcher = EventFetcher::new(config.clone()).await?;
        let event_processor = EventProcessor::new(pool.clone());
        let cursor_manager = CursorManager::new(pool.clone());
        let poll_interval = Duration::from_millis(config.indexer_poll_interval_ms);

        // Load and classify the last cursor for each watched package. An
        // unrecognizable cursor is a startup error (see StoredCursor::parse).
        let mut streams: Vec<PackageCursor> = Vec::new();
        for (index, package_id) in event_fetcher.package_ids().iter().enumerate() {
            let state_name = cursor_state_name(index, package_id);
            let raw = cursor_manager.load_cursor(&state_name).await?;
            let cursor = match StoredCursor::parse(raw.as_deref())
                .with_context(|| format!("loading cursor {state_name}"))?
            {
                StoredCursor::Genesis => CursorState::Genesis,
                StoredCursor::Envelope(envelope) => CursorState::Envelope(envelope),
                StoredCursor::Legacy {
                    tx_digest,
                    event_seq,
                } => {
                    info!(
                        "Found legacy JSON-RPC cursor for {} ({}:{}); will re-anchor on first fetch",
                        package_id, tx_digest, event_seq
                    );
                    CursorState::Legacy {
                        tx_digest,
                        event_seq,
                    }
                }
            };
            info!("Watching {} (state {})", package_id, state_name);
            streams.push(PackageCursor {
                package_id: package_id.clone(),
                state_name,
                cursor,
            });
        }

        Ok(Self {
            event_fetcher,
            event_processor,
            cursor_manager,
            poll_interval,
            streams,
        })
    }

    /// Start the indexer in real-time mode
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Dugong Indexer");

        let mut ticker = interval(self.poll_interval);

        loop {
            ticker.tick().await;
            self.run_once().await;
        }
    }

    /// One poll pass over every watched package. Errors are logged per package
    /// and never advance that package's cursor; the next tick retries.
    pub async fn run_once(&mut self) -> usize {
        let mut total = 0;
        for index in 0..self.streams.len() {
            match self.poll_package(index).await {
                Ok(processed) => {
                    if processed > 0 {
                        info!(
                            "Processed {} events ({})",
                            processed, self.streams[index].package_id
                        );
                        total += processed;
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Error processing events for {}: {:#}",
                        self.streams[index].package_id,
                        e
                    );
                    // Continue with the next package / next tick
                }
            }
        }
        total
    }

    /// Fetch, process, and persist for one package: re-anchor the cursor if
    /// needed, then page through the backlog (bounded per tick), persisting the
    /// cursor envelope only after each page's events are processed — a failure
    /// leaves the previous cursor in place so the page is re-fetched.
    async fn poll_package(&mut self, index: usize) -> Result<usize> {
        // Legacy cursors are re-anchored once, before any fetch, and the fresh
        // envelope is persisted immediately so a restart doesn't redo the scan.
        let legacy = match &self.streams[index].cursor {
            CursorState::Legacy {
                tx_digest,
                event_seq,
            } => Some((tx_digest.clone(), event_seq.clone())),
            _ => None,
        };
        if let Some((tx_digest, event_seq)) = legacy {
            let package_id = self.streams[index].package_id.clone();
            let envelope = self
                .event_fetcher
                .re_anchor(&package_id, &tx_digest, &event_seq, None)
                .await?;
            self.save_envelope(index, envelope).await?;
        }

        let mut processed = 0;
        for _ in 0..MAX_PAGES_PER_TICK {
            let after = match &self.streams[index].cursor {
                CursorState::Envelope(envelope) => Some(envelope.gql.clone()),
                CursorState::Genesis => None,
                CursorState::Legacy { .. } => unreachable!("re-anchored above"),
            };

            let page = match self
                .event_fetcher
                .fetch_events(
                    &self.streams[index].package_id,
                    after.as_deref(),
                    MAX_EVENTS_PAGE_SIZE,
                )
                .await
            {
                Ok(page) => page,
                // A rejected cursor (expired out of the endpoint's retention
                // window) is recovered by re-anchoring from the envelope's
                // durable (tx, seq, cp) anchor, then retrying next tick.
                Err(err) if err.chain().any(|c| c.is::<CursorRejected>()) => {
                    let (tx, seq, cp) = match &self.streams[index].cursor {
                        CursorState::Envelope(envelope) => {
                            (envelope.tx.clone(), envelope.seq.clone(), envelope.cp)
                        }
                        _ => return Err(err),
                    };
                    let package_id = self.streams[index].package_id.clone();
                    warn!(
                        "Stored GraphQL cursor rejected for {}; re-anchoring from {}:{} (checkpoint {})",
                        package_id, tx, seq, cp
                    );
                    let fresh = self
                        .event_fetcher
                        .re_anchor(&package_id, &tx, &seq, Some(cp))
                        .await?;
                    self.save_envelope(index, fresh).await?;
                    continue;
                }
                Err(err) => return Err(err),
            };

            if page.data.is_empty() {
                break;
            }

            // Process the page; an Err here propagates without touching the
            // persisted cursor, so these events are re-fetched next tick.
            processed += self.event_processor.process_events(&page.data).await?;

            let envelope = envelope_from_page(&page, &self.streams[index].cursor)?;
            self.save_envelope(index, envelope).await?;

            if !page.has_next_page {
                break;
            }
        }

        Ok(processed)
    }

    async fn save_envelope(&mut self, index: usize, envelope: CursorEnvelope) -> Result<()> {
        self.cursor_manager
            .save_cursor(&self.streams[index].state_name, Some(&envelope.to_stored()))
            .await?;
        self.streams[index].cursor = CursorState::Envelope(envelope);
        Ok(())
    }

    /// Sync all historical events from genesis (across every watched package).
    ///
    /// NOTE: public GraphQL endpoints retain a bounded history window; a
    /// genesis backfill needs a full-history provider endpoint in
    /// SUI_GRAPHQL_URL.
    #[allow(dead_code)]
    pub async fn sync_historical(&mut self) -> Result<()> {
        info!("Starting historical sync");

        let mut total_processed = 0;

        for index in 0..self.streams.len() {
            let package_id = self.streams[index].package_id.clone();
            let mut cursor: Option<String> = None;

            loop {
                let page = self
                    .event_fetcher
                    .fetch_events(&package_id, cursor.as_deref(), MAX_EVENTS_PAGE_SIZE)
                    .await?;

                if page.data.is_empty() {
                    break;
                }

                let processed = self.event_processor.process_events(&page.data).await?;
                total_processed += processed;

                info!(
                    "Historical sync progress ({}): {} events processed",
                    package_id, total_processed
                );

                cursor = page.next_cursor.clone();
                let envelope = envelope_from_page(&page, &CursorState::Genesis)?;
                self.save_envelope(index, envelope).await?;

                if page.has_next_page {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                } else {
                    break;
                }
            }
        }

        info!(
            "Historical sync complete: {} events processed",
            total_processed
        );
        Ok(())
    }
}

/// Build the cursor envelope persisted after a successfully processed page:
/// the page's end cursor plus the last event as the durable re-anchor point.
fn envelope_from_page(page: &EventPage, previous: &CursorState) -> Result<CursorEnvelope> {
    let last = page
        .data
        .last()
        .context("cannot build a cursor envelope from an empty page")?;
    let gql = page
        .next_cursor
        .clone()
        .or_else(|| last.cursor.clone())
        .context("event page has neither an end cursor nor per-event cursors")?;
    // The service omits per-event checkpoints only in pathological responses;
    // fall back to the previous envelope's checkpoint rather than failing the
    // whole page (the checkpoint is only used for re-anchoring).
    let cp = last
        .checkpoint
        .or(match previous {
            CursorState::Envelope(envelope) => Some(envelope.cp),
            _ => None,
        })
        .context("event page is missing checkpoint information")?;

    Ok(CursorEnvelope {
        v: CURSOR_ENVELOPE_VERSION,
        gql,
        tx: last.id.tx_digest.clone(),
        seq: last.id.event_seq.clone(),
        cp,
    })
}
