use anyhow::{Context, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub twitterapi_io_api_key: String,
    pub backend_url: String,
    pub poll_interval_seconds: u64,
    pub twitter_mention: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();

        let twitterapi_io_api_key =
            env::var("TWITTERAPI_IO_API_KEY").context("TWITTERAPI_IO_API_KEY must be set")?;

        let backend_url =
            env::var("BACKEND_URL").unwrap_or_else(|_| "http://localhost:43001".to_string());

        let poll_interval_seconds = env::var("POLL_INTERVAL_SECONDS")
            .unwrap_or_else(|_| "10".to_string())
            .parse::<u64>()
            .context("POLL_INTERVAL_SECONDS must be a valid number")?;

        let twitter_mention =
            env::var("TWITTER_MENTION").unwrap_or_else(|_| "@DugongWallet".to_string());

        Ok(Self {
            twitterapi_io_api_key,
            backend_url,
            poll_interval_seconds,
            twitter_mention,
        })
    }
}
