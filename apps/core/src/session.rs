//! Stateless backend session tokens.
//!
//! After a successful Twitter OAuth code-exchange the API issues one of these to
//! the SPA. It is a JWT (HS256) signed with `SESSION_TOKEN_SECRET` that binds the
//! request to a **verified** `x_user_id`. Endpoints that act on a user's behalf
//! (e.g. wallet linking) verify it to recover a trusted xid without a live Twitter
//! call — an expired Twitter access token can no longer be used as proof of identity.

use anyhow::{anyhow, Context, Result};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// JWT claims for a backend session. `sub` carries the X user id (xid).
#[derive(Debug, Serialize, Deserialize)]
struct SessionClaims {
    sub: String,
    iat: usize,
    exp: usize,
}

fn now_secs() -> Result<usize> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before the unix epoch")?
        .as_secs() as usize)
}

/// Issue a session token for `xid`, valid for `ttl`.
pub fn issue(secret: &str, xid: &str, ttl: Duration) -> Result<String> {
    let iat = now_secs()?;
    let claims = SessionClaims {
        sub: xid.to_string(),
        iat,
        exp: iat + ttl.as_secs() as usize,
    };
    encode(
        &Header::default(), // HS256
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| anyhow!("failed to issue session token: {e}"))
}

/// Verify a session token and return the trusted `xid`. Rejects tokens that are
/// expired, malformed, or not signed by `secret`.
pub fn verify(secret: &str, token: &str) -> Result<String> {
    let data = decode::<SessionClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(), // HS256 + exp validation
    )
    .map_err(|e| anyhow!("invalid session token: {e}"))?;
    Ok(data.claims.sub)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-session-secret-please-change";

    #[test]
    fn issue_then_verify_recovers_xid() {
        let token = issue(SECRET, "1555054958927835137", Duration::from_secs(3600)).unwrap();
        assert_eq!(verify(SECRET, &token).unwrap(), "1555054958927835137");
    }

    #[test]
    fn wrong_secret_rejected() {
        let token = issue(SECRET, "123", Duration::from_secs(3600)).unwrap();
        assert!(verify("different-secret", &token).is_err());
    }

    #[test]
    fn expired_token_rejected() {
        // exp in the past (ttl = 0). jsonwebtoken applies a small default leeway,
        // so backdate well beyond it by issuing with zero ttl and checking it is
        // rejected once leeway passes is flaky; instead craft an already-expired token.
        let iat = now_secs().unwrap() - 10_000;
        let claims = SessionClaims {
            sub: "123".into(),
            iat,
            exp: iat + 1, // expired ~10_000s ago
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .unwrap();
        assert!(verify(SECRET, &token).is_err());
    }

    #[test]
    fn malformed_token_rejected() {
        assert!(verify(SECRET, "not-a-jwt").is_err());
    }
}
