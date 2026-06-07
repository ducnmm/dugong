//! Wiremock-backed tests for the event fetcher's Sui RPC pagination.

mod common;

use common::test_config;
use dugong_indexer::event_fetcher::EventFetcher;
use serde_json::json;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn fetch_events_parses_page() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": "1",
            "result": {
                "data": [{
                    "id": { "txDigest": "DIGEST1", "eventSeq": "0" },
                    "packageId": "0x9",
                    "transactionModule": "events",
                    "sender": "0xsender",
                    "type": "0x9::events::AccountCreated",
                    "parsedJson": { "xid": "1", "handle": "alice", "account_id": "0xobj" },
                    "bcs": null,
                    "timestampMs": "1700000000000"
                }],
                "nextCursor": { "txDigest": "DIGEST1", "eventSeq": "0" },
                "hasNextPage": true
            }
        })))
        .mount(&server)
        .await;

    let fetcher = EventFetcher::new(test_config(server.uri(), "0x9".to_string()))
        .await
        .expect("fetcher");
    let page = fetcher.fetch_events(None, 50).await.expect("fetch");

    assert_eq!(page.data.len(), 1);
    assert!(page.has_next_page);
    assert_eq!(page.data[0].event_type, "0x9::events::AccountCreated");
    assert_eq!(page.next_cursor.unwrap().to_cursor(), "DIGEST1:0");
}

#[tokio::test]
async fn fetch_events_empty_page() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": "1",
            "result": { "data": [], "nextCursor": null, "hasNextPage": false }
        })))
        .mount(&server)
        .await;

    let fetcher = EventFetcher::new(test_config(server.uri(), "0x9".to_string()))
        .await
        .expect("fetcher");
    let page = fetcher
        .fetch_events(Some("CURSOR:1"), 50)
        .await
        .expect("fetch");

    assert!(page.data.is_empty());
    assert!(!page.has_next_page);
}
