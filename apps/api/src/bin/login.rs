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
use dugong_api::twitter_session::{login_cookie_auth_status, LoginCookieAuthStatus};
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Serialize)]
struct LoginRequest {
    user_name: String,
    email: String,
    password: String,
    proxy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    totp_secret: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginResponse {
    // The endpoint documents `login_cookie` (singular); accept the plural
    // form too in case the API ever returns it that way.
    #[serde(alias = "login_cookies")]
    login_cookie: Option<String>,
    status: String,
    // user_login_v2 returns errors under `message`; other endpoints use `msg`.
    #[serde(alias = "message")]
    msg: Option<String>,
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

fn redact_login_cookie(body: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.to_string();
    };

    for key in ["login_cookie", "login_cookies"] {
        if let Some(cookie) = value.get_mut(key) {
            *cookie = serde_json::Value::String("<redacted>".to_string());
        }
    }

    serde_json::to_string(&value).unwrap_or_else(|_| "<redacted login response>".to_string())
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

    let response = reqwest::Client::new()
        .post("https://api.twitterapi.io/twitter/user_login_v2")
        .header("X-API-Key", &api_key)
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .context("Failed to send user_login_v2 request")?;

    let http_status = response.status();
    let body = response
        .text()
        .await
        .context("Failed to read login response body")?;

    if !http_status.is_success() {
        bail!("user_login_v2 HTTP error ({http_status}): {body}");
    }

    let parsed: LoginResponse =
        serde_json::from_str(&body).context("Failed to parse login response")?;

    if !parsed.status.eq_ignore_ascii_case("success") {
        eprintln!(
            "Raw response from user_login_v2:\n{}",
            redact_login_cookie(&body)
        );
        bail!(
            "user_login_v2 failed (status={}): {}",
            parsed.status,
            parsed
                .msg
                .unwrap_or_else(|| "no msg field in response".to_string())
        );
    }

    let cookie = parsed
        .login_cookie
        .filter(|c| !c.is_empty())
        .context("user_login_v2 succeeded but returned no login cookie")?;

    // A successful response can still hand back a *guest* session (only
    // guest_id / __cf_bm / att, no auth_token). Twitter rejects tweet
    // creation from guest sessions with HTTP 422, so verify the cookie is
    // actually authenticated before telling the user it worked.
    match login_cookie_auth_status(&cookie) {
        LoginCookieAuthStatus::Authenticated => {}
        LoginCookieAuthStatus::Unauthenticated => {
            eprintln!(
                "Raw response from user_login_v2:\n{}",
                redact_login_cookie(&body)
            );
            bail!(
                "user_login_v2 returned a guest session (no auth_token/kdt), so it cannot post. \
                 Set TWITTER_BOT_TOTP_SECRET to the X 2FA base32 seed, rerun this command, \
                 and make sure the account has no pending verification/lock."
            );
        }
        LoginCookieAuthStatus::Unknown => {
            eprintln!(
                "Raw response from user_login_v2:\n{}",
                redact_login_cookie(&body)
            );
            bail!(
                "user_login_v2 returned a login cookie, but its format could not be verified. \
                 Refusing to save it because unauthenticated cookies fail create_tweet_v2 with HTTP 422."
            );
        }
    }

    eprintln!("Success (authenticated session). Paste the line below into apps/api/.env:\n");
    println!("TWITTERAPI_IO_LOGIN_COOKIES={cookie}");

    Ok(())
}
