//! HTTP client tests for dugong-core, exercised against a local wiremock
//! server so no live network calls are made.

mod common;

use common::test_config;
use dugong_core::clients::enclave::EnclaveClient;
use dugong_core::clients::enoki::EnokiClient;
use dugong_core::clients::sui_client::SuiClient;
use dugong_core::clients::twitter::{RefreshError, TwitterClient, TwitterOAuth2Client};
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ============================ SuiClient (GraphQL) ============================
// Response shapes mirror live captures from https://graphql.testnet.sui.io/graphql
// (beta schema generation), 2026-07-13.

/// A one-event GraphQL `events` page, as the beta schema serves it.
fn graphql_events_page(cursor: &str, has_next_page: bool) -> serde_json::Value {
    json!({
        "data": {
            "events": {
                "edges": [{
                    "cursor": cursor,
                    "node": {
                        "sequenceNumber": 0,
                        "timestamp": "2023-11-14T22:13:20Z",
                        "sender": { "address": "0xsender" },
                        "transaction": {
                            "digest": "DIGEST1",
                            "effects": { "checkpoint": { "sequenceNumber": 42 } }
                        },
                        "contents": {
                            "type": { "repr": "0x9::events::AccountCreated" },
                            "json": { "xid": "42" }
                        }
                    }
                }],
                "pageInfo": { "hasNextPage": has_next_page, "endCursor": cursor }
            }
        }
    })
}

#[tokio::test]
async fn sui_get_coin_metadata_parses_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "coinMetadata": {
                    "decimals": 9,
                    "name": "Sui",
                    "symbol": "SUI",
                    "description": "Native token",
                    "iconUrl": null,
                    "address": "0xabc"
                }
            }
        })))
        .mount(&server)
        .await;

    let client = SuiClient::new(server.uri());
    let meta = client
        .get_coin_metadata("0x2::sui::SUI")
        .await
        .expect("request should succeed")
        .expect("metadata should be present");

    assert_eq!(meta.decimals, 9);
    assert_eq!(meta.symbol, "SUI");
    assert_eq!(meta.id.as_deref(), Some("0xabc"));
}

#[tokio::test]
async fn sui_coin_metadata_null_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "coinMetadata": null } })),
        )
        .mount(&server)
        .await;

    let client = SuiClient::new(server.uri());
    let meta = client
        .get_coin_metadata("0x2::nope::NOPE")
        .await
        .expect("request should succeed");
    assert!(meta.is_none());
}

#[tokio::test]
async fn sui_graphql_error_is_surfaced() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": null,
            "errors": [{ "message": "invalid coin type" }]
        })))
        .mount(&server)
        .await;

    let client = SuiClient::new(server.uri());
    let err = client
        .get_coin_metadata("bad")
        .await
        .expect_err("GraphQL error should map to Err");
    assert!(err.to_string().contains("invalid coin type"));
}

#[tokio::test]
async fn sui_non_success_status_is_surfaced() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .mount(&server)
        .await;

    let client = SuiClient::new(server.uri());
    let err = client
        .query_events("0x9", "events", None, 50)
        .await
        .expect_err("non-2xx should be Err, never an empty page");
    assert!(err.to_string().contains("429"));
}

