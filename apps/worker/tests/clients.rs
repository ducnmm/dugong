//! Wiremock-backed tests for the worker's HTTP clients.

use dugong_worker::backend_client::{BackendClient, TweetCreateEvent, WebhookPayload, WebhookUser};
use dugong_worker::twitter_client::TwitterClient;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn search_mentions_parses_tweets_and_users() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/twitter/tweet/advanced_search"))
        .and(header("X-API-Key", "test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tweets": [
                {
                    "id": "100",
                    "text": "@DugongWallet send 1 SUI to @bob",
                    "createdAt": "Wed May 21 12:00:00 +0000 2025",
                    "author": { "id": "111", "userName": "alice" }
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = TwitterClient::with_base_url("test-key".to_string(), server.uri());
    let resp = client
        .search_mentions("@DugongWallet", None, None)
        .await
        .expect("search should succeed");

    assert_eq!(resp.data.len(), 1);
    assert_eq!(resp.data[0].id, "100");
    assert_eq!(resp.data[0].author_id, "111");
    let users = resp.includes.expect("includes").users;
    assert_eq!(users[0].username, "alice");
    assert_eq!(resp.meta.unwrap().newest_id.as_deref(), Some("100"));
}

#[tokio::test]
async fn search_mentions_filters_by_since_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/twitter/tweet/advanced_search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tweets": [
                { "id": "100", "text": "old", "createdAt": "Wed May 21 12:00:00 +0000 2025",
                  "author": { "id": "1", "userName": "a" } },
                { "id": "200", "text": "new", "createdAt": "Wed May 21 13:00:00 +0000 2025",
                  "author": { "id": "2", "userName": "b" } }
            ]
        })))
        .mount(&server)
        .await;

    let client = TwitterClient::with_base_url("k".to_string(), server.uri());
    // since_id=100 should drop the tweet with id 100 and keep 200.
    let resp = client
        .search_mentions("@DugongWallet", Some("100"), None)
        .await
        .expect("search should succeed");

    assert_eq!(resp.data.len(), 1);
    assert_eq!(resp.data[0].id, "200");
}

#[tokio::test]
async fn search_mentions_surfaces_http_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .mount(&server)
        .await;

    let client = TwitterClient::with_base_url("k".to_string(), server.uri());
    let err = client
        .search_mentions("@DugongWallet", None, None)
        .await
        .expect_err("non-2xx should be an error");
    assert!(err.to_string().contains("rate limited"));
}

#[tokio::test]
async fn backend_send_tweets_posts_payload() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = BackendClient::new(server.uri());
    let payload = WebhookPayload {
        for_user_id: None,
        tweet_create_events: vec![TweetCreateEvent {
            id_str: "100".to_string(),
            text: "hi".to_string(),
            user: WebhookUser {
                id_str: "111".to_string(),
                screen_name: "alice".to_string(),
            },
            in_reply_to_status_id_str: None,
        }],
    };

    assert!(client.send_tweets(payload).await.expect("send"));
}

#[tokio::test]
async fn backend_health_check_reports_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = BackendClient::new(server.uri());
    assert!(client.health_check().await.expect("health"));
}
