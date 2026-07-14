//! One-time authorization for the bot account's official X API posting.
//!
//! NOT needed when OAuth 1.0a keys are configured: with TWITTER_API_KEY/
//! TWITTER_API_SECRET + TWITTER_ACCESS_TOKEN/TWITTER_ACCESS_TOKEN_SECRET set
//! (from the app's "Keys and tokens" page), the processor signs posts directly
//! and never reads the token this tool stores.
//!
//! Runs the OAuth 2.0 Authorization Code + PKCE flow with `tweet.write` scope,
//! then stores the bot's encrypted refresh/access token in `twitter_oauth_tokens`
//! (keyed by the bot's X user id). The processor reads that row to post replies
//! as the bot, refreshing automatically thereafter — so this only needs to run
//! once per bot account (or again if the token is ever revoked).
//!
//! Run:
//!   cargo run --bin dugong-bot-authorize
//!
//! Then follow the printed URL, authorize as the BOT account (@DugongWallet),
//! and paste back the redirected URL (or just its `code` value).
//!
//! Reads from the environment / `.env`:
//!   TWITTER_OAUTH2_CLIENT_ID      (required) - the X app's OAuth2 client id
//!   TWITTER_OAUTH2_CLIENT_SECRET  (required) - the X app's OAuth2 client secret
//!   DATABASE_URL                  (required) - where the token is stored
//!   TOKEN_ENCRYPTION_KEY          (required) - 32-byte key (base64 or hex) to encrypt it
//!   TWITTER_BOT_REDIRECT_URI      (optional) - a redirect URI registered on the X app;
//!                                   defaults to TWITTER_OAUTH2_REDIRECT_URI, then to
//!                                   http://localhost:43173/callback
//!   TWITTER_API_BASE_URL          (optional) - override the token endpoint host (tests)

use anyhow::{anyhow, bail, Context, Result};
use dugong_core::clients::twitter::{
    generate_pkce, generate_state, TwitterOAuth2Client, TWITTER_API_BASE_URL,
};
use std::env;
use std::io::{self, Write};

/// Scopes the bot needs: read + WRITE (to post) + user identity + a refresh token.
const BOT_SCOPES: &[&str] = &["tweet.read", "tweet.write", "users.read", "offline.access"];

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

/// Extract the `code` from either a full redirect URL (`https://.../callback?code=...&state=...`)
/// or a bare code string. When a URL is given, verify `state` matches (CSRF check).
fn extract_code(input: &str, expected_state: &str) -> Result<String> {
    let input = input.trim();
    if input.is_empty() {
        bail!("no input provided");
    }
    if input.contains("://") || input.contains("code=") {
        let url = reqwest::Url::parse(input)
            .context("could not parse the pasted value as a URL; paste the full redirect URL or just the code")?;
        let mut code = None;
        let mut state = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "code" => code = Some(value.into_owned()),
                "state" => state = Some(value.into_owned()),
                _ => {}
            }
        }
        if let Some(state) = state {
            if state != expected_state {
                bail!("state mismatch (possible CSRF): expected {expected_state}, got {state}");
            }
        }
        code.ok_or_else(|| anyhow!("no `code` parameter found in the pasted URL"))
    } else {
        Ok(input.to_string())
    }
}

fn prompt(message: &str) -> Result<String> {
    print!("{message}");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("failed to read from stdin")?;
    Ok(line.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    // With a full OAuth 1.0a key set, the processor signs posts directly and
    // never reads the OAuth 2.0 token this tool stores — running it would be
    // pointless, so say so instead of silently authorizing.
    let oauth1_configured = [
        "TWITTER_API_KEY",
        "TWITTER_API_SECRET",
        "TWITTER_ACCESS_TOKEN",
        "TWITTER_ACCESS_TOKEN_SECRET",
    ]
    .iter()
    .all(|name| required(name).is_ok());
    if oauth1_configured {
        println!(
            "NOTE: OAuth 1.0a posting keys (TWITTER_API_KEY/SECRET + \
             TWITTER_ACCESS_TOKEN/SECRET) are configured, so reply posting \
             does not use the OAuth 2.0 token this tool stores."
        );
        let answer = prompt("Run the OAuth 2.0 authorization anyway? [y/N]: ")?;
        if !answer.eq_ignore_ascii_case("y") {
            println!("Aborted — nothing to do.");
            return Ok(());
        }
    }

    let client_id = required("TWITTER_OAUTH2_CLIENT_ID")?;
    let client_secret = required("TWITTER_OAUTH2_CLIENT_SECRET")?;
    let database_url = required("DATABASE_URL")?;
    let encryption_key = {
        let raw = required("TOKEN_ENCRYPTION_KEY")?;
        dugong_core::config::parse_encryption_key(&raw)
            .context("TOKEN_ENCRYPTION_KEY is invalid")?
    };
    let redirect_uri = env::var("TWITTER_BOT_REDIRECT_URI")
        .or_else(|_| env::var("TWITTER_OAUTH2_REDIRECT_URI"))
        .unwrap_or_else(|_| "http://localhost:43173/callback".to_string())
        .trim()
        .to_string();
    let api_base =
        env::var("TWITTER_API_BASE_URL").unwrap_or_else(|_| TWITTER_API_BASE_URL.to_string());

    let oauth = TwitterOAuth2Client::from_parts(client_id, client_secret, api_base);

    // 1. Build and print the authorization URL.
    let pkce = generate_pkce();
    let state = generate_state();
    let auth_url = oauth.authorize_url(&redirect_uri, BOT_SCOPES, &state, &pkce.challenge);

    println!("\n=== Dugong bot authorization ===\n");
    println!("1. Open this URL in a browser where you are logged in as the BOT account:");
    println!("\n{auth_url}\n");
    println!("2. Approve the '{}' permissions.", BOT_SCOPES.join(", "));
    println!(
        "3. You'll be redirected to {redirect_uri} (the page may not load — that's fine).\n   \
         Copy the full redirected URL from the address bar.\n"
    );

    // 2. Read back the redirect URL / code and exchange it.
    let pasted = prompt("Paste the redirected URL (or just the code): ")?;
    let code = extract_code(&pasted, &state)?;

    let tokens = oauth
        .exchange_code(&code, &pkce.verifier, &redirect_uri)
        .await
        .context("failed to exchange authorization code for tokens")?;

    if tokens.refresh_token.is_none() {
        bail!(
            "the token response has no refresh_token — ensure `offline.access` is granted \
             (and that the X app is a confidential client)."
        );
    }

    // 3. Identify the bot account and persist the encrypted token.
    let user = oauth
        .get_user_info(&tokens.access_token)
        .await
        .context("failed to fetch bot user info with the new token")?;

    let pool = dugong_core::db::create_pool(&database_url)
        .await
        .context("failed to connect to DATABASE_URL")?;
    dugong_core::oauth::store_tokens(&pool, &encryption_key, &user.id, &tokens)
        .await
        .context("failed to store bot token")?;

    println!(
        "\n✓ Authorized and stored token for @{} (id {}).",
        user.username, user.id
    );
    println!("\nSet this in the processor's environment:\n");
    println!("  TWITTER_BOT_USER_ID={}", user.id);
    println!(
        "\nThe processor will now post replies as @{} via the official X API.",
        user.username
    );

    Ok(())
}
