//! End-to-end smoke test for the TwitterAPI.io posting flow.
//!
//! Posts a root tweet with a clear, human-readable body, then posts a
//! self-reply to it via `reply_to_tweet_id`. This exercises exactly the
//! same `create_tweet_v2` call the processor worker uses, so if this passes
//! the bot can post account/transfer replies.
//!
//! Run:
//!   cargo run --bin dugong-test-tweet
//!
//! Reads from the environment / `.env` (same vars the server uses):
//!   TWITTERAPI_IO_API_KEY        (required)
//!   TWITTERAPI_IO_PROXY          (required)
//!   TWITTERAPI_IO_LOGIN_COOKIES  (required)
//!
//! Each run embeds a unique timestamp so Twitter does not reject the tweet
//! as duplicate content (HTTP 422).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
struct CreateTweetRequest {
    login_cookies: String,
    tweet_text: String,
    proxy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_tweet_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateTweetResponse {
    status: String,
    #[serde(alias = "message")]
    msg: Option<String>,
    tweet_id: Option<String>,
}

fn required(name: &str) -> Result<String> {
    let value = env::var(name)
        .with_context(|| format!("{name} must be set"))?
        .trim()
        .to_string();
    if value.is_empty() || value.starts_with("replace_with_") {
        bail!("{name} must be set to a real value (found empty or placeholder)");
    }
    Ok(value)
}

struct Poster {
    http: reqwest::Client,
    api_key: String,
    login_cookies: String,
    proxy: String,
}

impl Poster {
    async fn post(&self, text: &str, reply_to: Option<&str>) -> Result<String> {
        let body = CreateTweetRequest {
            login_cookies: self.login_cookies.clone(),
            tweet_text: text.to_string(),
            proxy: self.proxy.clone(),
            reply_to_tweet_id: reply_to.map(|s| s.to_string()),
        };

        let resp = self
            .http
            .post("https://api.twitterapi.io/twitter/create_tweet_v2")
            .header("X-API-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send create_tweet_v2 request")?;

        let http_status = resp.status();
        let raw = resp
            .text()
            .await
            .context("Failed to read create_tweet_v2 response body")?;

        if !http_status.is_success() {
            bail!("create_tweet_v2 HTTP error ({http_status}): {raw}");
        }

        let parsed: CreateTweetResponse =
            serde_json::from_str(&raw).context("Failed to parse create_tweet_v2 response")?;

        if !parsed.status.eq_ignore_ascii_case("success") {
            bail!(
                "create_tweet_v2 failed: {} (raw: {raw})",
                parsed
                    .msg
                    .unwrap_or_else(|| "no message field in response".to_string())
            );
        }

        parsed
            .tweet_id
            .filter(|id| !id.is_empty())
            .context("create_tweet_v2 succeeded but returned no tweet_id")
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let poster = Poster {
        http: reqwest::Client::new(),
        api_key: required("TWITTERAPI_IO_API_KEY")?,
        login_cookies: required("TWITTERAPI_IO_LOGIN_COOKIES")?,
        // Strip any trailing path; TwitterAPI.io wants exactly host:port.
        proxy: required("TWITTERAPI_IO_PROXY")?
            .trim_end_matches('/')
            .to_string(),
    };

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let root_text = format!(
        "gm from the Dugong team \u{1F44B}\n\n\
         Hello! My fellow Sui developers, Sending and receiving on Sui should feel as easy as a tweet. \
         We're building toward that, and we're glad you're here.\n\n\
         #{nonce}"
    );

    eprintln!("Posting root tweet ...");
    let root_id = poster.post(&root_text, None).await?;
    eprintln!("  root tweet posted: https://x.com/i/status/{root_id}");

    let reply_text = format!(
        "Thanks for following along \u{2014} more good things on the way. \
         Stay tuned! \u{1F420}\n\n#{nonce}"
    );

    eprintln!("Posting self-reply ...");
    let reply_id = poster.post(&reply_text, Some(&root_id)).await?;
    eprintln!("  reply posted: https://x.com/i/status/{reply_id}");

    eprintln!("\nFlow OK.");
    println!("root_tweet_id={root_id}");
    println!("reply_tweet_id={reply_id}");

    Ok(())
}
