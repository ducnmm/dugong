use anyhow::Result;
use tokio::time::{interval, Duration};
use tracing::info;

use dugong_core::config::Config;

use crate::cursor::CursorManager;
use crate::event_fetcher::EventFetcher;
use crate::event_processor::EventProcessor;
use crate::types::EventPage;

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

/// One watched event stream: a defining package id, its cursor state-row name, and
/// the in-memory cursor advanced as events are processed.
struct PackageCursor {
    package_id: String,
    state_name: String,
    cursor: Option<String>,
}

pub struct Indexer {
    event_fetcher: EventFetcher,
    event_processor: EventProcessor,
    cursor_manager: CursorManager,
    poll_interval: Duration,
}

impl Indexer {
    pub async fn new(config: Config, pool: sqlx::PgPool) -> Result<Self> {
        let event_fetcher = EventFetcher::new(config.clone()).await?;
        let event_processor = EventProcessor::new(pool.clone());
        let cursor_manager = CursorManager::new(pool.clone());
        let poll_interval = Duration::from_millis(config.indexer_poll_interval_ms);

        Ok(Self {
            event_fetcher,
            event_processor,
            cursor_manager,
            poll_interval,
        })
    }

    /// Start the indexer in real-time mode
    pub async fn start(&self) -> Result<()> {
        info!("Starting Dugong Indexer");

        // Load the last cursor for each watched package.
        let mut streams: Vec<PackageCursor> = Vec::new();
        for (index, package_id) in self.event_fetcher.package_ids().iter().enumerate() {
            let state_name = cursor_state_name(index, package_id);
            let cursor = self.cursor_manager.load_cursor(&state_name).await?;
            info!(
                "Watching {} from cursor: {:?} (state {})",
                package_id, cursor, state_name
            );
            streams.push(PackageCursor {
                package_id: package_id.clone(),
                state_name,
                cursor,
            });
        }

        let mut ticker = interval(self.poll_interval);

        loop {
            ticker.tick().await;

            for stream in streams.iter_mut() {
                match self
                    .fetch_and_process_events(&stream.package_id, &mut stream.cursor)
                    .await
                {
                    Ok(processed) => {
                        if processed > 0 {
                            info!("Processed {} events ({})", processed, stream.package_id);
                            // Save cursor after processing
                            self.cursor_manager
                                .save_cursor(&stream.state_name, stream.cursor.as_ref())
                                .await?;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error processing events for {}: {}", stream.package_id, e);
                        // Continue with the next package / next tick
                    }
                }
            }
        }
    }

    async fn fetch_and_process_events(
        &self,
        package_id: &str,
        cursor: &mut Option<String>,
    ) -> Result<usize> {
        // Fetch events
        let page: EventPage = self
            .event_fetcher
            .fetch_events(package_id, cursor.as_deref(), 100)
            .await?;

        if page.data.is_empty() {
            return Ok(0);
        }

        info!("Fetched {} events ({})", page.data.len(), package_id);

        // Process events
        let processed = self.event_processor.process_events(&page.data).await?;

        // Update cursor from last event
        *cursor = page.next_cursor.map(|c| c.to_cursor());

        Ok(processed)
    }

    /// Sync all historical events from genesis (across every watched package).
    #[allow(dead_code)]
    pub async fn sync_historical(&self) -> Result<()> {
        info!("Starting historical sync");

        let mut total_processed = 0;

        for (index, package_id) in self.event_fetcher.package_ids().iter().enumerate() {
            let state_name = cursor_state_name(index, package_id);
            let mut cursor: Option<String> = None;

            loop {
                let page = self
                    .event_fetcher
                    .fetch_events(package_id, cursor.as_deref(), 1000)
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

                // Update cursor
                cursor = page.next_cursor.map(|c| c.to_cursor());

                // Save checkpoint periodically
                self.cursor_manager
                    .save_cursor(&state_name, cursor.as_ref())
                    .await?;

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
