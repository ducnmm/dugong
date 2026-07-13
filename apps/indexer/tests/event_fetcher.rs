//! Wiremock-backed tests for the event fetcher's Sui GraphQL pagination and
//! cursor re-anchoring. Response shapes mirror live captures from
//! https://graphql.testnet.sui.io/graphql (beta schema generation), 2026-07-13.

mod common;

use common::test_config;
use dugong_indexer::event_fetcher::EventFetcher;
use serde_json::{json, Value};
use wiremock::matchers::{body_string_contains, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// One `events` edge for the GraphQL page fixtures.
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
                "json": { "xid": "1", "handle": "alice", "account_id": "0xobj" }
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

async fn fetcher(server: &MockServer) -> EventFetcher {
    EventFetcher::new(test_config(server.uri(), "0x9".to_string()))
        .await
        .expect("fetcher")
}

#[tokio::test]
async fn fetch_events_parses_page() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(events_page(
            vec![event_edge("CURSOR1", "DIGEST1", 0, 42)],
            true,
            Some("CURSOR1"),
        )))
        .mount(&server)
        .await;

    let page = fetcher(&server)
        .await
        .fetch_events("0x9", None, 50)
        .await
        .expect("fetch");

    assert_eq!(page.data.len(), 1);
    assert!(page.has_next_page);
    assert_eq!(page.data[0].event_type, "0x9::events::AccountCreated");
    assert_eq!(page.data[0].id.tx_digest, "DIGEST1");
    assert_eq!(page.data[0].checkpoint, Some(42));
    assert_eq!(page.next_cursor.as_deref(), Some("CURSOR1"));
}

#[tokio::test]
async fn fetch_events_empty_page() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(events_page(vec![], false, None)),
        )
        .mount(&server)
        .await;

    let page = fetcher(&server)
        .await
        .fetch_events("0x9", Some("SOMECURSOR"), 50)
        .await
        .expect("fetch");

    assert!(page.data.is_empty());
    assert!(!page.has_next_page);
}

/// Legacy-cursor migration: resolve the anchor tx's checkpoint, page from that
/// checkpoint, and adopt the anchor event's own cursor — so resuming `after`
/// it re-processes nothing at/before the anchor and skips nothing after it.
#[tokio::test]
async fn re_anchor_legacy_cursor_adopts_anchor_events_cursor() {
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

    // Events from checkpoint 42 onward: an earlier event in the same tx, the
    // anchor (ANCHOR_TX seq 1), and a later event that must NOT be consumed.
    Mock::given(method("POST"))
        .and(body_string_contains("query Events"))
        .and(body_string_contains("\"afterCheckpoint\":41"))
        .respond_with(ResponseTemplate::new(200).set_body_json(events_page(
            vec![
                event_edge("CUR_A", "ANCHOR_TX", 0, 42),
                event_edge("CUR_ANCHOR", "ANCHOR_TX", 1, 42),
                event_edge("CUR_B", "LATER_TX", 0, 43),
            ],
            true,
            Some("CUR_B"),
        )))
        .mount(&server)
        .await;

    let envelope = fetcher(&server)
        .await
        .re_anchor("0x9", "ANCHOR_TX", "1", None)
        .await
        .expect("re-anchor");

    assert_eq!(envelope.gql, "CUR_ANCHOR");
    assert_eq!(envelope.tx, "ANCHOR_TX");
    assert_eq!(envelope.seq, "1");
    assert_eq!(envelope.cp, 42);
}

/// Re-anchoring from an envelope whose GraphQL cursor was rejected: the
/// checkpoint is already known, so no transaction lookup is needed, and the
/// anchor may sit on a later page of the checkpoint scan.
#[tokio::test]
async fn re_anchor_with_known_checkpoint_pages_until_anchor() {
    let server = MockServer::start().await;

    // First scan page (no `after`): anchor not there yet.
    Mock::given(method("POST"))
        .and(body_string_contains("query Events"))
        .and(body_string_contains("\"after\":null"))
        .respond_with(ResponseTemplate::new(200).set_body_json(events_page(
            vec![event_edge("PAGE1_END", "OTHER_TX", 0, 42)],
            true,
            Some("PAGE1_END"),
        )))
        .mount(&server)
        .await;
    // Second page: contains the anchor.
    Mock::given(method("POST"))
        .and(body_string_contains("query Events"))
        .and(body_string_contains("\"after\":\"PAGE1_END\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(events_page(
            vec![event_edge("CUR_ANCHOR", "ANCHOR_TX", 0, 42)],
            false,
            Some("CUR_ANCHOR"),
        )))
        .mount(&server)
        .await;

    let envelope = fetcher(&server)
        .await
        .re_anchor("0x9", "ANCHOR_TX", "0", Some(42))
        .await
        .expect("re-anchor");

    assert_eq!(envelope.gql, "CUR_ANCHOR");
    assert_eq!(envelope.cp, 42);
}

/// The anchor tx is unknown to the endpoint (pruned out of retention): fail
/// loudly with the package, digest, and remediation — never restart silently.
#[tokio::test]
async fn re_anchor_unknown_anchor_fails_loudly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("TransactionCheckpoint"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "transaction": null } })),
        )
        .mount(&server)
        .await;

    let err = fetcher(&server)
        .await
        .re_anchor("0x9", "PRUNED_TX", "0", None)
        .await
        .expect_err("must not silently restart");
    let msg = format!("{err:#}");
    assert!(msg.contains("0x9"), "message names the package: {msg}");
    assert!(msg.contains("PRUNED_TX"), "message names the digest: {msg}");
    assert!(msg.contains("SUI_GRAPHQL_URL"), "message has remediation: {msg}");
}

/// The scan walks past the anchor's checkpoint without finding it: also a loud
/// failure (the anchor event no longer exists on this endpoint).
#[tokio::test]
async fn re_anchor_scan_past_checkpoint_fails_loudly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("query Events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(events_page(
            vec![event_edge("CUR_X", "OTHER_TX", 0, 43)],
            true,
            Some("CUR_X"),
        )))
        .mount(&server)
        .await;

    let err = fetcher(&server)
        .await
        .re_anchor("0x9", "GONE_TX", "0", Some(42))
        .await
        .expect_err("must not silently restart");
    assert!(format!("{err:#}").contains("GONE_TX"));
}
