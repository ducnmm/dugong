//! Integration tests for the `/webhook` routes.
//!
//! The router is driven in-process with `tower::ServiceExt::oneshot` (no TCP
//! listener). Database state uses `#[sqlx::test]`; the dedup/queue path needs
//! a live Redis (see common::test_redis_url). Tests that need Redis skip
//! gracefully if it is unreachable.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{app_state, drain_queue, lock_queue, test_config, try_redis};
use dugong_api::build_router;
use dugong_core::constants::{events, redis};
use dugong_core::db::models::WebhookEvent;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

/// Unique tweet id per test invocation so Redis dedup keys never collide
/// across test runs against a shared Redis.
fn unique_tweet_id(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{tag}-{nanos}")
}

#[sqlx::test(migrations = "../core/migrations")]
async fn crc_challenge_returns_signed_token(pool: PgPool) {
    let Some(redis) = try_redis().await else {
        eprintln!("skipping: Redis unreachable at {}", common::test_redis_url());
        return;
    };
    let state = app_state(test_config(), pool, redis);
    let app = build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/webhook?crc_token=challenge-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let token = body["response_token"].as_str().expect("response_token");
    assert!(token.starts_with("sha256="), "unexpected token: {token}");
}

#[sqlx::test(migrations = "../core/migrations")]
async fn webhook_enqueues_new_tweet(pool: PgPool) {
    let Some(redis) = try_redis().await else {
        eprintln!("skipping: Redis unreachable at {}", common::test_redis_url());
        return;
    };
    let _guard = lock_queue().await;
    drain_queue(&redis).await;

    let tweet_id = unique_tweet_id("enqueue");
    let state = app_state(test_config(), pool.clone(), redis.clone());
    let app = build_router(state);

    let payload = json!({
        "for_user_id": "999",
        "tweet_create_events": [{
            "id_str": tweet_id,
            "text": "@DugongWallet send 1 SUI to @bob",
            "user": { "id_str": "111", "screen_name": "alice" }
        }]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // The webhook event should now exist in the DB.
    let event_id = events::tweet_event_id(&tweet_id);
    assert!(
        WebhookEvent::exists(&pool, &event_id).await.unwrap(),
        "webhook event should be persisted"
    );

    // The dedup key should be set and a queue item enqueued.
    assert!(
        redis.check_dedup(&redis::dedup_tweet(&tweet_id)).await.unwrap(),
        "dedup key should be set"
    );
    let queued = redis.pop_queue(redis::QUEUE_TWEETS).await.unwrap();
    let queued = queued.expect("a queue item should be present");
    let item: Value = serde_json::from_str(&queued).unwrap();
    assert_eq!(item["tweet_id"], json!(tweet_id));
    assert_eq!(item["event_id"], json!(event_id));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn webhook_skips_already_persisted_event(pool: PgPool) {
    let Some(redis) = try_redis().await else {
        eprintln!("skipping: Redis unreachable at {}", common::test_redis_url());
        return;
    };
    let _guard = lock_queue().await;
    drain_queue(&redis).await;

    let tweet_id = unique_tweet_id("dedup");
    let event_id = events::tweet_event_id(&tweet_id);

    // Pre-seed the event so the webhook should treat it as a duplicate.
    WebhookEvent::create(&pool, &event_id, Some(&tweet_id), json!({}))
        .await
        .expect("seed event");

    let state = app_state(test_config(), pool.clone(), redis.clone());
    let app = build_router(state);

    let payload = json!({
        "tweet_create_events": [{
            "id_str": tweet_id,
            "text": "duplicate",
            "user": { "id_str": "111", "screen_name": "alice" }
        }]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Nothing new should have been enqueued for the duplicate event.
    assert!(
        redis.pop_queue(redis::QUEUE_TWEETS).await.unwrap().is_none(),
        "duplicate event must not enqueue work"
    );
}

#[sqlx::test(migrations = "../core/migrations")]
async fn health_check_returns_ok(pool: PgPool) {
    let Some(redis) = try_redis().await else {
        eprintln!("skipping: Redis unreachable at {}", common::test_redis_url());
        return;
    };
    let state = app_state(test_config(), pool, redis);
    let app = build_router(state);

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..], b"OK");
}
