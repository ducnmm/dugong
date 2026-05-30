use anyhow::{Context, Result};
use tracing::debug;

use dugong_core::clients::sui_client::{EventPage, SuiClient, DUGONG_MODULE};
use dugong_core::config::Config;

pub struct EventFetcher {
    client: SuiClient,
    package_id: String,
}

impl EventFetcher {
    pub async fn new(config: Config) -> Result<Self> {
        let client = SuiClient::new(config.sui_rpc_url);

        Ok(Self {
            client,
            // Event types keep their ORIGINAL (defining) package id across
            // upgrades, so the MoveEventModule filter must use the original id,
            // not the latest move-call id. See Config::dugong_event_package_id.
            package_id: config.dugong_event_package_id,
        })
    }

    /// Fetch events using cursor-based pagination
    pub async fn fetch_events(&self, cursor: Option<&str>, limit: u64) -> Result<EventPage> {
        debug!(
            "Fetching events with cursor: {:?}, limit: {}",
            cursor, limit
        );

        // Query events from Sui using the existing client
        let page = self
            .client
            .query_events(&self.package_id, DUGONG_MODULE, cursor, limit)
            .await
            .context("Failed to query events from Sui")?;

        debug!(
            "Fetched {} events, has_next_page: {}",
            page.data.len(),
            page.has_next_page
        );

        Ok(page)
    }
}
