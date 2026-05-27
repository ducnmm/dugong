//! Integration tests for the JSON API routes.
//!
//! DB-backed routes use `#[sqlx::test]`. Routes that call external services
//! (Enoki, Twitter OAuth, the enclave) are pointed at a `wiremock` server via
//! the configurable base URLs on `Config`. A live Redis is only needed to
//! build `AppState`; these routes do not touch it.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{app_state, test_config, try_redis};
use dugong_api::build_router;
use dugong_core::db::models::DugongAccount;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

macro_rules! redis_or_skip {
    () => {
        match try_redis().await {
            Some(r) => r,
            None => {
                eprintln!("skipping: Redis unreachable at {}", common::test_redis_url());
                return;
            }
        }
    };
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[sqlx::test(migrations = "../core/migrations")]
async fn get_account_by_wallet_returns_seeded_account(pool: PgPool) {
    let redis = redis_or_skip!();
    DugongAccount::create(&pool, "user-1", "alice", "0xobj1")
        .await
        .expect("create");
    DugongAccount::link_owner(&pool, "user-1", "0xowner1")
        .await
        .expect("link");

    let app = build_router(app_state(test_config(), pool, redis));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/account/by-wallet/0xowner1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["x_user_id"], json!("user-1"));
    assert_eq!(body["owner_address"], json!("0xowner1"));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn get_account_by_wallet_unknown_is_404(pool: PgPool) {
    let redis = redis_or_skip!();
    let app = build_router(app_state(test_config(), pool, redis));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/account/by-wallet/0xnope")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../core/migrations")]
async fn search_accounts_matches_handle(pool: PgPool) {
    let redis = redis_or_skip!();
    DugongAccount::create(&pool, "user-2", "bob", "0xobj2")
        .await
        .expect("create");

    let app = build_router(app_state(test_config(), pool, redis));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/accounts/search?q=bob")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert!(body["count"].as_u64().unwrap() >= 1);
    assert!(body["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["x_user_id"] == json!("user-2")));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn exchange_twitter_token_returns_auth(pool: PgPool) {
    let redis = redis_or_skip!();
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
    Mock::given(method("GET"))
        .and(path("/2/users/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "id": "555", "name": "Alice", "username": "alice" }
        })))
        .mount(&server)
        .await;

    let mut config = test_config();
    config.twitter_api_base = server.uri();
    let app = build_router(app_state(config, pool, redis));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/twitter/token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "code": "auth-code",
                        "code_verifier": "verifier",
                        "redirect_uri": "http://localhost/callback"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["accessToken"], json!("tok123"));
    assert_eq!(body["user"]["username"], json!("alice"));
    // No dugong account seeded for this user.
    assert_eq!(body["dugongAccount"], json!(null));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn sponsor_transaction_returns_bytes_and_digest(pool: PgPool) {
    let redis = redis_or_skip!();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/transaction-blocks/sponsor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "bytes": "AAAA", "digest": "0xdigest" }
        })))
        .mount(&server)
        .await;

    let mut config = test_config();
    config.enoki_base_url = server.uri();
    let app = build_router(app_state(config, pool, redis));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sponsor")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "network": "testnet",
                        "txBytes": "KIND",
                        "sender": "0xsender",
                        "allowedAddresses": []
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["bytes"], json!("AAAA"));
    assert_eq!(body["digest"], json!("0xdigest"));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn execute_sponsored_transaction_returns_digest(pool: PgPool) {
    let redis = redis_or_skip!();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/transaction-blocks/sponsor/0xdigest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "digest": "0xfinal" }
        })))
        .mount(&server)
        .await;

    let mut config = test_config();
    config.enoki_base_url = server.uri();
    let app = build_router(app_state(config, pool, redis));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/execute")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "digest": "0xdigest", "signature": "c2ln" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["digest"], json!("0xfinal"));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn secure_link_wallet_surfaces_enclave_failure(pool: PgPool) {
    let redis = redis_or_skip!();
    // Enclave returns an error; the route should respond 200 with success=false
    // rather than panicking or leaking a 500.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("enclave down"))
        .mount(&server)
        .await;

    let mut config = test_config();
    config.enclave_url = server.uri();
    let app = build_router(app_state(config, pool, redis));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/link-wallet/submit")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "access_token": "tok",
                        "wallet_address": "0xwallet",
                        "wallet_signature": "c2ln",
                        "message": "Link XID:1 to wallet 0xwallet at 0",
                        "timestamp": 0
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["success"], json!(false));
    assert!(body["error"].as_str().unwrap().contains("Verification failed"));
}
