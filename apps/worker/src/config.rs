use anyhow::{bail, Context, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub twitterapi_io_api_key: String,
    pub backend_url: String,
    pub poll_interval_seconds: u64,
    pub max_tweets_per_poll: usize,
    pub twitter_mention: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env")).ok();

        let twitterapi_io_api_key =
            env::var("TWITTERAPI_IO_API_KEY").context("TWITTERAPI_IO_API_KEY must be set")?;

        let backend_url =
            env::var("BACKEND_URL").unwrap_or_else(|_| "http://localhost:43001".to_string());

        let poll_interval_seconds = env::var("POLL_INTERVAL_SECONDS")
            .unwrap_or_else(|_| "10".to_string())
            .parse::<u64>()
            .context("POLL_INTERVAL_SECONDS must be a valid number")?;

        let max_tweets_per_poll = parse_max_tweets_per_poll(env::var("MAX_TWEETS_PER_POLL").ok())?;

        let twitter_mention =
            env::var("TWITTER_MENTION").unwrap_or_else(|_| "@DugongWallet".to_string());

        Ok(Self {
            twitterapi_io_api_key,
            backend_url,
            poll_interval_seconds,
            max_tweets_per_poll,
            twitter_mention,
        })
    }
}

fn parse_max_tweets_per_poll(value: Option<String>) -> Result<usize> {
    let value = value.unwrap_or_else(|| "1".to_string());
    let max_tweets_per_poll = value
        .parse::<usize>()
        .context("MAX_TWEETS_PER_POLL must be a valid number")?;

    if max_tweets_per_poll == 0 {
        bail!("MAX_TWEETS_PER_POLL must be greater than 0");
    }

    Ok(max_tweets_per_poll)
}

#[cfg(test)]
mod tests {
    use super::parse_max_tweets_per_poll;

    #[test]
    fn max_tweets_per_poll_defaults_to_one() {
        assert_eq!(parse_max_tweets_per_poll(None).unwrap(), 1);
    }

    #[test]
    fn max_tweets_per_poll_accepts_positive_values() {
        assert_eq!(parse_max_tweets_per_poll(Some("5".to_string())).unwrap(), 5);
    }

    #[test]
    fn max_tweets_per_poll_rejects_zero() {
        let error = parse_max_tweets_per_poll(Some("0".to_string())).unwrap_err();
        assert!(error.to_string().contains("greater than 0"));
    }
}
