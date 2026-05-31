//! Shared fixtures for the dugong-indexer integration tests.

use dugong_core::config::Config;
use dugong_indexer::types::SuiEvent;
use serde_json::{json, Value};

/// Build a `SuiEvent` with the given event type, parsed JSON payload, and a
/// deterministic tx digest used for transfer/bet/market dedup.
pub fn sui_event(event_type: &str, tx_digest: &str, parsed_json: Value) -> SuiEvent {
    serde_json::from_value(json!({
        "id": { "txDigest": tx_digest, "eventSeq": "0" },
        "packageId": "0x9",
        "transactionModule": "events",
        "sender": "0xsender",
        "type": event_type,
        "parsedJson": parsed_json,
        "bcs": null,
        "timestampMs": "1700000000000"
    }))
    .expect("construct SuiEvent fixture")
}

/// Minimal `Config` for the event fetcher, with `sui_rpc_url` and
/// `dugong_package_id` overridable by the caller (the only fields it reads).
#[allow(dead_code)]
pub fn test_config(sui_rpc_url: String, package_id: String) -> Config {
    Config {
        port: 0,
        log_level: "info".to_string(),
        database_url: "postgres://localhost/dugong_test".to_string(),
        redis_url: "redis://localhost:6379".to_string(),

        twitterapi_io_api_key: "test-twitterapi-key".to_string(),
        twitterapi_io_login_cookies: None,
        twitterapi_io_proxy: None,
        twitter_webhook_secret: None,

        twitter_oauth2_client_id: "test-client-id".to_string(),
        twitter_oauth2_client_secret: "test-client-secret".to_string(),
        twitter_oauth2_redirect_uri: "http://localhost/callback".to_string(),

        sui_rpc_url,
        dugong_witness_package_id: package_id.clone(),
        // The indexer filters events on dugong_event_package_id; mirror the
        // caller-supplied id here so existing tests keep targeting it.
        dugong_event_package_id: package_id.clone(),
        dugong_package_id: package_id,
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
