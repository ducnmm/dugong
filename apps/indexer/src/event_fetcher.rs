use anyhow::{ensure, Context, Result};
use tracing::debug;

use dugong_core::clients::sui_client::{EventPage, SuiClient, DUGONG_MODULE};
use dugong_core::config::Config;

pub struct EventFetcher {
    client: SuiClient,
    // Event types are keyed by the package version that DEFINED them. Across an
    // upgrade, pre-existing events keep the ORIGINAL defining id while events
    // ADDED in the upgrade carry the upgraded id. A `MoveEventModule` filter
    // matches a single defining id, so we watch each id in turn. See
    // Config::dugong_event_package_id.
    package_ids: Vec<String>,
}

impl EventFetcher {
    pub async fn new(config: Config) -> Result<Self> {
        let client = SuiClient::new(config.sui_rpc_url.clone());
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
    /// since each `MoveEventModule` filter has its own event stream).
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
}
