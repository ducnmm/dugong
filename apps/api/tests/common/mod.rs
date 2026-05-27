//! Shared fixtures for the dugong-api integration tests.
//!
//! Tests use `#[sqlx::test]` (which provisions an isolated migrated Postgres
//! per test) for the database, `wiremock` for the enclave/Twitter/Enoki HTTP
//! dependencies, and a live Redis for the webhook dedup/queue path. Point
//! Redis at a throwaway instance via `REDIS_URL` (defaults to the local test
//! container on port 56379, see docs/local-dev-guide.md).

use std::sync::{Arc, OnceLock};

use dugong_api::webhook::handler::AppState;
use dugong_core::clients::redis_client::RedisClient;
use dugong_core::config::Config;
use dugong_core::constants::redis;
use sqlx::PgPool;
use tokio::sync::Mutex;

/// Process-wide lock serializing tests that touch the shared Redis tweet queue.
///
/// `queue:tweets` is a single hardcoded key, so concurrent pushers/poppers in
/// the same test binary would steal each other's items. Cargo runs separate
/// integration-test binaries sequentially, so a per-process lock is enough to
/// make the queue tests deterministic.
#[allow(dead_code)]
pub async fn lock_queue() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().await
}

/// Drain any leftover items from the shared tweet queue.
#[allow(dead_code)]
pub async fn drain_queue(redis: &RedisClient) {
    while redis.pop_queue(redis::QUEUE_TWEETS).await.unwrap().is_some() {}
}

/// Redis URL for tests: honor `REDIS_URL` (set in CI), else the local
/// throwaway container documented in `docs/local-dev-guide.md`.
#[allow(dead_code)]
pub fn test_redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:56379".to_string())
}

/// Build a `Config` with deterministic placeholders. Callers override the
/// external base URLs they want pointed at a mock server.
#[allow(dead_code)]
pub fn test_config() -> Config {
    Config {
        port: 0,
        log_level: "info".to_string(),
        database_url: "postgres://localhost/dugong_test".to_string(),
        redis_url: test_redis_url(),

        twitterapi_io_api_key: "test-twitterapi-key".to_string(),
        twitterapi_io_login_cookies: Some("test-login-cookies".to_string()),
        twitterapi_io_proxy: Some("http://proxy.local:8080".to_string()),
        twitter_webhook_secret: Some("test-webhook-secret".to_string()),

        twitter_oauth2_client_id: "test-client-id".to_string(),
        twitter_oauth2_client_secret: "test-client-secret".to_string(),
        twitter_oauth2_redirect_uri: "http://localhost/callback".to_string(),

        sui_rpc_url: "http://localhost".to_string(),
        dugong_package_id: "0x1".to_string(),
        dugong_registry_id: "0x2".to_string(),
        enclave_config_id: "0x3".to_string(),
        enclave_object_id: "0x4".to_string(),

        enoki_api_key: "test-enoki-key".to_string(),
        enoki_network: "testnet".to_string(),

        enoki_base_url: "http://localhost".to_string(),
        twitter_api_base: "http://localhost".to_string(),
        twitterapi_io_base: "http://localhost".to_string(),

        backend_signer_private_key: "test-signer-key".to_string(),

        enclave_url: "http://localhost:43000".to_string(),

        market_registry_id: "0x0".to_string(),
        market_treasury_account_id: "0x0".to_string(),
        market_default_fee_bps: 100,

        indexer_poll_interval_ms: 5000,
        indexer_batch_size: 50,
        enable_indexer: false,
    }
}

/// Connect to the test Redis, returning `None` if it is unreachable so tests
/// can skip rather than hard-fail when no Redis is provisioned locally.
#[allow(dead_code)]
pub async fn try_redis() -> Option<RedisClient> {
    RedisClient::new(&test_redis_url()).await.ok()
}

/// Build an `AppState` around a `sqlx::test` pool, a live Redis, and the
/// given config (with whatever mock base URLs the caller set).
#[allow(dead_code)]
pub fn app_state(config: Config, db: PgPool, redis: RedisClient) -> Arc<AppState> {
    Arc::new(AppState { config, db, redis })
}
