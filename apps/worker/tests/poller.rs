//! Tests for the pure tweet→webhook conversion used by the poller.

use dugong_worker::backend_client::{TweetCreateEvent, WebhookUser};
use dugong_worker::poller::{
    select_events_for_poll, split_events_for_poll, take_events_from_queue, tweets_to_events,
};
use dugong_worker::twitter_client::{TweetData, TwitterUser};
use std::collections::VecDeque;

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

fn event(id: &str) -> TweetCreateEvent {
    TweetCreateEvent {
        id_str: id.to_string(),
        text: format!("tweet {id}"),
        user: WebhookUser {
            id_str: "111".to_string(),
            screen_name: "alice".to_string(),
        },
        in_reply_to_status_id_str: None,
    }
}

#[test]
fn select_events_for_poll_keeps_configured_oldest_tweets() {
    let selected = select_events_for_poll(vec![event("300"), event("100"), event("200")], 2);

    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].id_str, "100");
    assert_eq!(selected[1].id_str, "200");
}

#[test]
fn split_events_for_poll_queues_remaining_tweets_in_order() {
    let (selected, queued) =
        split_events_for_poll(vec![event("300"), event("100"), event("200")], 2);

    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].id_str, "100");
    assert_eq!(selected[1].id_str, "200");
    let queued_ids: Vec<_> = queued.into_iter().map(|event| event.id_str).collect();
    assert_eq!(queued_ids, vec!["300"]);
}

#[test]
fn take_events_from_queue_drains_configured_batch_in_order() {
    let mut queue = VecDeque::from(vec![event("100"), event("200"), event("300")]);

    let selected = take_events_from_queue(&mut queue, 2);

    let selected_ids: Vec<_> = selected.into_iter().map(|event| event.id_str).collect();
    assert_eq!(selected_ids, vec!["100", "200"]);
    assert_eq!(
        queue.front().map(|event| event.id_str.as_str()),
        Some("300")
    );
}