#[tokio::test]
async fn sui_query_events_parses_page() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        // The event-type filter must carry the package::module prefix.
        .and(wiremock::matchers::body_string_contains("0x9::events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(graphql_events_page("CURSOR1", false)))
        .mount(&server)
        .await;

    let client = SuiClient::new(server.uri());
    let page = client
        .query_events("0x9", "events", None, 50)
        .await
        .expect("query should succeed");

    assert_eq!(page.data.len(), 1);
    assert!(!page.has_next_page);
    assert_eq!(page.data[0].event_type, "0x9::events::AccountCreated");
    assert_eq!(page.data[0].id.tx_digest, "DIGEST1");
    assert_eq!(page.data[0].id.event_seq, "0");
    // ISO-8601 timestamp converted to epoch milliseconds at the client boundary.
    assert_eq!(page.data[0].timestamp_ms.as_deref(), Some("1700000000000"));
    assert_eq!(page.data[0].checkpoint, Some(42));
    assert_eq!(page.next_cursor.as_deref(), Some("CURSOR1"));
}

#[tokio::test]
async fn sui_query_events_clamps_page_size() {
    let server = MockServer::start().await;
    // Asking for 1000 must reach the service as the 50-event maximum.
    Mock::given(method("POST"))
        .and(wiremock::matchers::body_string_contains("\"first\":50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(graphql_events_page("CURSOR1", true)))
        .expect(1)
        .mount(&server)
        .await;

    let client = SuiClient::new(server.uri());
    client
        .query_events("0x9", "events", None, 1000)
        .await
        .expect("clamped query should succeed");
}

#[tokio::test]
async fn sui_rejected_cursor_is_typed() {
    let server = MockServer::start().await;
    // Live capture: an unparseable `after` cursor yields this errors entry.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": null,
            "errors": [{ "message": "Failed to parse \"String\": Invalid JSON" }]
        })))
        .mount(&server)
        .await;

    let client = SuiClient::new(server.uri());
    let err = client
        .query_events("0x9", "events", Some("EXPIRED"), 50)
        .await
        .expect_err("rejected cursor should be Err");
    assert!(
        err.chain()
            .any(|c| c.is::<dugong_core::clients::sui_client::CursorRejected>()),
        "error should downcast to CursorRejected, got: {err:#}"
    );
}

#[tokio::test]
async fn sui_transaction_checkpoint_lookup() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "transaction": { "effects": { "checkpoint": { "sequenceNumber": 359762100u64 } } }
            }
        })))
        .mount(&server)
        .await;

    let client = SuiClient::new(server.uri());
    let cp = client
        .get_transaction_checkpoint("DIGEST1")
        .await
        .expect("lookup should succeed");
    assert_eq!(cp, Some(359762100));
}

#[tokio::test]
async fn sui_unknown_transaction_is_none() {
    let server = MockServer::start().await;
    // Live capture: unknown digest -> data.transaction = null (no errors entry).
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "transaction": null } })),
        )
        .mount(&server)
        .await;

    let client = SuiClient::new(server.uri());
    let cp = client
        .get_transaction_checkpoint("11111111111111111111111111111111")
        .await
        .expect("lookup should succeed");
    assert_eq!(cp, None);
}

// ============================ EnokiClient ============================

#[tokio::test]
async fn enoki_create_sponsored_transaction_unwraps_data() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/transaction-blocks/sponsor"))
        .and(header("Authorization", "Bearer test-enoki-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "bytes": "AAAA", "digest": "0xdigest" }
        })))
        .mount(&server)
        .await;

    let client = EnokiClient::with_base_url(
        "test-enoki-key".to_string(),
        "testnet".to_string(),
        server.uri(),
    );
    let resp = client
        .create_sponsored_transaction("KIND".to_string(), "0xsender".to_string(), vec![])
        .await
        .expect("sponsor should succeed");

    assert_eq!(resp.bytes, "AAAA");
    assert_eq!(resp.digest, "0xdigest");
}

#[tokio::test]
async fn enoki_error_status_is_surfaced() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
        .mount(&server)
        .await;

    let client = EnokiClient::with_base_url("k".to_string(), "testnet".to_string(), server.uri());
    let err = client
        .create_sponsored_transaction("KIND".to_string(), "0xs".to_string(), vec![])
        .await
        .expect_err("non-2xx should be Err");
    assert!(err.to_string().contains("bad request"));
}

// ============================ EnclaveClient ============================

#[tokio::test]
async fn enclave_process_tweet_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/process_tweet"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "command_type": "transfer",
            "intent": 3,
            "timestamp_ms": 1700000000000u64,
            "signature": "c2ln",
            "common": {
                "tweet_id": "111",
                "author_xid": "222",
                "author_handle": "alice"
            },
            "data": {
                "from_xid": "222",
                "from_handle": "alice",
                "to_xid": "333",
                "to_handle": "bob",
                "amount": 1000,
                "coin_type": "0x2::sui::SUI"
            }
        })))
        .mount(&server)
        .await;

    let client = EnclaveClient::new(server.uri());
    let resp = client
        .process_tweet("https://x.com/alice/status/111")
        .await
        .expect("process_tweet should succeed");

    let transfer = EnclaveClient::parse_transfer_data(&resp).expect("transfer data parses");
    assert_eq!(transfer.to_handle, "bob");
    assert_eq!(transfer.amount, 1000);
}

