//! Integration tests for the indexer's poll loop: legacy-cursor migration,
//! cursor-envelope persistence, and the processed-events idempotency ledger.
//! Sui GraphQL responses are served by wiremock; Postgres by `#[sqlx::test]`.

mod common;

use common::{sui_event, test_config};
use dugong_core::db::models::DugongAccount;
use dugong_indexer::cursor::{CursorManager, StoredCursor};
use dugong_indexer::event_processor::EventProcessor;
use dugong_indexer::indexer::Indexer;
use serde_json::{json, Value};
use sqlx::PgPool;
use wiremock::matchers::{body_string_contains, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn event_edge(cursor: &str, tx_digest: &str, event_seq: u64, checkpoint: u64) -> Value {
    json!({
        "cursor": cursor,
        "node": {
            "sequenceNumber": event_seq,
            "timestamp": "2023-11-14T22:13:20Z",
            "sender": { "address": "0xsender" },
            "transaction": {
                "digest": tx_digest,
                "effects": { "checkpoint": { "sequenceNumber": checkpoint } }
            },
            "contents": {
                "type": { "repr": "0x9::events::AccountCreated" },
                "json": { "xid": "user-gql", "handle": "alice", "account_id": "0xobj" }
            }
        }
    })
}

fn events_page(edges: Vec<Value>, has_next_page: bool, end_cursor: Option<&str>) -> Value {
    json!({
        "data": {
            "events": {
                "edges": edges,
                "pageInfo": { "hasNextPage": has_next_page, "endCursor": end_cursor }
            }
        }
    })
}

/// End-to-end legacy migration: a JSON-RPC-era `txDigest:eventSeq` cursor is
/// re-anchored via GraphQL, the events after the anchor are processed exactly
/// once, and the persisted cursor becomes a v2 envelope.
#[sqlx::test(migrations = "../core/migrations")]
async fn legacy_cursor_is_reanchored_then_new_events_processed(pool: PgPool) {
    let server = MockServer::start().await;

    // Anchor tx lookup -> checkpoint 42.
    Mock::given(method("POST"))
        .and(body_string_contains("TransactionCheckpoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "transaction": { "effects": { "checkpoint": { "sequenceNumber": 42 } } }
            }
        })))
        .mount(&server)
        .await;
    // Re-anchor scan (from checkpoint 42): the anchor event itself.
    Mock::given(method("POST"))
        .and(body_string_contains("\"afterCheckpoint\":41"))
        .respond_with(ResponseTemplate::new(200).set_body_json(events_page(
            vec![event_edge("CUR_ANCHOR", "ANCHOR_TX", 0, 42)],
            false,
            Some("CUR_ANCHOR"),
        )))
        .mount(&server)
        .await;
    // Normal fetch resuming after the adopted anchor cursor: one new event.
    Mock::given(method("POST"))
        .and(body_string_contains("\"after\":\"CUR_ANCHOR\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(events_page(
            vec![event_edge("CUR_NEW", "NEW_TX", 0, 43)],
            false,
            Some("CUR_NEW"),
        )))
        .mount(&server)
        .await;
    // Subsequent fetches: nothing new.
    Mock::given(method("POST"))
        .and(body_string_contains("\"after\":\"CUR_NEW\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(events_page(vec![], false, None)))
        .mount(&server)
        .await;

    // Seed the pre-migration cursor under the primary state row name.
    let cursor_manager = CursorManager::new(pool.clone());
    cursor_manager
        .save_cursor("dugong_events", Some(&"ANCHOR_TX:0".to_string()))
        .await
        .expect("seed legacy cursor");

    let mut indexer = Indexer::new(test_config(server.uri(), "0x9".to_string()), pool.clone())
        .await
        .expect("indexer");

    // First pass: re-anchor + process the one new event.
    assert_eq!(indexer.run_once().await, 1);
    let account = DugongAccount::find_by_x_user_id(&pool, "user-gql")
        .await
        .unwrap()
        .expect("event was processed into dugong_accounts");
    assert_eq!(account.x_handle, "alice");

    // The persisted cursor is now a v2 envelope anchored at the new event.
    let stored = cursor_manager
        .load_cursor("dugong_events")
        .await
        .unwrap()
        .expect("cursor saved");
    match StoredCursor::parse(Some(&stored)).unwrap() {
        StoredCursor::Envelope(envelope) => {
            assert_eq!(envelope.gql, "CUR_NEW");
            assert_eq!(envelope.tx, "NEW_TX");
            assert_eq!(envelope.seq, "0");
            assert_eq!(envelope.cp, 43);
        }
        other => panic!("expected envelope cursor, got {other:?}"),
    }

    // Second pass: empty page, nothing re-processed, cursor unchanged.
    assert_eq!(indexer.run_once().await, 0);
    assert_eq!(
        cursor_manager.load_cursor("dugong_events").await.unwrap(),
        Some(stored)
    );
}

/// The processed-events ledger: replaying the same event (as happens when a
/// page is re-fetched after a crash before the cursor was saved) must not
/// double-apply increment-style balance updates.
#[sqlx::test(migrations = "../core/migrations")]
async fn replayed_event_is_not_double_processed(pool: PgPool) {
    let processor = EventProcessor::new(pool.clone());
    let event = sui_event(
        "0x9::events::CoinDeposited",
        "DIGEST-dep",
        json!({ "xid": "user-9", "coin_type": "0x2::sui::SUI", "amount": "1000" }),
    );

    processor
        .process_events(std::slice::from_ref(&event))
        .await
        .unwrap();
    processor.process_events(&[event]).await.unwrap();

    let balance: i64 = sqlx::query_scalar(
        "SELECT balance FROM account_balances WHERE x_user_id = $1 AND coin_type = $2",
    )
    .bind("user-9")
    .bind("0x2::sui::SUI")
    .fetch_one(&pool)
    .await
    .expect("balance row");
    assert_eq!(balance, 1000, "second replay must be a no-op");
}
