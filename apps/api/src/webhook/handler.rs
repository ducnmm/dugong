use crate::error::{BackendError, Result};
use crate::webhook::signature::generate_crc_response;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
};
use dugong_core::clients::redis_client::RedisClient;
use dugong_core::config::Config;
use dugong_core::constants::{events, redis};
use dugong_core::db::models::WebhookEvent;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub redis: RedisClient,
}

#[derive(Deserialize)]
pub struct CrcParams {
    crc_token: String,
}

#[derive(Serialize)]
pub struct CrcResponse {
    response_token: String,
}

#[derive(Debug, Deserialize)]
pub struct WebhookPayload {
    #[serde(default)]
    pub tweet_create_events: Vec<TweetEvent>,
    #[serde(default)]
    pub for_user_id: Option<String>,
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub rule_tag: Option<String>,
    #[serde(default)]
    pub tweets: Vec<TwitterApiIoTweet>,
}

#[derive(Debug, Deserialize)]
pub struct TweetEvent {
    pub id_str: String,
    pub text: String,
    pub user: User,
    #[serde(default)]
    pub in_reply_to_status_id_str: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id_str: String,
    pub screen_name: String,
}

#[derive(Debug, Deserialize)]
pub struct TwitterApiIoTweet {
    pub id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub author: Option<TwitterApiIoAuthor>,
    #[serde(
        default,
        rename = "inReplyToId",
        alias = "in_reply_to_id",
        alias = "in_reply_to_status_id_str"
    )]
    pub in_reply_to_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct TwitterApiIoAuthor {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default, alias = "username", alias = "userName", alias = "screen_name")]
    pub user_name: Option<String>,
}

impl TwitterApiIoTweet {
    fn into_tweet_event(self) -> Option<TweetEvent> {
        let id_str = self.id.trim().to_string();
        if id_str.is_empty() {
            return None;
        }

        let author = self.author.unwrap_or_default();
        Some(TweetEvent {
            id_str,
            text: self.text,
            user: User {
                id_str: author.id.unwrap_or_else(|| "unknown".to_string()),
                screen_name: author.user_name.unwrap_or_else(|| "unknown".to_string()),
            },
            in_reply_to_status_id_str: self.in_reply_to_id,
        })
    }
}

impl WebhookPayload {
    fn into_events(self) -> (String, Vec<TweetEvent>) {
        let source = self
            .for_user_id
            .or(self.rule_tag)
            .or(self.event_type)
            .unwrap_or_else(|| "unknown".to_string());

        if !self.tweet_create_events.is_empty() {
            return (source, self.tweet_create_events);
        }

        let tweets = self
            .tweets
            .into_iter()
            .filter_map(TwitterApiIoTweet::into_tweet_event)
            .collect();
        (source, tweets)
    }
}

pub async fn handle_crc_challenge(
    Query(params): Query<CrcParams>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<CrcResponse>> {
    info!("Received CRC challenge: {}", params.crc_token);

    let webhook_secret = state
        .config
        .twitter_webhook_secret
        .as_ref()
        .ok_or_else(|| {
            BackendError::Config(
                "TWITTER_WEBHOOK_SECRET must be set to answer X CRC challenges".to_string(),
            )
        })?;

    let response_token = generate_crc_response(&params.crc_token, webhook_secret)
        .map_err(BackendError::WebhookValidation)?;

    info!("CRC challenge passed: {}", response_token);

    Ok(Json(CrcResponse { response_token }))
}

pub async fn handle_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<WebhookPayload>,
) -> StatusCode {
    if !payload.tweets.is_empty() {
        let valid_api_key = headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
            .map(|value| value == state.config.twitterapi_io_api_key)
            .unwrap_or(false);
        if !valid_api_key {
            warn!("Rejecting TwitterAPI.io webhook with missing or invalid X-API-Key");
            return StatusCode::UNAUTHORIZED;
        }
    }

    let (for_user_id, tweets) = payload.into_events();
    info!("Received webhook for user: {}", for_user_id);

    if tweets.is_empty() {
        warn!("Webhook payload did not contain any supported tweet events");
        return StatusCode::OK;
    }

    for tweet in tweets {
        let event_id = events::tweet_event_id(&tweet.id_str);

        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("Tweet ID: {}", tweet.id_str);
        info!("From: @{} ({})", tweet.user.screen_name, tweet.user.id_str);
        info!("Text: {}", tweet.text);

        // Check deduplication in Redis
        let dedup_key = redis::dedup_tweet(&tweet.id_str);
        match state.redis.check_dedup(&dedup_key).await {
            Ok(exists) if exists => {
                info!("Tweet {} already processed (dedup)", tweet.id_str);
                continue;
            }
            Err(e) => {
                warn!("Redis dedup check failed: {}", e);
            }
            _ => {}
        }

        // Check if event already exists in DB
        match WebhookEvent::exists(&state.db, &event_id).await {
            Ok(true) => {
                info!("Event {} already exists in DB", event_id);
                continue;
            }
            Err(e) => {
                warn!("DB exists check failed: {}", e);
                continue;
            }
            _ => {}
        }

        // Store in database
        let payload_json = serde_json::json!({
            "tweet_id": tweet.id_str,
            "user_id": tweet.user.id_str,
            "screen_name": tweet.user.screen_name,
            "text": tweet.text,
            "in_reply_to": tweet.in_reply_to_status_id_str,
        });

        match WebhookEvent::create(&state.db, &event_id, Some(&tweet.id_str), payload_json).await {
            Ok(_) => {
                info!("Stored event {} in DB", event_id);
            }
            Err(e) => {
                warn!("Failed to store event in DB: {}", e);
                continue;
            }
        }

        // Set deduplication key (24h TTL)
        if let Err(e) = state.redis.set_dedup(&dedup_key, redis::TTL_DEDUP).await {
            warn!("Failed to set dedup key: {}", e);
        }

        // Push to processing queue
        let queue_item = serde_json::json!({
            "tweet_id": tweet.id_str,
            "event_id": event_id,
        });

        match state
            .redis
            .push_queue(redis::QUEUE_TWEETS, &queue_item.to_string())
            .await
        {
            Ok(_) => {
                info!("Pushed tweet {} to queue", tweet.id_str);
            }
            Err(e) => {
                warn!("Failed to push to queue: {}", e);
            }
        }

        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    }

    StatusCode::OK
}

pub async fn health_check() -> &'static str {
    "OK"
}