#[tokio::test]
async fn enclave_non_success_is_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/process_tweet"))
        .respond_with(ResponseTemplate::new(500).set_body_string("enclave boom"))
        .mount(&server)
        .await;

    let client = EnclaveClient::new(server.uri());
    let err = client
        .process_tweet("https://x.com/a/status/1")
        .await
        .expect_err("500 should be Err");
    assert!(err.to_string().contains("enclave boom"));
}

#[tokio::test]
async fn enclave_retries_transient_5xx_then_succeeds() {
    let server = MockServer::start().await;
    // Fallback success (mounted first => lower match priority).
    Mock::given(method("POST"))
        .and(path("/process_tweet"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "command_type": "transfer",
            "intent": 3,
            "timestamp_ms": 1700000000000u64,
            "signature": "c2ln",
            "common": { "tweet_id": "111", "author_xid": "222", "author_handle": "alice" },
            "data": {
                "from_xid": "222", "from_handle": "alice",
                "to_xid": "333", "to_handle": "bob",
                "amount": 1000, "coin_type": "0x2::sui::SUI"
            }
        })))
        .mount(&server)
        .await;
    // First call returns a transient 503 (mounted last => matched first, once only).
    Mock::given(method("POST"))
        .and(path("/process_tweet"))
        .respond_with(ResponseTemplate::new(503).set_body_string("cold boot"))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let client = EnclaveClient::new(server.uri());
    let resp = client
        .process_tweet("https://x.com/alice/status/111")
        .await
        .expect("should succeed after retrying the 503");
    let transfer = EnclaveClient::parse_transfer_data(&resp).expect("transfer data parses");
    assert_eq!(transfer.to_handle, "bob");
}

#[tokio::test]
async fn enclave_does_not_retry_business_error() {
    let server = MockServer::start().await;
    // `expect(1)` verifies (on drop) that exactly one request was made — i.e. a
    // 400 business error is NOT retried.
    Mock::given(method("POST"))
        .and(path("/process_tweet"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
        .expect(1)
        .mount(&server)
        .await;

    let client = EnclaveClient::new(server.uri());
    let err = client
        .process_tweet("https://x.com/a/status/1")
        .await
        .expect_err("400 should be Err");
    assert!(err.to_string().contains("400"));
}

#[tokio::test]
async fn enclave_connection_error_is_bounded() {
    // Nothing listens on this port → connection refused on each attempt; the
    // client must give up (bounded retries) and surface a transport error
    // rather than hang.
    let client = EnclaveClient::new("http://127.0.0.1:1");
    let err = client
        .process_tweet("https://x.com/a/status/1")
        .await
        .expect_err("connection refused should error");
    assert!(err.to_string().contains("request failed"));
}

// ============================ TwitterClient ============================

#[tokio::test]
async fn twitter_get_user_by_username_parses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/twitter/user/info"))
        .and(query_param("userName", "alice"))
        .and(header("X-API-Key", "test-twitterapi-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "msg": null,
            "data": { "id": "42", "userName": "alice", "name": "Alice" }
        })))
        .mount(&server)
        .await;

    let client = TwitterClient::with_base_url(&test_config(), server.uri());
    let user = client
        .get_user_by_username("@alice")
        .await
        .expect("lookup should succeed");
    assert_eq!(user.id, "42");
    assert_eq!(user.username, "alice");
}

#[tokio::test]
async fn twitter_api_error_status_is_surfaced() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/twitter/user/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "error",
            "msg": "user not found",
            "data": { "id": "", "userName": "", "name": "" }
        })))
        .mount(&server)
        .await;

    let client = TwitterClient::with_base_url(&test_config(), server.uri());
    let err = client
        .get_user_by_username("ghost")
        .await
        .expect_err("api status error should be Err");
    assert!(err.to_string().contains("user not found"));
}

// ============================ TwitterOAuth2Client ============================

#[tokio::test]
async fn oauth2_exchange_code_parses_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/2/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "tok123",
            "token_type": "bearer",
            "expires_in": 7200,
            "refresh_token": "ref456",
            "scope": "tweet.read users.read"
        })))
        .mount(&server)
        .await;

    let client = TwitterOAuth2Client::with_base_url(&test_config(), server.uri());
    let token = client
        .exchange_code("code", "verifier", "http://localhost/callback")
        .await
        .expect("exchange should succeed");
    assert_eq!(token.access_token, "tok123");
    assert_eq!(token.refresh_token.as_deref(), Some("ref456"));
}

