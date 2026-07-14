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
use dugong_core::db::models::{DugongAccount, TwitterOAuthToken};
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
                eprintln!(
                    "skipping: Redis unreachable at {}",
                    common::test_redis_url()
                );
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
async fn get_transaction_by_digest_returns_seeded_transfer(pool: PgPool) {
    let redis = redis_or_skip!();
    sqlx::query(
        r#"
        INSERT INTO transfers
            (transaction_digest, transfer_type, from_xid, to_xid, coin_type, amount, tweet_id, timestamp)
        VALUES
            ('0xtx123', 'transfer'::transfer_type, 'alice-xid', 'bob-xid', '0x2::usdc::USDC', 1234567, 'tweet-1', 42)
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed transfer");

    let server = MockServer::start().await;
    // Sui GraphQL coinMetadata response (beta schema shape).
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "coinMetadata": {
                    "decimals": 6,
                    "name": "USDC",
                    "symbol": "USDC",
                    "description": null,
                    "iconUrl": null,
                    "address": null
                }
            }
        })))
        .mount(&server)
        .await;

    let mut config = test_config();
    config.sui_graphql_url = server.uri();
    let app = build_router(app_state(config, pool, redis));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/transaction/0xtx123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["tx_digest"], json!("0xtx123"));
    // Displayed amounts are rounded to 2dp (4922bdc); amount_mist keeps full precision.
    assert_eq!(body["amount"], json!("1.23"));
    assert_eq!(body["from_xid"], json!("alice-xid"));
    assert_eq!(body["to_xid"], json!("bob-xid"));
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
async fn ensure_account_returns_existing_account(pool: PgPool) {
    let redis = redis_or_skip!();
    DugongAccount::create(&pool, "555", "alice", "0xacct555")
        .await
        .expect("create");

    let server = MockServer::start().await;
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
                .uri("/api/auth/twitter/ensure-account")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "access_token": "tok123" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["user"]["id"], json!("555"));
    assert_eq!(body["accessToken"], json!("tok123"));
    assert_eq!(body["dugongAccount"]["sui_object_id"], json!("0xacct555"));
    // The account already existed, so no init tx was submitted.
    assert_eq!(body["createdAccountTxDigest"], json!(null));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn ensure_account_rejects_invalid_token(pool: PgPool) {
    let redis = redis_or_skip!();
    let server = MockServer::start().await;
    // X rejects the token, so account assurance must not run.
    Mock::given(method("GET"))
        .and(path("/2/users/me"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "title": "Unauthorized",
            "status": 401
        })))
        .mount(&server)
        .await;

    let mut config = test_config();
    config.twitter_api_base = server.uri();
    let app = build_router(app_state(config, pool.clone(), redis));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/twitter/ensure-account")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "access_token": "bad-token" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    // No account row should have been created for the (unverifiable) caller.
    assert!(DugongAccount::find_by_x_user_id(&pool, "555")
        .await
        .expect("lookup")
        .is_none());
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

/// Issue a backend session token for `xid` using the test config's secret.
fn test_session_token(config: &dugong_core::config::Config, xid: &str) -> String {
    dugong_core::session::issue(
        config.session_token_secret().unwrap(),
        xid,
        std::time::Duration::from_secs(3600),
    )
    .unwrap()
}

/// Seed an encrypted stored refresh token for `xid` using the test config's key.
async fn seed_refresh_token(
    config: &dugong_core::config::Config,
    pool: &PgPool,
    xid: &str,
    refresh: &str,
) {
    let enc = dugong_core::crypto::seal(config.token_encryption_key().unwrap(), refresh).unwrap();
    TwitterOAuthToken::upsert(pool, xid, &enc, None, None, Some("offline.access"))
        .await
        .unwrap();
}

#[sqlx::test(migrations = "../core/migrations")]
async fn secure_link_wallet_refreshes_then_reaches_enclave(pool: PgPool) {
    let redis = redis_or_skip!();
    // Happy auth path: a valid session + a stored refresh token means the route
    // mints a FRESH Twitter token and forwards it to the enclave. The enclave mock
    // returns 500 so we stop before on-chain submission (no Sui mock here), proving
    // the request got past auth+mint and reached the enclave (not a re-auth bail).
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
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/process_secure_link_wallet"))
        .respond_with(ResponseTemplate::new(500).set_body_string("enclave down"))
        .mount(&server)
        .await;

    let mut config = test_config();
    config.enclave_url = server.uri();
    config.twitter_api_base = server.uri();

    seed_refresh_token(&config, &pool, "1", "stored-refresh").await;
    let session = test_session_token(&config, "1");
    let app = build_router(app_state(config, pool, redis));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/link-wallet/submit")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {session}"))
                .body(Body::from(
                    json!({
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
    assert_eq!(body["reauth_required"], json!(false));
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("Verification failed"));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn secure_link_wallet_without_session_requires_reauth(pool: PgPool) {
    let redis = redis_or_skip!();
    // No Authorization header → unauthenticated → re-auth signal, and the enclave
    // is never contacted.
    let config = test_config();
    let app = build_router(app_state(config, pool, redis));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/link-wallet/submit")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
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
    assert_eq!(body["reauth_required"], json!(true));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn secure_link_wallet_xid_mismatch_rejected(pool: PgPool) {
    let redis = redis_or_skip!();
    // Session is for xid "1" but the signed message is for xid "999": reject so a
    // caller cannot link a wallet on behalf of another X account.
    let config = test_config();
    seed_refresh_token(&config, &pool, "1", "stored-refresh").await;
    let session = test_session_token(&config, "1");
    let app = build_router(app_state(config, pool, redis));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/link-wallet/submit")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {session}"))
                .body(Body::from(
                    json!({
                        "wallet_address": "0xwallet",
                        "wallet_signature": "c2ln",
                        "message": "Link XID:999 to wallet 0xwallet at 0",
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
    assert_eq!(body["reauth_required"], json!(false));
    assert!(body["error"].as_str().unwrap().contains("does not match"));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn secure_link_wallet_missing_refresh_token_requires_reauth(pool: PgPool) {
    let redis = redis_or_skip!();
    // Valid session but NO stored refresh token → cannot mint a fresh token → the
    // user must re-authenticate. The enclave is never contacted.
    let config = test_config();
    let session = test_session_token(&config, "1");
    let app = build_router(app_state(config, pool, redis));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/link-wallet/submit")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {session}"))
                .body(Body::from(
                    json!({
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
    assert_eq!(body["reauth_required"], json!(true));
}
