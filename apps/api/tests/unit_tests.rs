mod common;

#[cfg(test)]
mod webhook_tests {
    use dugong_api::webhook::signature::generate_crc_response;

    #[test]
    fn test_generate_crc_response() {
        let crc_token = "test_token";
        let consumer_secret = "test_secret";

        let result = generate_crc_response(crc_token, consumer_secret);

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.starts_with("sha256="));
        assert!(response.len() > 7); // "sha256=" + base64
    }

    #[test]
    fn test_generate_crc_response_consistency() {
        let crc_token = "test_token";
        let consumer_secret = "test_secret";

        let response1 = generate_crc_response(crc_token, consumer_secret).unwrap();
        let response2 = generate_crc_response(crc_token, consumer_secret).unwrap();

        assert_eq!(response1, response2);
    }

    #[test]
    fn test_generate_crc_response_different_tokens() {
        let consumer_secret = "test_secret";

        let response1 = generate_crc_response("token1", consumer_secret).unwrap();
        let response2 = generate_crc_response("token2", consumer_secret).unwrap();

        assert_ne!(response1, response2);
    }
}

#[cfg(test)]
mod error_tests {
    use dugong_api::error::BackendError;

    #[test]
    fn test_error_display() {
        let err = BackendError::WebhookValidation("Invalid signature".to_string());
        assert_eq!(
            err.to_string(),
            "Webhook validation failed: Invalid signature"
        );
    }

    #[test]
    fn test_error_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("Test error");
        let backend_err: BackendError = anyhow_err.into();

        match backend_err {
            BackendError::Internal(msg) => assert_eq!(msg, "Test error"),
            _ => panic!("Expected Internal error"),
        }
    }
}