#[tokio::test]
async fn oauth2_get_user_info_parses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/2/users/me"))
        .and(header("Authorization", "Bearer tok123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "id": "9", "name": "Alice", "username": "alice" }
        })))
        .mount(&server)
        .await;

    let client = TwitterOAuth2Client::with_base_url(&test_config(), server.uri());
    let info = client
        .get_user_info("tok123")
        .await
        .expect("me should succeed");
    assert_eq!(info.username, "alice");
}

#[tokio::test]
async fn oauth2_refresh_returns_rotated_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/2/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "fresh-access",
            "token_type": "bearer",
            "expires_in": 7200,
            "refresh_token": "rotated-refresh",
            "scope": "tweet.read users.read offline.access"
        })))
        .mount(&server)
        .await;

    let client = TwitterOAuth2Client::with_base_url(&test_config(), server.uri());
    let token = client
        .refresh_access_token("old-refresh")
        .await
        .expect("refresh should succeed");
    assert_eq!(token.access_token, "fresh-access");
    // Twitter rotates the refresh token — caller must persist the new one.
    assert_eq!(token.refresh_token.as_deref(), Some("rotated-refresh"));
}

#[tokio::test]
async fn oauth2_refresh_invalid_grant_requires_reauth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/2/oauth2/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Value passed for the token was invalid."
        })))
        .mount(&server)
        .await;

    let client = TwitterOAuth2Client::with_base_url(&test_config(), server.uri());
    let err = client
        .refresh_access_token("dead-refresh")
        .await
        .expect_err("refresh should fail");
    assert!(
        matches!(err, RefreshError::ReauthRequired(_)),
        "invalid_grant must map to ReauthRequired, got {err:?}"
    );
}

#[tokio::test]
async fn oauth2_refresh_server_error_is_transient() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/2/oauth2/token"))
        .respond_with(ResponseTemplate::new(503).set_body_string("upstream unavailable"))
        .mount(&server)
        .await;

    let client = TwitterOAuth2Client::with_base_url(&test_config(), server.uri());
    let err = client
        .refresh_access_token("some-refresh")
        .await
        .expect_err("refresh should fail");
    assert!(
        matches!(err, RefreshError::Transient(_)),
        "5xx must map to Transient, got {err:?}"
    );
}

// ==================== OAuth 1.0a bot posting ====================

/// End-to-end reply posting through the OAuth 1.0a path: with all four keys
/// configured, `POST /2/tweets` must carry a signed `OAuth ...` header (never
/// a Bearer token) and need no DB-stored token at all.
#[tokio::test]
async fn twitter_reply_posts_with_oauth1_signature_when_keys_configured() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/2/tweets"))
        .and(wiremock::matchers::header_regex(
            "authorization",
            concat!(
                r#"^OAuth oauth_consumer_key="test-consumer-key", "#,
                r#"oauth_nonce="[0-9a-f]{32}", "#,
                r#"oauth_signature_method="HMAC-SHA1", "#,
                r#"oauth_timestamp="\d+", "#,
                r#"oauth_token="test-access-token", "#,
                r#"oauth_version="1\.0", "#,
                r#"oauth_signature="[A-Za-z0-9%]+"$"#,
            ),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "data": { "id": "1900000000000000123" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = test_config();
    config.twitter_api_base = server.uri();
    config.twitter_api_key = Some("test-consumer-key".to_string());
    config.twitter_api_secret = Some("test-consumer-secret".to_string());
    config.twitter_access_token = Some("test-access-token".to_string());
    config.twitter_access_token_secret = Some("test-token-secret".to_string());

    // The OAuth 1.0a path never touches the DB; a lazy pool never connects.
    let pool = sqlx::PgPool::connect_lazy("postgres://unused:unused@localhost:1/unused")
        .expect("lazy pool");
    let client = TwitterClient::new_with_bot(&config, pool);

    let reply_id = client
        .reply_error("123", "boom")
        .await
        .expect("reply should post via the OAuth 1.0a-signed official API");
    assert_eq!(reply_id, "1900000000000000123");
}
