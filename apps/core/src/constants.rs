// Redis keys
pub mod redis {
    /// Queue for tweet processing
    pub const QUEUE_TWEETS: &str = "queue:tweets";

    /// Deduplication key prefix for tweets
    pub fn dedup_tweet(tweet_id: &str) -> String {
        format!("dedup:tweet:{}", tweet_id)
    }

    /// Deduplication key prefix for webhook events
    #[allow(dead_code)]
    pub fn dedup_webhook(event_id: &str) -> String {
        format!("dedup:webhook:{}", event_id)
    }

    /// Cache key prefix for account lookups
    #[allow(dead_code)]
    pub fn cache_account(xid: &str) -> String {
        format!("cache:account:{}", xid)
    }

    /// Rate limiting key prefix
    #[allow(dead_code)]
    pub fn ratelimit_user(user_id: &str) -> String {
        format!("ratelimit:user:{}", user_id)
    }

    /// Rate limiting key for the manual tweet-processing endpoint, keyed by client IP
    pub fn ratelimit_process(ip: &str) -> String {
        format!("ratelimit:process:{}", ip)
    }

    /// TTL values in seconds
    pub const TTL_DEDUP: u64 = 86400; // 24 hours
    /// Fixed window (seconds) and max requests for the manual tweet-processing endpoint
    pub const RATELIMIT_PROCESS_WINDOW: u64 = 60;
    pub const RATELIMIT_PROCESS_MAX: i64 = 10;
    #[allow(dead_code)]
    pub const TTL_CACHE: u64 = 3600; // 1 hour
}

// Event ID formats
pub mod events {
    pub fn tweet_event_id(tweet_id: &str) -> String {
        format!("tweet:{}", tweet_id)
    }
}

// Database
pub mod db {
    #[allow(dead_code)]
    pub const MAX_CONNECTIONS: u32 = 10;
}

// Server
pub mod server {
    #[allow(dead_code)]
    pub const DEFAULT_PORT: u16 = 43001;
    #[allow(dead_code)]
    pub const SHUTDOWN_TIMEOUT_SECS: u64 = 30;
}

// Sui
pub mod sui {
    #[allow(dead_code)]
    pub const TESTNET_RPC: &str = "https://fullnode.testnet.sui.io:443";
    #[allow(dead_code)]
    pub const MAINNET_RPC: &str = "https://fullnode.mainnet.sui.io:443";
}

// Enclave endpoints
pub mod enclave {
    #[allow(dead_code)]
    pub const DEFAULT_URL: &str = "http://localhost:43000";

    /// Unified tweet processing endpoint (handles all tweet-based commands)
    pub const PROCESS_TWEET_ENDPOINT: &str = "/process_tweet";

    /// For auto-creating recipient accounts (not tweet-based)
    pub const PROCESS_INIT_ACCOUNT_ENDPOINT: &str = "/process_init_account";

    /// For dApp wallet linking (not tweet-based)
    pub const PROCESS_SECURE_LINK_WALLET_ENDPOINT: &str = "/process_secure_link_wallet";

    /// Health check endpoint
    #[allow(dead_code)]
    pub const HEALTH_CHECK_ENDPOINT: &str = "/health_check";

    /// Get attestation endpoint
    #[allow(dead_code)]
    pub const GET_ATTESTATION_ENDPOINT: &str = "/get_attestation";
}
