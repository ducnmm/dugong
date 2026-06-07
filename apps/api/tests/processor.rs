//! Integration tests for the processor worker's queue handling.
//!
//! These cover the early-exit paths of `ProcessorWorker::process_once` that do
//! NOT submit Sui transactions: empty queue, event-not-found, and
//! already-done events. The full per-`CommandType` dispatch (transfer,
//! create_account, market lifecycle) builds and submits real Sui transactions
//! via `SuiTransactionBuilder` against a live fullnode, which can't be faked
//! with wiremock without a trait-based Sui-client injection refactor — so it
//! is exercised by the live stack rather than here.

mod common;

use common::{app_state, drain_queue, lock_queue, test_config, try_redis};
use dugong_api::processor::{ProcessOutcome, ProcessorWorker};
use dugong_core::constants::{events, redis};
use dugong_core::db::models::WebhookEvent;
use serde_json::json;
use sqlx::PgPool;

macro_rules! redis_or_skip {
    () => {
        match try_redis().await {
            Some(r) => r,
            None => {
                eprintln!(
                    "skipping: Redis unreachable at {}",
                    common::test_redis_url()
                );
                return;
            }
        }
    };
}

#[sqlx::test(migrations = "../core/migrations")]
async fn process_once_returns_empty_when_queue_drained(pool: PgPool) {
    let redis = redis_or_skip!();
    let _guard = lock_queue().await;
    drain_queue(&redis).await;

    let worker = ProcessorWorker::new(app_state(test_config(), pool, redis));
    let outcome = worker.process_once().await.expect("process_once");
    assert!(matches!(outcome, ProcessOutcome::Empty));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn process_once_skips_missing_event(pool: PgPool) {
    let redis = redis_or_skip!();
    let _guard = lock_queue().await;
    drain_queue(&redis).await;

    // Queue references an event that was never persisted.
    let item = json!({ "tweet_id": "tw-missing", "event_id": "tweet:tw-missing" });
    redis
        .push_queue(redis::QUEUE_TWEETS, &item.to_string())
        .await
        .unwrap();

    let worker = ProcessorWorker::new(app_state(test_config(), pool, redis));
    let outcome = worker.process_once().await.expect("process_once");
    match outcome {
        ProcessOutcome::Processed { tweet_id, .. } => assert_eq!(tweet_id, "tw-missing"),
        other => panic!("expected Processed, got {other:?}"),
    }
}

#[sqlx::test(migrations = "../core/migrations")]
async fn process_once_skips_already_done_event(pool: PgPool) {
    let redis = redis_or_skip!();
    let _guard = lock_queue().await;
    drain_queue(&redis).await;

    let tweet_id = "tw-done";
    let event_id = events::tweet_event_id(tweet_id);
    WebhookEvent::create(&pool, &event_id, Some(tweet_id), json!({}))
        .await
        .expect("create event");
    WebhookEvent::set_completed(&pool, &event_id)
        .await
        .expect("complete event");

    let item = json!({ "tweet_id": tweet_id, "event_id": event_id });
    redis
        .push_queue(redis::QUEUE_TWEETS, &item.to_string())
        .await
        .unwrap();

    let worker = ProcessorWorker::new(app_state(test_config(), pool.clone(), redis));
    let outcome = worker.process_once().await.expect("process_once");
    match outcome {
        ProcessOutcome::Processed { tweet_id: tid, .. } => assert_eq!(tid, tweet_id),
        other => panic!("expected Processed, got {other:?}"),
    }

    // A done event must not be re-driven: status stays Completed.
    let reloaded = WebhookEvent::find_by_event_id(&pool, &event_id)
        .await
        .unwrap()
        .unwrap();
    assert!(reloaded.is_done());
}
