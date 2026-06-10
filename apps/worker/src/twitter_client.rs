use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TweetData {
    pub id: String,
    pub text: String,
    pub author_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwitterUser {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwitterSearchResponse {
    #[serde(default)]
    pub data: Vec<TweetData>,
    #[serde(default)]
    pub includes: Option<TwitterIncludes>,
    pub meta: Option<TwitterMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwitterIncludes {
    #[serde(default)]
    pub users: Vec<TwitterUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwitterMeta {
    pub newest_id: Option<String>,
    pub oldest_id: Option<String>,
    pub result_count: Option<i32>,
}

/// Production base URL for the TwitterAPI.io search endpoint.
pub const TWITTERAPI_IO_BASE_URL: &str = "https://api.twitterapi.io";

pub struct TwitterClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl TwitterClient {
    pub fn new(api_key: String) -> Self {
        Self::with_base_url(api_key, TWITTERAPI_IO_BASE_URL.to_string())
    }

    /// Construct a client pointed at a custom base URL (used in tests to aim
    /// the search endpoint at a mock server).
    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            api_key,
            base_url,
        }
    }

    /// Search for recent tweets mentioning a specific account
    ///
    /// # Arguments
    /// * `mention` - The account to search for (e.g., "@DugongWallet")
    /// * `since_id` - Only return tweets with ID greater than this (for pagination)
    /// * `start_time` - Only return tweets created after this time (ISO 8601 format)
    pub async fn search_mentions(
        &self,
        mention: &str,
        since_id: Option<&str>,
        start_time: Option<&str>,
    ) -> Result<TwitterSearchResponse> {
        let mention_query = if mention.starts_with('@') {
            mention.to_string()
        } else {
            format!("@{}", mention)
        };

        // Match every supported command keyword so the poller doesn't miss
        // market (predict/resolve/claim) or reward-campaign commands.
        let mut query = format!(
            "{} (send OR link OR create OR init OR predict OR bet OR resolve OR solve OR reward OR claim)",
            mention_query
        );

        let start_time = start_time
            .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
            .map(|time| time.with_timezone(&Utc));

        if let Some(start_time) = start_time {
            query.push_str(&format!(
                " since_time:{} until_time:{}",
                start_time.timestamp(),
                Utc::now().timestamp()
            ));
        }

        // Retry logic for network issues
        let mut retries = 3;
        let mut last_error = None;

        let url = format!("{}/twitter/tweet/advanced_search", self.base_url);

        while retries > 0 {
            match self
                .client
                .get(&url)
                .header("X-API-Key", &self.api_key)
                .query(&[("query", query.as_str()), ("queryType", "Latest")])
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let text = response.text().await.unwrap_or_default();
                        anyhow::bail!("TwitterAPI.io error {}: {}", status, text);
                    }

                    let response = response
                        .json::<TwitterApiSearchResponse>()
                        .await
                        .context("Failed to parse TwitterAPI.io response")?;

                    return Ok(response.into_twitter_search_response(since_id, start_time));
                }
                Err(e) => {
                    last_error = Some(e);
                    retries -= 1;
                    if retries > 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap()).context("Failed to send request to TwitterAPI.io after retries")
    }

    /// Get user info by username lookup
    pub fn get_user_by_id(&self, user_id: &str, users: &[TwitterUser]) -> Option<TwitterUser> {
        users.iter().find(|u| u.id == user_id).cloned()
    }
}

#[derive(Debug, Deserialize)]
struct TwitterApiSearchResponse {
    #[serde(default)]
    tweets: Vec<TwitterApiTweet>,
}

#[derive(Debug, Deserialize)]
struct TwitterApiTweet {
    id: String,
    text: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    author: TwitterApiAuthor,
}

#[derive(Debug, Deserialize)]
struct TwitterApiAuthor {
    id: String,
    #[serde(rename = "userName")]
    username: String,
}

impl TwitterApiSearchResponse {
    fn into_twitter_search_response(
        self,
        since_id: Option<&str>,
        start_time: Option<DateTime<Utc>>,
    ) -> TwitterSearchResponse {
        let since_id = since_id.and_then(|id| id.parse::<u128>().ok());
        let mut users = Vec::new();
        let mut data = Vec::new();
        let mut newest_id: Option<String> = None;

        for tweet in self.tweets {
            let tweet_id = tweet.id.parse::<u128>().ok();
            if let (Some(tweet_id), Some(since_id)) = (tweet_id, since_id) {
                if tweet_id <= since_id {
                    continue;
                }
            }

            if let Some(start_time) = start_time {
                let created_at =
                    DateTime::parse_from_str(&tweet.created_at, "%a %b %d %H:%M:%S %z %Y")
                        .map(|time| time.with_timezone(&Utc));
                if matches!(created_at, Ok(created_at) if created_at < start_time) {
                    continue;
                }
            }

            if newest_id
                .as_ref()
                .and_then(|id| id.parse::<u128>().ok())
                .map(|current| tweet_id.unwrap_or(0) > current)
                .unwrap_or(true)
            {
                newest_id = Some(tweet.id.clone());
            }

            users.push(TwitterUser {
                id: tweet.author.id.clone(),
                username: tweet.author.username,
            });
            data.push(TweetData {
                id: tweet.id,
                text: tweet.text,
                author_id: tweet.author.id,
            });
        }

        TwitterSearchResponse {
            meta: Some(TwitterMeta {
                newest_id,
                oldest_id: data.last().map(|tweet| tweet.id.clone()),
                result_count: Some(data.len() as i32),
            }),
            includes: Some(TwitterIncludes { users }),
            data,
        }
    }
}
