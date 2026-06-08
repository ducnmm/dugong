use crate::backend_client::{BackendClient, TweetCreateEvent, WebhookPayload, WebhookUser};
use crate::config::Config;
use crate::twitter_client::{TweetData, TwitterClient, TwitterUser};
use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

const MAX_TWEETS_PER_POLL: usize = 1;

/// Convert searched tweets into webhook `TweetCreateEvent`s, pairing each
/// tweet with its author from `users`. Tweets whose author is missing from
/// `users` are dropped (the backend needs the screen name).
pub fn tweets_to_events(data: &[TweetData], users: &[TwitterUser]) -> Vec<TweetCreateEvent> {
    data.iter()
        .filter_map(|tweet| {
            let user = users.iter().find(|u| u.id == tweet.author_id)?;
            Some(TweetCreateEvent {
                id_str: tweet.id.clone(),
                text: tweet.text.clone(),
                user: WebhookUser {
                    id_str: user.id.clone(),
                    screen_name: user.username.clone(),
                },
                in_reply_to_status_id_str: None,
            })
        })
        .collect()
}

/// Pick the bounded batch to send for a single poll. We process the oldest
/// tweet first so a backlog drains across later polls instead of being skipped
/// by advancing `last_tweet_id` to the newest result immediately.
pub fn select_events_for_poll(mut events: Vec<TweetCreateEvent>) -> Vec<TweetCreateEvent> {
    sort_events_for_poll(&mut events);
    events.truncate(MAX_TWEETS_PER_POLL);
    events
}

pub fn split_events_for_poll(
    mut events: Vec<TweetCreateEvent>,
) -> (Vec<TweetCreateEvent>, VecDeque<TweetCreateEvent>) {
    sort_events_for_poll(&mut events);
    let queued = if events.len() > MAX_TWEETS_PER_POLL {
        events.split_off(MAX_TWEETS_PER_POLL)
    } else {
        Vec::new()
    };

    (events, queued.into_iter().collect())
}

fn sort_events_for_poll(events: &mut [TweetCreateEvent]) {
    events.sort_by(
        |a, b| match (a.id_str.parse::<u128>(), b.id_str.parse::<u128>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            _ => a.id_str.cmp(&b.id_str),
        },
    );
}

pub struct PollerService {
    twitter_client: TwitterClient,
    backend_client: BackendClient,
    config: Config,
    last_tweet_id: Arc<Mutex<Option<String>>>,
    last_poll_time: Arc<Mutex<Option<chrono::DateTime<chrono::Utc>>>>,
    pending_events: Arc<Mutex<VecDeque<TweetCreateEvent>>>,
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
            pending_events: Arc::new(Mutex::new(VecDeque::new())),
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
        if let Some(event) = self.pending_events.lock().await.pop_front() {
            info!("Processing 1 queued tweet from previous poll");
            let processed_tweet_id = self.send_events_to_backend(vec![event]).await?;

            if let Some(newest_id) = processed_tweet_id {
                let mut last_id = self.last_tweet_id.lock().await;
                *last_id = Some(newest_id);
            }

            return Ok(());
        }

        let since_id = self.last_tweet_id.lock().await.clone();

        // Calculate start_time: current time - poll_interval - 5s buffer
        let now = chrono::Utc::now();
        let last_poll = *self.last_poll_time.lock().await;

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
        let fallback_newest_id = response.meta.as_ref().and_then(|m| m.newest_id.clone());

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

        // Convert to webhook payload and cap this poll to one tweet. Any
        // additional matches stay in memory and are drained one-per-poll
        // without making another TwitterAPI.io call.
        let (events, queued_events) =
            split_events_for_poll(tweets_to_events(&response.data, &users));
        if !queued_events.is_empty() {
            let queued_count = queued_events.len();
            self.pending_events.lock().await.extend(queued_events);
            info!("Queued {} tweet(s) for later polls", queued_count);
        }
        let processed_tweet_id = self.send_events_to_backend(events).await?;

        // Update only to the tweet actually sent so remaining results are
        // picked up one-per-poll. If every result was unusable, fall back to
        // newest_id to avoid looping forever on tweets without author data.
        if let Some(newest_id) = processed_tweet_id.or(fallback_newest_id) {
            let mut last_id = self.last_tweet_id.lock().await;
            *last_id = Some(newest_id);
        }

        Ok(())
    }

    async fn send_events_to_backend(
        &self,
        events: Vec<TweetCreateEvent>,
    ) -> Result<Option<String>> {
        let processed_tweet_id = events.last().map(|event| event.id_str.clone());

        if events.is_empty() {
            return Ok(None);
        }

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

        Ok(processed_tweet_id)
    }
}
