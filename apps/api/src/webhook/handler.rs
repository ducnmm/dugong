use crate::clients::redis_client::RedisClient;
use crate::config::Config;
use crate::constants::{events, redis};
use crate::db::models::WebhookEvent;
use crate::error::{BackendError, Result};
use crate::webhook::signature::generate_crc_response;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
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
    Json(payload): Json<WebhookPayload>,
) -> StatusCode {
    let for_user_id = payload.for_user_id.unwrap_or_else(|| "unknown".to_string());
    info!("Received webhook for user: {}", for_user_id);

    for tweet in payload.tweet_create_events {
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
