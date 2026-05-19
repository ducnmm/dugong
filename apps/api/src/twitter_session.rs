use anyhow::Result;
use base64::Engine;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginCookieAuthStatus {
    Authenticated,
    Unauthenticated,
    Unknown,
}

pub fn login_cookie_auth_status(login_cookie: &str) -> LoginCookieAuthStatus {
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(login_cookie.as_bytes())
    else {
        return LoginCookieAuthStatus::Unknown;
    };

    let Ok(value) = serde_json::from_slice::<Value>(&decoded) else {
        return LoginCookieAuthStatus::Unknown;
    };

    let Some(object) = value.as_object() else {
        return LoginCookieAuthStatus::Unknown;
    };

    if object.contains_key("auth_token") || object.contains_key("kdt") {
        return LoginCookieAuthStatus::Authenticated;
    }

    LoginCookieAuthStatus::Unauthenticated
}

pub fn ensure_authenticated_login_cookie(login_cookie: &str) -> Result<()> {
    if login_cookie_auth_status(login_cookie) == LoginCookieAuthStatus::Unauthenticated {
        anyhow::bail!(
            "TWITTERAPI_IO_LOGIN_COOKIES is a guest/unauthenticated session and cannot post tweets. \
             Set TWITTER_BOT_TOTP_SECRET to the X 2FA base32 seed, rerun `cargo run --bin dugong-login`, \
             then paste the authenticated TWITTERAPI_IO_LOGIN_COOKIES value into .env."
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{login_cookie_auth_status, LoginCookieAuthStatus};
    use base64::Engine;

    fn encode_json(json: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    }

    #[test]
    fn detects_authenticated_cookie() {
        let cookie = encode_json(r#"{"auth_token":"token","guest_id":"v1%3A123"}"#);

        assert_eq!(
            login_cookie_auth_status(&cookie),
            LoginCookieAuthStatus::Authenticated
        );
    }

    #[test]
    fn detects_guest_cookie() {
        let cookie = encode_json(r#"{"guest_id":"v1%3A123","att":"1-abc"}"#);

        assert_eq!(
            login_cookie_auth_status(&cookie),
            LoginCookieAuthStatus::Unauthenticated
        );
    }

    #[test]
    fn leaves_unknown_format_unblocked() {
        assert_eq!(
            login_cookie_auth_status("not base64 json"),
            LoginCookieAuthStatus::Unknown
        );
    }
}
