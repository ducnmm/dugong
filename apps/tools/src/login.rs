//! TwitterAPI.io login flow shared between the `dugong-login` binary and tests.

use anyhow::{bail, Context, Result};
use dugong_core::twitter_session::{login_cookie_auth_status, LoginCookieAuthStatus};
use serde::{Deserialize, Serialize};

/// Production base URL for the TwitterAPI.io login endpoint.
pub const TWITTERAPI_IO_BASE_URL: &str = "https://api.twitterapi.io";

#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub user_name: String,
    pub email: String,
    pub password: String,
    pub proxy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totp_secret: Option<String>,
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

/// Redact the login cookie from a raw response body so it is safe to log.
pub fn redact_login_cookie(body: &str) -> String {
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

/// Call `POST {base_url}/twitter/user_login_v2`, validate the response, and
/// return an *authenticated* login cookie.
///
/// Returns an error (with the cookie redacted from any echoed body) if the
/// request fails, the API reports a non-success status, no cookie is returned,
/// or the cookie is a guest/unverifiable session that cannot post tweets.
pub async fn fetch_login_cookie(
    base_url: &str,
    api_key: &str,
    request: &LoginRequest,
) -> Result<String> {
    let url = format!("{}/twitter/user_login_v2", base_url);

    let response = reqwest::Client::new()
        .post(&url)
        .header("X-API-Key", api_key)
        .header("Content-Type", "application/json")
        .json(request)
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
        bail!(
            "user_login_v2 failed (status={}): {} | raw: {}",
            parsed.status,
            parsed
                .msg
                .unwrap_or_else(|| "no msg field in response".to_string()),
            redact_login_cookie(&body)
        );
    }

    let cookie = parsed
        .login_cookie
        .filter(|c| !c.is_empty())
        .context("user_login_v2 succeeded but returned no login cookie")?;

    // A successful response can still hand back a *guest* session (only
    // guest_id / __cf_bm / att, no auth_token). Twitter rejects tweet creation
    // from guest sessions with HTTP 422, so verify the cookie is actually
    // authenticated before handing it back.
    match login_cookie_auth_status(&cookie) {
        LoginCookieAuthStatus::Authenticated => Ok(cookie),
        LoginCookieAuthStatus::Unauthenticated => bail!(
            "user_login_v2 returned a guest session (no auth_token/kdt), so it cannot post. \
             Set TWITTER_BOT_TOTP_SECRET to the X 2FA base32 seed, rerun this command, \
             and make sure the account has no pending verification/lock. | raw: {}",
            redact_login_cookie(&body)
        ),
        LoginCookieAuthStatus::Unknown => bail!(
            "user_login_v2 returned a login cookie, but its format could not be verified. \
             Refusing to use it because unauthenticated cookies fail create_tweet_v2 with HTTP 422. \
             | raw: {}",
            redact_login_cookie(&body)
        ),
    }
}
