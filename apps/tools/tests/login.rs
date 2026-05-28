//! Integration tests for the TwitterAPI.io login flow against a mock server.

use base64::Engine;
use dugong_tools::login::{fetch_login_cookie, LoginRequest};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Build a base64-encoded cookie payload the way TwitterAPI.io returns it.
fn encode_cookie(json: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
}

fn sample_request() -> LoginRequest {
    LoginRequest {
        user_name: "bot".to_string(),
        email: "bot@example.com".to_string(),
        password: "hunter2".to_string(),
        proxy: "http://user:pass@127.0.0.1:8080".to_string(),
        totp_secret: Some("BASE32SEED".to_string()),
    }
}

async fn mock_login(server: &MockServer, status: u16, body: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/twitter/user_login_v2"))
        .and(header("X-API-Key", "test-key"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn returns_authenticated_cookie_on_success() {
    let server = MockServer::start().await;
    let cookie = encode_cookie(r#"{"auth_token":"token","guest_id":"v1%3A123"}"#);
    mock_login(
        &server,
        200,
        serde_json::json!({ "status": "success", "login_cookie": cookie }),
    )
    .await;

    let result = fetch_login_cookie(&server.uri(), "test-key", &sample_request()).await;

    assert_eq!(result.unwrap(), cookie);
}

#[tokio::test]
async fn rejects_guest_session_cookie() {
    let server = MockServer::start().await;
    // No auth_token / kdt → guest session that cannot post tweets.
    let cookie = encode_cookie(r#"{"guest_id":"v1%3A123","att":"abc"}"#);
    mock_login(
        &server,
        200,
        serde_json::json!({ "status": "success", "login_cookie": cookie }),
    )
    .await;

    let err = fetch_login_cookie(&server.uri(), "test-key", &sample_request())
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("guest session"), "unexpected error: {err}");
}

#[tokio::test]
async fn errors_on_non_success_status() {
    let server = MockServer::start().await;
    mock_login(
        &server,
        200,
        serde_json::json!({ "status": "error", "message": "bad credentials" }),
    )
    .await;

    let err = fetch_login_cookie(&server.uri(), "test-key", &sample_request())
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("bad credentials"), "unexpected error: {err}");
}

#[tokio::test]
async fn errors_on_http_failure() {
    let server = MockServer::start().await;
    mock_login(
        &server,
        500,
        serde_json::json!({ "status": "error", "message": "boom" }),
    )
    .await;

    let err = fetch_login_cookie(&server.uri(), "test-key", &sample_request())
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("HTTP error"), "unexpected error: {err}");
}

#[tokio::test]
async fn errors_when_cookie_missing() {
    let server = MockServer::start().await;
    mock_login(&server, 200, serde_json::json!({ "status": "success" })).await;

    let err = fetch_login_cookie(&server.uri(), "test-key", &sample_request())
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("no login cookie"), "unexpected error: {err}");
}
