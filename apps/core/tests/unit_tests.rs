#[cfg(test)]
mod constants_tests {
    use dugong_core::constants::{events, redis};

    #[test]
    fn test_event_id_format() {
        let tweet_id = "1234567890";
        let event_id = events::tweet_event_id(tweet_id);
        assert_eq!(event_id, "tweet:1234567890");
    }

    #[test]
    fn test_redis_key_formats() {
        assert_eq!(redis::dedup_tweet("123"), "dedup:tweet:123");
        assert_eq!(redis::dedup_webhook("evt_123"), "dedup:webhook:evt_123");
        assert_eq!(redis::cache_account("user123"), "cache:account:user123");
        assert_eq!(redis::ratelimit_user("user456"), "ratelimit:user:user456");
    }

    #[test]
    fn test_redis_ttl_constants() {
        assert_eq!(redis::TTL_DEDUP, 86400);
        assert_eq!(redis::TTL_CACHE, 3600);
    }
}
