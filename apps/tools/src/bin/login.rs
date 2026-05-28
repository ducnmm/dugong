//! One-off helper to mint a TwitterAPI.io session cookie.
//!
//! Calls `POST /twitter/user_login_v2` with the bot account's credentials and
//! prints a `TWITTERAPI_IO_LOGIN_COOKIES=...` line ready to paste into `.env`.
//!
//! Run:
//!   cargo run --bin dugong-login
//!
//! Reads everything from the environment / `.env` (no secrets on the CLI):
//!   TWITTERAPI_IO_API_KEY   (required) - your TwitterAPI.io key
//!   TWITTERAPI_IO_PROXY     (required) - residential proxy, http://user:pass@ip:port
//!   TWITTER_BOT_USERNAME    (required) - bot account @handle, without the @
//!   TWITTER_BOT_EMAIL       (required) - bot account email
//!   TWITTER_BOT_PASSWORD    (required) - bot account password
//!   TWITTER_BOT_TOTP_SECRET (optional) - base32 2FA seed (NOT a 6-digit code);
//!                                        strongly recommended to avoid login failures

use anyhow::{bail, Context, Result};
use dugong_tools::login::{fetch_login_cookie, LoginRequest, TWITTERAPI_IO_BASE_URL};
use std::env;

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

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let api_key = required("TWITTERAPI_IO_API_KEY")?;
    // TwitterAPI.io expects exactly http://user:pass@ip:port with no trailing
    // path; a stray `/` is rejected as a proxy connection error.
    let proxy = required("TWITTERAPI_IO_PROXY")?
        .trim_end_matches('/')
        .to_string();
    let request = LoginRequest {
        user_name: required("TWITTER_BOT_USERNAME")?,
        email: required("TWITTER_BOT_EMAIL")?,
        password: required("TWITTER_BOT_PASSWORD")?,
        proxy,
        totp_secret: env::var("TWITTER_BOT_TOTP_SECRET")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty() && !v.starts_with("replace_with_")),
    };

    if request.totp_secret.is_none() {
        eprintln!(
            "warning: TWITTER_BOT_TOTP_SECRET is not set; login may fail if the \
             account has 2FA enabled"
        );
    }

    eprintln!("Logging in as @{} ...", request.user_name);

    let cookie = fetch_login_cookie(TWITTERAPI_IO_BASE_URL, &api_key, &request).await?;

    eprintln!("Success (authenticated session). Paste the line below into apps/api/.env:\n");
    println!("TWITTERAPI_IO_LOGIN_COOKIES={cookie}");

    Ok(())
}
