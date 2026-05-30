use crate::twitter_session::ensure_authenticated_login_cookie;
use anyhow::{Context, Result};
use std::env;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Config {
    // Server
    pub port: u16,
    pub log_level: String,

    // Database
    pub database_url: String,

    // Redis
    pub redis_url: String,

    // TwitterAPI.io
    pub twitterapi_io_api_key: String,
    pub twitterapi_io_login_cookies: Option<String>,
    pub twitterapi_io_proxy: Option<String>,
    pub enable_twitter_replies: bool,
    pub twitter_webhook_secret: Option<String>,

    // Twitter OAuth 2.0 (for user authentication)
    pub twitter_oauth2_client_id: String,
    pub twitter_oauth2_client_secret: String,
    pub twitter_oauth2_redirect_uri: String,

    // Sui
    pub sui_rpc_url: String,
    pub dugong_package_id: String,
    pub dugong_registry_id: String,
    pub enclave_config_id: String,
    pub enclave_object_id: String,

    // Enoki (gas sponsorship)
    pub enoki_api_key: String,
    pub enoki_network: String,

    // Backend signer
    pub backend_signer_private_key: String,

    // Enclave
    pub enclave_url: String,

    // Indexer
    pub indexer_poll_interval_ms: u64,
    pub indexer_batch_size: u64,
    pub enable_indexer: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Config {
            // Server
            port: env::var("PORT")
                .unwrap_or_else(|_| "43001".to_string())
                .parse()
                .context("PORT must be a valid u16")?,
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),

            // Database
            database_url: env::var("DATABASE_URL").context("DATABASE_URL must be set")?,

            // Redis
            redis_url: env::var("REDIS_URL").context("REDIS_URL must be set")?,

            // TwitterAPI.io
            twitterapi_io_api_key: env::var("TWITTERAPI_IO_API_KEY")
                .context("TWITTERAPI_IO_API_KEY must be set")?,
            twitterapi_io_login_cookies: optional_env("TWITTERAPI_IO_LOGIN_COOKIES"),
            twitterapi_io_proxy: optional_env("TWITTERAPI_IO_PROXY"),
            enable_twitter_replies: env_flag("ENABLE_TWITTER_REPLIES", false),
            twitter_webhook_secret: optional_env("TWITTER_WEBHOOK_SECRET"),

            // Twitter OAuth 2.0
            twitter_oauth2_client_id: env::var("TWITTER_OAUTH2_CLIENT_ID")
                .context("TWITTER_OAUTH2_CLIENT_ID must be set")?,
            twitter_oauth2_client_secret: env::var("TWITTER_OAUTH2_CLIENT_SECRET")
                .context("TWITTER_OAUTH2_CLIENT_SECRET must be set")?,
            twitter_oauth2_redirect_uri: env::var("TWITTER_OAUTH2_REDIRECT_URI")
                .unwrap_or_else(|_| "http://localhost:43173/callback".to_string()),

            // Sui
            sui_rpc_url: env::var("SUI_RPC_URL")
                .unwrap_or_else(|_| "https://fullnode.testnet.sui.io:443".to_string()),
            dugong_package_id: env::var("DUGONG_PACKAGE_ID")
                .context("DUGONG_PACKAGE_ID must be set")?,
            dugong_registry_id: env::var("DUGONG_REGISTRY_ID")
                .context("DUGONG_REGISTRY_ID must be set")?,
            enclave_config_id: env::var("ENCLAVE_CONFIG_ID")
                .context("ENCLAVE_CONFIG_ID must be set")?,
            enclave_object_id: env::var("ENCLAVE_ID")
                .or_else(|_| env::var("ENCLAVE_OBJECT_ID"))
                .context("ENCLAVE_ID or ENCLAVE_OBJECT_ID must be set to the enclave shared object (NOT the config object)")?,

            // Enoki
            enoki_api_key: env::var("ENOKI_API_KEY").context("ENOKI_API_KEY must be set")?,
            enoki_network: env::var("ENOKI_NETWORK").unwrap_or_else(|_| "testnet".to_string()),

            // Backend signer
            backend_signer_private_key: env::var("BACKEND_SIGNER_PRIVATE_KEY")
                .context("BACKEND_SIGNER_PRIVATE_KEY must be set")?,

            // Enclave
            enclave_url: env::var("ENCLAVE_URL")
                .unwrap_or_else(|_| "http://localhost:43000".to_string()),

            // Indexer
            indexer_poll_interval_ms: env::var("INDEXER_POLL_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5000),
            indexer_batch_size: env::var("INDEXER_BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50),
            enable_indexer: env::var("ENABLE_INDEXER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false), // Default: disabled in API server
        })
    }

    /// Ensure reply credentials are present when reply posting is enabled.
    pub fn ensure_reply_capable(&self) -> Result<()> {
        if !self.enable_twitter_replies {
            return Ok(());
        }

        let login_cookies = self.twitterapi_io_login_cookies.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "TWITTERAPI_IO_LOGIN_COOKIES must be set to post reply tweets \
                 (unset ENABLE_TWITTER_REPLIES if this process should not reply)"
            )
        })?;
        ensure_authenticated_login_cookie(login_cookies)?;

        if self.twitterapi_io_proxy.is_none() {
            anyhow::bail!("TWITTERAPI_IO_PROXY must be set to post reply tweets");
        }
        Ok(())
    }
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.starts_with("replace_with_"))
}

fn env_flag(name: &str, default: bool) -> bool {
    env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}
