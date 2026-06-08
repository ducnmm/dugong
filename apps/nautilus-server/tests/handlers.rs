//! HTTP-handler integration tests for the dugong enclave endpoints.
//!
//! Each test boots the real Axum router (`build_router`) on an ephemeral port
//! with a deterministic test keypair, points outbound X API calls at a
//! wiremock server, and drives it over real HTTP.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use fastcrypto::ed25519::Ed25519KeyPair;
use fastcrypto::traits::{KeyPair, Signer, ToFromBytes};
use nautilus_server::{build_router, AppState};
use rand::SeedableRng;
use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Lowercase hex without a `0x` prefix, matching the enclave's internal helper.
fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Boot the enclave router on an ephemeral port, returning its base URL.
async fn spawn_app(twitter_api_base_url: String) -> String {
    let eph_kp = Ed25519KeyPair::generate(&mut rand::rngs::StdRng::from_seed([7u8; 32]));
    let state = Arc::new(AppState {
        eph_kp,
        api_key: "test-key".to_string(),
        twitterapi_io_base_url: twitter_api_base_url.clone(),
        twitter_api_base_url,
        dugong_package_id: "0x0".to_string(),
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_router(state);
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    format!("http://{}", addr)
}

#[tokio::test]
async fn process_tweet_create_account_returns_signed_payload() {
    let twitter = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/twitter/tweets"))
        .and(query_param("tweet_ids", "123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "tweets": [{
                "id": "123",
                "text": "@dugong create account",
                "author": { "id": "555", "userName": "alice" }
            }]
        })))
        .mount(&twitter)
        .await;

    let base = spawn_app(twitter.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/process_tweet"))
        .json(&json!({ "payload": { "tweet_url": "https://x.com/alice/status/123" } }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["command_type"], "create_account");
    assert_eq!(body["common"]["author_xid"], "555");
    assert_eq!(body["common"]["author_handle"], "alice");
    assert!(body["signature"].as_str().is_some_and(|s| !s.is_empty()));
}

#[tokio::test]
async fn process_tweet_transfer_resolves_mentioned_user() {
    let twitter = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/twitter/tweets"))
        .and(query_param("tweet_ids", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "tweets": [{
                "id": "200",
                "text": "@dugong send 5 SUI to @bob",
                "author": { "id": "111", "userName": "alice" },
                "entities": {
                    "user_mentions": [
                        { "id_str": "999", "screen_name": "bob" }
                    ]
                }
            }]
        })))
        .mount(&twitter)
        .await;

    let base = spawn_app(twitter.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/process_tweet"))
        .json(&json!({ "payload": { "tweet_url": "https://x.com/alice/status/200" } }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["command_type"], "transfer");
    assert_eq!(body["data"]["to_xid"], "999");
    assert_eq!(body["data"]["to_handle"], "bob");
    // 5 SUI at 9 decimals = 5_000_000_000 MIST.
    assert_eq!(body["data"]["amount"], 5_000_000_000u64);
}

#[tokio::test]
async fn process_tweet_invalid_url_returns_bad_request() {
    // The URL regex fails before any HTTP call, so no mock is needed.
    let base = spawn_app("http://unused".to_string()).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/process_tweet"))
        .json(&json!({ "payload": { "tweet_url": "not-a-tweet-url" } }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("Invalid tweet URL"));
}

#[tokio::test]
async fn process_init_account_returns_signed_payload() {
    // No external HTTP: the handler mocks the handle internally.
    let base = spawn_app("http://unused".to_string()).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/process_init_account"))
        .json(&json!({ "payload": { "xid": "1985975069177511936" } }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    // ProcessData intent == 0 (INIT_ACCOUNT_INTENT).
    assert_eq!(body["response"]["intent"], 0);
    assert!(body["signature"].as_str().is_some_and(|s| !s.is_empty()));
}

#[tokio::test]
async fn process_secure_link_wallet_verifies_token_and_signature() {
    let xid = "1985975069177511936";
    let timestamp = now_ms();

    // Build a real Sui-style ed25519 wallet signature over the link message.
    let wallet_kp = Ed25519KeyPair::generate(&mut rand::rngs::StdRng::from_seed([3u8; 32]));
    let pubkey = wallet_kp.public().as_bytes().to_vec();

    use fastcrypto::encoding::{Base64, Encoding};
    use fastcrypto::hash::{Blake2b256, HashFunction};

    let mut addr_input = vec![0u8]; // Ed25519 flag.
    addr_input.extend_from_slice(&pubkey);
    let address = to_hex(Blake2b256::digest(&addr_input).as_ref());
    let wallet_address = format!("0x{address}");

    let message = format!("Link XID:{xid} to wallet {wallet_address} at {timestamp}");

    let message_bcs = bcs::to_bytes(&message.as_bytes().to_vec()).unwrap();
    let mut signing_input = vec![3u8, 0, 0]; // PersonalMessage, V0, Sui.
    signing_input.extend_from_slice(&message_bcs);
    let digest = Blake2b256::digest(&signing_input);
    let sig = wallet_kp.sign(digest.as_ref());

    let mut sui_sig = vec![0u8]; // Ed25519 flag.
    sui_sig.extend_from_slice(sig.as_ref());
    sui_sig.extend_from_slice(&pubkey);
    let wallet_signature = Base64::encode(&sui_sig);

    let twitter = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/2/users/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "id": xid, "username": "alice" }
        })))
        .mount(&twitter)
        .await;

    let base = spawn_app(twitter.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/process_secure_link_wallet"))
        .json(&json!({ "payload": {
            "access_token": "valid-token",
            "wallet_address": wallet_address,
            "wallet_signature": wallet_signature,
            "message": message,
            "timestamp": timestamp,
        }}))
        .send()
        .await
        .unwrap();

    let status = resp.status();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(status, 200, "body: {body:?}");
    // LinkWallet intent == 1.
    assert_eq!(body["response"]["intent"], 1);
    assert!(body["signature"].as_str().is_some_and(|s| !s.is_empty()));
}

#[tokio::test]
async fn process_secure_link_wallet_rejects_invalid_token() {
    // No mock is mounted, so the wiremock server replies 404 to /2/users/me;
    // assert the handler surfaces that upstream failure as a 400 EnclaveError.
    let twitter = MockServer::start().await;
    let base = spawn_app(twitter.uri()).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/process_secure_link_wallet"))
        .json(&json!({ "payload": {
            "access_token": "bad-token",
            "wallet_address": "0x00",
            "wallet_signature": "AA==",
            "message": "irrelevant",
            "timestamp": now_ms(),
        }}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("Twitter API returned error"));
}
