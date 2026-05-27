//! Shared test fixtures for the dugong-core integration tests.

use dugong_core::config::Config;

/// Build a `Config` populated with deterministic placeholder values.
///
/// Clients only read a few fields each, so the rest are filled with
/// obviously-fake values that are safe to log.
#[allow(dead_code)]
pub fn test_config() -> Config {
    Config {
        port: 0,
        log_level: "info".to_string(),
        database_url: "postgres://localhost/dugong_test".to_string(),
        redis_url: "redis://localhost:6379".to_string(),

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
