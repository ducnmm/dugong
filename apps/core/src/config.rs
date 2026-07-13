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

    // TwitterAPI.io — used for READS (mention polling, user lookup, campaign
    // search) via `X-API-Key`. Reply posting goes through the official X API.
    pub twitterapi_io_api_key: String,
    pub twitter_webhook_secret: Option<String>,

    // Twitter OAuth 2.0 (for user authentication)
    pub twitter_oauth2_client_id: String,
    pub twitter_oauth2_client_secret: String,
    pub twitter_oauth2_redirect_uri: String,

    // The bot account's own X user id (numeric). Reply posting authenticates as
    // this account using its stored OAuth 2.0 user-context token in
    // `twitter_oauth_tokens` (obtained once via `dugong-bot-authorize`, then
    // auto-refreshed). `None` disables official-API posting.
    pub twitter_bot_user_id: Option<String>,

    // OAuth 1.0a user-context credentials from the app's "Keys and tokens"
    // page (consumer API key/secret + the bot account's access token/secret).
    // When all four are set, reply posting signs each request with these
    // directly — no DB-stored token, no refresh, no `dugong-bot-authorize`
    // needed — and they take precedence over the OAuth 2.0 path above.
    pub twitter_api_key: Option<String>,
    pub twitter_api_secret: Option<String>,
    pub twitter_access_token: Option<String>,
    pub twitter_access_token_secret: Option<String>,

    // OAuth credential security (API only; see `ensure_token_security`).
    // 32-byte AES-256 key for encrypting refresh tokens at rest, decoded from
    // base64 or hex in `TOKEN_ENCRYPTION_KEY`. `None` when unset.
    pub token_encryption_key: Option<[u8; 32]>,
    // HMAC secret for signing backend session JWTs (`SESSION_TOKEN_SECRET`).
    pub session_token_secret: Option<String>,

    // Sui
    pub sui_rpc_url: String,
    pub dugong_package_id: String,
    /// Defining package id(s) the indexer filters events on (`MoveEventModule`),
    /// as a comma-separated list. An event type's identity is keyed by the
    /// package version that DEFINED that struct: pre-existing event structs keep
    /// the ORIGINAL (defining) id across upgrades, but event structs ADDED in an
    /// upgrade carry the UPGRADED package's id. A single `MoveEventModule` filter
    /// matches only one defining id, so to see every event you must list every
    /// defining package id — the original id, plus each upgraded id that
    /// introduced new events (e.g. reward-campaign events added in v2).
    /// Defaults to `dugong_package_id` (correct before any upgrade). Use
    /// `dugong_event_package_ids()` to read the parsed list.
    pub dugong_event_package_id: String,
    /// Package id used for the `Enclave<DUGONG>` type-argument when calling
    /// enclave-gated entry functions (init_account, link_wallet, transfer_coin).
    /// Defaults to `dugong_package_id`; override via DUGONG_WITNESS_PACKAGE_ID
    /// when the on-chain Enclave was registered under a different (older)
    /// package version than the one currently in use.
    pub dugong_witness_package_id: String,
    pub dugong_registry_id: String,
    pub enclave_config_id: String,
    pub enclave_object_id: String,

    // Enoki (gas sponsorship)
    pub enoki_api_key: String,
    pub enoki_network: String,

    // External API base URLs (overridable in tests; default to production)
    pub enoki_base_url: String,
    pub twitter_api_base: String,
    pub twitterapi_io_base: String,

    // Backend signer
    pub backend_signer_private_key: String,

    // Enclave
    pub enclave_url: String,

    // Prediction markets
    pub market_registry_id: String,
    pub market_treasury_account_id: String,
    pub market_default_fee_bps: u16,

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
            twitter_webhook_secret: optional_env("TWITTER_WEBHOOK_SECRET"),

            // Twitter OAuth 2.0
            twitter_oauth2_client_id: env::var("TWITTER_OAUTH2_CLIENT_ID")
                .context("TWITTER_OAUTH2_CLIENT_ID must be set")?,
            twitter_oauth2_client_secret: env::var("TWITTER_OAUTH2_CLIENT_SECRET")
                .context("TWITTER_OAUTH2_CLIENT_SECRET must be set")?,
            twitter_oauth2_redirect_uri: env::var("TWITTER_OAUTH2_REDIRECT_URI")
                .unwrap_or_else(|_| "http://localhost:43173/callback".to_string()),

            twitter_bot_user_id: optional_env("TWITTER_BOT_USER_ID"),

            // OAuth 1.0a posting credentials (all-or-nothing; see field docs)
            twitter_api_key: optional_env("TWITTER_API_KEY"),
            twitter_api_secret: optional_env("TWITTER_API_SECRET"),
            twitter_access_token: optional_env("TWITTER_ACCESS_TOKEN"),
            twitter_access_token_secret: optional_env("TWITTER_ACCESS_TOKEN_SECRET"),

            // OAuth credential security. Parsed (and length-validated) when present;
            // a present-but-malformed key is a hard misconfiguration. Requiredness is
            // enforced for the API binary via `ensure_token_security`, so other
            // binaries (worker/indexer) that don't refresh tokens still start.
            token_encryption_key: optional_env("TOKEN_ENCRYPTION_KEY")
                .map(|raw| parse_encryption_key(&raw))
                .transpose()?,
            session_token_secret: optional_env("SESSION_TOKEN_SECRET"),

            // Sui
            // fullnode.testnet.sui.io stopped serving JSON-RPC in July 2026
            // (gRPC only now); default to a public node that still speaks it.
            sui_rpc_url: env::var("SUI_RPC_URL")
                .unwrap_or_else(|_| "https://sui-testnet-rpc.publicnode.com".to_string()),
            dugong_package_id: env::var("DUGONG_PACKAGE_ID")
                .context("DUGONG_PACKAGE_ID must be set")?,
            // Event-filter package id (indexer): original/defining id, preserved
            // across upgrades. Falls back to DUGONG_PACKAGE_ID when unset.
            dugong_event_package_id: env::var("DUGONG_EVENT_PACKAGE_ID")
                .or_else(|_| env::var("DUGONG_PACKAGE_ID"))
                .context("DUGONG_EVENT_PACKAGE_ID or DUGONG_PACKAGE_ID must be set")?,
            dugong_witness_package_id: env::var("DUGONG_WITNESS_PACKAGE_ID")
                .or_else(|_| env::var("DUGONG_PACKAGE_ID"))
                .context("DUGONG_WITNESS_PACKAGE_ID or DUGONG_PACKAGE_ID must be set")?,
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

            // External API base URLs (default to production; overridable in tests)
            enoki_base_url: env::var("ENOKI_API_BASE_URL")
                .unwrap_or_else(|_| crate::clients::enoki::ENOKI_API_BASE_URL.to_string()),
            twitter_api_base: env::var("TWITTER_API_BASE_URL")
                .unwrap_or_else(|_| crate::clients::twitter::TWITTER_API_BASE_URL.to_string()),
            twitterapi_io_base: env::var("TWITTERAPI_IO_BASE_URL")
                .unwrap_or_else(|_| crate::clients::twitter::TWITTERAPI_IO_BASE_URL.to_string()),

            // Backend signer
            backend_signer_private_key: env::var("BACKEND_SIGNER_PRIVATE_KEY")
                .context("BACKEND_SIGNER_PRIVATE_KEY must be set")?,

            // Enclave
            enclave_url: env::var("ENCLAVE_URL")
                .unwrap_or_else(|_| "http://localhost:43000".to_string()),

            // Prediction markets
            market_registry_id: env::var("MARKET_REGISTRY_ID")
                .unwrap_or_else(|_| "0x0".to_string()),
            market_treasury_account_id: env::var("MARKET_TREASURY_ACCOUNT_ID")
                .unwrap_or_else(|_| "0x0".to_string()),
            market_default_fee_bps: env::var("MARKET_DEFAULT_FEE_BPS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100), // 1% default

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

    /// True when the complete OAuth 1.0a posting credential set (consumer
    /// key/secret + access token/secret) is configured.
    pub fn has_twitter_oauth1_credentials(&self) -> bool {
        self.twitter_api_key.is_some()
            && self.twitter_api_secret.is_some()
            && self.twitter_access_token.is_some()
            && self.twitter_access_token_secret.is_some()
    }

    /// Ensure the credentials required to post reply tweets are present.
    ///
    /// The processor worker replies to every tweet it handles via the official
    /// X API, authenticating as the bot account either with OAuth 1.0a keys
    /// (`TWITTER_API_KEY`/`TWITTER_API_SECRET` + `TWITTER_ACCESS_TOKEN`/
    /// `TWITTER_ACCESS_TOKEN_SECRET`) or with its stored OAuth 2.0 token.
    /// Missing config here is an operator error that must fail loudly at startup
    /// rather than silently dropping replies at runtime. Call this from binaries
    /// that run the processor; the indexer binary does not post replies and
    /// should not call it.
    ///
    /// For the OAuth 2.0 path this validates only that the config is present.
    /// The bot's refresh token itself must have been seeded into
    /// `twitter_oauth_tokens` (run `dugong-bot-authorize` once); a missing/dead
    /// token surfaces as a clear error on the first reply attempt.
    pub fn ensure_reply_capable(&self) -> Result<()> {
        let oauth1_set = [
            self.twitter_api_key.is_some(),
            self.twitter_api_secret.is_some(),
            self.twitter_access_token.is_some(),
            self.twitter_access_token_secret.is_some(),
        ]
        .iter()
        .filter(|set| **set)
        .count();
        if oauth1_set == 4 {
            return Ok(());
        }
        if oauth1_set > 0 {
            anyhow::bail!(
                "OAuth 1.0a posting credentials are partially configured ({oauth1_set}/4). \
                 Set all of TWITTER_API_KEY, TWITTER_API_SECRET, TWITTER_ACCESS_TOKEN and \
                 TWITTER_ACCESS_TOKEN_SECRET, or none of them to use the OAuth 2.0 path."
            );
        }
        if self.twitter_bot_user_id.is_none() {
            anyhow::bail!(
                "Posting reply tweets via the official X API requires either the OAuth 1.0a \
                 keys (TWITTER_API_KEY/TWITTER_API_SECRET + TWITTER_ACCESS_TOKEN/\
                 TWITTER_ACCESS_TOKEN_SECRET from the app's Keys and tokens page) or \
                 TWITTER_BOT_USER_ID plus a stored OAuth 2.0 token (run `dugong-bot-authorize` \
                 once). (Set ENABLE_INDEXER and run the indexer binary if this process should \
                 not reply.)"
            );
        }
        if self.token_encryption_key.is_none() {
            anyhow::bail!(
                "TOKEN_ENCRYPTION_KEY must be set to decrypt the bot's stored OAuth token \
                 for posting reply tweets"
            );
        }
        Ok(())
    }

    /// Ensure the OAuth credential-security config is present. The API binary
    /// stores and refreshes Twitter tokens, so missing `TOKEN_ENCRYPTION_KEY` or
    /// `SESSION_TOKEN_SECRET` is an operator error that must fail loudly at startup
    /// rather than at the first login/link request. Other binaries do not call this.
    pub fn ensure_token_security(&self) -> Result<()> {
        if self.token_encryption_key.is_none() {
            anyhow::bail!(
                "TOKEN_ENCRYPTION_KEY must be set to a 32-byte key (base64 or hex) \
                 to encrypt Twitter refresh tokens at rest"
            );
        }
        if self
            .session_token_secret
            .as_ref()
            .map(|s| s.len() < 16)
            .unwrap_or(true)
        {
            anyhow::bail!(
                "SESSION_TOKEN_SECRET must be set (>= 16 chars) to sign backend session tokens"
            );
        }
        Ok(())
    }

    /// The 32-byte refresh-token encryption key, or an error explaining it is unset.
    pub fn token_encryption_key(&self) -> Result<&[u8; 32]> {
        self.token_encryption_key
            .as_ref()
            .context("TOKEN_ENCRYPTION_KEY is not configured")
    }

    /// The session-token signing secret, or an error explaining it is unset.
    pub fn session_token_secret(&self) -> Result<&str> {
        self.session_token_secret
            .as_deref()
            .context("SESSION_TOKEN_SECRET is not configured")
    }

    /// Parsed list of defining package ids the indexer watches for events.
    /// Splits `dugong_event_package_id` on commas and trims; empties are dropped.
    /// See the field doc for why multiple ids are needed after an upgrade that
    /// introduces new event structs.
    pub fn dugong_event_package_ids(&self) -> Vec<String> {
        self.dugong_event_package_id
            .split(',')
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect()
    }
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.starts_with("replace_with_"))
}

/// Decode a 32-byte AES-256 key from a base64 or hex string. Accepts standard
/// base64 (e.g. `openssl rand -base64 32`) or 64-char hex; rejects any other length.
///
/// Public so out-of-process helpers (e.g. `dugong-bot-authorize`) can decode
/// `TOKEN_ENCRYPTION_KEY` the same way the server does, without duplicating the
/// rules.
pub fn parse_encryption_key(raw: &str) -> Result<[u8; 32]> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

    let bytes = BASE64
        .decode(raw.trim())
        .or_else(|_| hex::decode(raw.trim()))
        .context("TOKEN_ENCRYPTION_KEY must be valid base64 or hex")?;

    let len = bytes.len();
    bytes.try_into().map_err(|_| {
        anyhow::anyhow!("TOKEN_ENCRYPTION_KEY must decode to exactly 32 bytes, got {len}")
    })
}
