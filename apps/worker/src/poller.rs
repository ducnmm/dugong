use crate::backend_client::{BackendClient, TweetCreateEvent, WebhookPayload, WebhookUser};
use crate::config::Config;
use crate::twitter_client::TwitterClient;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

pub struct PollerService {
    twitter_client: TwitterClient,
    backend_client: BackendClient,
    config: Config,
    last_tweet_id: Arc<Mutex<Option<String>>>,
    last_poll_time: Arc<Mutex<Option<chrono::DateTime<chrono::Utc>>>>,
}

impl PollerService {
    pub fn new(config: Config) -> Self {
        let twitter_client = TwitterClient::new(config.twitterapi_io_api_key.clone());
        let backend_client = BackendClient::new(config.backend_url.clone());

        Self {
            twitter_client,
            backend_client,
            config,
            last_tweet_id: Arc::new(Mutex::new(None)),
            last_poll_time: Arc::new(Mutex::new(None)),
        }
    }

    /// Start polling for tweets
    pub async fn start(&self) -> Result<()> {
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("Starting Twitter Poller Service");
        info!("Mention: {}", self.config.twitter_mention);
        info!("Backend: {}", self.config.backend_url);
        info!("Poll Interval: {}s", self.config.poll_interval_seconds);
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        // Check backend health
        match self.backend_client.health_check().await {
            Ok(true) => info!("✓ Backend health check passed"),
            Ok(false) => warn!("⚠ Backend health check failed (non-200 response)"),
            Err(e) => warn!("⚠ Backend health check error: {}", e),
        }

        let interval = tokio::time::Duration::from_secs(self.config.poll_interval_seconds);
        let mut timer = tokio::time::interval(interval);

        loop {
            timer.tick().await;

            if let Err(e) = self.poll_and_send().await {
                error!("Error polling tweets: {:#}", e);
            }
        }
    }

    async fn poll_and_send(&self) -> Result<()> {
        let since_id = self.last_tweet_id.lock().await.clone();

        // Calculate start_time: current time - poll_interval - 5s buffer
        let now = chrono::Utc::now();
        let last_poll = self.last_poll_time.lock().await.clone();

        let start_time = if let Some(last) = last_poll {
            // Use last poll time minus 5s buffer to avoid missing tweets
            Some((last - chrono::Duration::seconds(5)).to_rfc3339())
        } else {
            // First run: get tweets from last (poll_interval + 5s)
            let lookback = self.config.poll_interval_seconds as i64 + 5;
            Some((now - chrono::Duration::seconds(lookback)).to_rfc3339())
        };

        info!(
            "Polling for tweets mentioning '{}' (since: {})",
            self.config.twitter_mention,
            start_time.as_ref().unwrap_or(&"beginning".to_string())
        );

        let response = self
            .twitter_client
            .search_mentions(
                &self.config.twitter_mention,
                since_id.as_deref(),
                start_time.as_deref(),
            )
            .await
            .context("Failed to search Twitter mentions")?;

        // Update last poll time
        *self.last_poll_time.lock().await = Some(now);

        let result_count = response.data.len();

        if result_count == 0 {
            info!("No new tweets found");
            return Ok(());
        }

        info!("Found {} new tweet(s)", result_count);

        // Get users from includes
        let users = response
            .includes
            .as_ref()
            .map(|inc| inc.users.clone())
            .unwrap_or_default();

        // Convert to webhook payload
        let mut events = Vec::new();

        for tweet in &response.data {
            let user = self.twitter_client.get_user_by_id(&tweet.author_id, &users);

            if let Some(user) = user {
                info!(
                    "  Tweet {} from @{}: {}",
                    tweet.id, user.username, tweet.text
                );

                events.push(TweetCreateEvent {
                    id_str: tweet.id.clone(),
                    text: tweet.text.clone(),
                    user: WebhookUser {
                        id_str: user.id.clone(),
                        screen_name: user.username.clone(),
                    },
                    in_reply_to_status_id_str: None,
                });
            } else {
                warn!("  Tweet {} has no user info", tweet.id);
            }
        }

        if !events.is_empty() {
            let payload = WebhookPayload {
                for_user_id: None,
                tweet_create_events: events,
            };

            info!(
                "Sending {} tweet(s) to backend...",
                payload.tweet_create_events.len()
            );

            match self.backend_client.send_tweets(payload).await {
                Ok(true) => info!("✓ Successfully sent tweets to backend"),
                Ok(false) => warn!("⚠ Backend returned non-success status"),
                Err(e) => error!("✗ Failed to send tweets to backend: {:#}", e),
            }
        }

        // Update last tweet ID
        if let Some(newest_id) = response.meta.and_then(|m| m.newest_id) {
            let mut last_id = self.last_tweet_id.lock().await;
            *last_id = Some(newest_id);
        }

        Ok(())
    }
}
