//! Tests for the pure tweet→webhook conversion used by the poller.

use dugong_worker::poller::tweets_to_events;
use dugong_worker::twitter_client::{TweetData, TwitterUser};

fn tweet(id: &str, author_id: &str, text: &str) -> TweetData {
    TweetData {
        id: id.to_string(),
        text: text.to_string(),
        author_id: author_id.to_string(),
    }
}

fn user(id: &str, username: &str) -> TwitterUser {
    TwitterUser {
        id: id.to_string(),
        username: username.to_string(),
    }
}

#[test]
fn pairs_tweets_with_authors() {
    let data = vec![tweet("100", "111", "@DugongWallet send 1 SUI to @bob")];
    let users = vec![user("111", "alice")];

    let events = tweets_to_events(&data, &users);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id_str, "100");
    assert_eq!(events[0].user.id_str, "111");
    assert_eq!(events[0].user.screen_name, "alice");
    assert!(events[0].in_reply_to_status_id_str.is_none());
}

#[test]
fn drops_tweets_with_missing_author() {
    let data = vec![tweet("100", "111", "kept"), tweet("200", "999", "dropped")];
    let users = vec![user("111", "alice")]; // no user 999

    let events = tweets_to_events(&data, &users);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id_str, "100");
}

#[test]
fn empty_input_yields_no_events() {
    assert!(tweets_to_events(&[], &[]).is_empty());
}
