//! Database round-trip tests for the dugong-core models.
//!
//! These use `#[sqlx::test]`, which provisions an isolated, migrated Postgres
//! database per test. A reachable Postgres is required at runtime via
//! `DATABASE_URL` (CI provides one as a service container).

use dugong_core::db::models::{
    AccountBalance, DugongAccount, EventStatus, Market, MarketBet, TwitterOAuthToken, WebhookEvent,
};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn dugong_account_create_and_lookup(pool: PgPool) {
    let created = DugongAccount::create(&pool, "user-1", "alice", "0xobj1")
        .await
        .expect("create account");
    assert_eq!(created.x_handle, "alice");

    let found = DugongAccount::find_by_x_user_id(&pool, "user-1")
        .await
        .expect("query")
        .expect("account exists");
    assert_eq!(found.id, created.id);
    assert_eq!(found.sui_object_id, "0xobj1");

    let missing = DugongAccount::find_by_x_user_id(&pool, "nobody")
        .await
        .expect("query");
    assert!(missing.is_none());
}

#[sqlx::test]
async fn dugong_account_upsert_updates_existing(pool: PgPool) {
    DugongAccount::upsert_from_indexer(&pool, "user-2", "bob", "0xobj2")
        .await
        .expect("first upsert");
    let updated = DugongAccount::upsert_from_indexer(&pool, "user-2", "bob_v2", "0xobj2b")
        .await
        .expect("second upsert");

    assert_eq!(updated.x_handle, "bob_v2");
    assert_eq!(updated.sui_object_id, "0xobj2b");

    // Only one row should exist for this x_user_id.
    let found = DugongAccount::find_by_x_user_id(&pool, "user-2")
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(found.x_handle, "bob_v2");
}

#[sqlx::test]
async fn dugong_account_link_owner_and_find(pool: PgPool) {
    DugongAccount::create(&pool, "user-3", "carol", "0xobj3")
        .await
        .expect("create");
    let linked = DugongAccount::link_owner(&pool, "user-3", "0xowner3")
        .await
        .expect("link")
        .expect("row updated");
    assert_eq!(linked.owner_address.as_deref(), Some("0xowner3"));

    let by_owner = DugongAccount::find_by_owner_address(&pool, "0xowner3")
        .await
        .expect("query")
        .expect("found");
    assert_eq!(by_owner.x_user_id, "user-3");
}

#[sqlx::test]
async fn dugong_account_search_matches_handle(pool: PgPool) {
    DugongAccount::create(&pool, "user-4", "daisy", "0xobj4")
        .await
        .expect("create");
    let results = DugongAccount::search(&pool, "@dais").await.expect("search");
    assert!(results.iter().any(|a| a.x_user_id == "user-4"));
}

#[sqlx::test]
async fn account_balance_round_trip(pool: PgPool) {
    sqlx::query("INSERT INTO account_balances (x_user_id, coin_type, balance) VALUES ($1, $2, $3)")
        .bind("user-5")
        .bind("0x2::sui::SUI")
        .bind(1_500i64)
        .execute(&pool)
        .await
        .expect("insert balance");

    let balances = AccountBalance::find_by_x_user_id(&pool, "user-5")
        .await
        .expect("query");
    assert_eq!(balances.len(), 1);
    assert_eq!(balances[0].balance, 1_500);
    assert_eq!(balances[0].coin_type, "0x2::sui::SUI");
}

#[sqlx::test]
async fn webhook_event_lifecycle(pool: PgPool) {
    let event = WebhookEvent::create(&pool, "evt-1", Some("tweet-1"), json!({"k": "v"}))
        .await
        .expect("create event");
    assert_eq!(event.status, EventStatus::Pending);
    assert!(!event.is_done());

    assert!(WebhookEvent::exists(&pool, "evt-1").await.expect("exists"));
    assert!(!WebhookEvent::exists(&pool, "evt-x")
        .await
        .expect("not exists"));

    WebhookEvent::set_processing(&pool, "evt-1")
        .await
        .expect("processing");
    WebhookEvent::set_submitting(&pool, "evt-1")
        .await
        .expect("submitting");
    WebhookEvent::set_replying(&pool, "evt-1", "0xdigest")
        .await
        .expect("replying");
    WebhookEvent::set_completed(&pool, "evt-1")
        .await
        .expect("completed");

    let reloaded = WebhookEvent::find_by_event_id(&pool, "evt-1")
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(reloaded.status, EventStatus::Completed);
    assert_eq!(reloaded.tx_digest.as_deref(), Some("0xdigest"));
    assert!(reloaded.is_done());
}

#[sqlx::test]
async fn webhook_event_failed_sets_message(pool: PgPool) {
    WebhookEvent::create(&pool, "evt-2", None, json!({}))
        .await
        .expect("create");
    WebhookEvent::set_failed(&pool, "evt-2", "boom")
        .await
        .expect("fail");

    let reloaded = WebhookEvent::find_by_event_id(&pool, "evt-2")
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(reloaded.status, EventStatus::Failed);
    assert_eq!(reloaded.error_message.as_deref(), Some("boom"));
    assert!(reloaded.is_done());
}

#[sqlx::test]
async fn market_and_bets_round_trip(pool: PgPool) {
    let market = Market::upsert(
        &pool,
        "market-tweet-1",
        "0xmarket1",
        "creator-x",
        "Will it rain tomorrow?",
        100,
        Some("0xtx-market"),
    )
    .await
    .expect("upsert market");
    assert_eq!(market.status, "open");
    assert_eq!(market.question, "Will it rain tomorrow?");

    let found = Market::find_by_market_tweet_id(&pool, "market-tweet-1")
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(found.sui_object_id, "0xmarket1");

    // Place bets on both sides.
    MarketBet::upsert(
        &pool,
        "market-tweet-1",
        "bet-tweet-yes",
        "better-yes",
        true,
        "0x2::sui::SUI",
        1000,
        Some("0xtx-bet1"),
    )
    .await
    .expect("yes bet");
    MarketBet::upsert(
        &pool,
        "market-tweet-1",
        "bet-tweet-no",
        "better-no",
        false,
        "0x2::sui::SUI",
        500,
        Some("0xtx-bet2"),
    )
    .await
    .expect("no bet");

    let coin_types = Market::find_bet_coin_types(&pool, "market-tweet-1")
        .await
        .expect("coin types");
    assert_eq!(coin_types, vec!["0x2::sui::SUI".to_string()]);

    // Resolve YES and check winners.
    Market::set_resolved(&pool, "market-tweet-1", true, Some("resolve-digest-1"))
        .await
        .expect("resolve");
    let resolved = Market::find_by_market_tweet_id(&pool, "market-tweet-1")
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(resolved.status, "resolved");
    assert_eq!(resolved.outcome, Some(true));

    let winners = Market::find_winners(&pool, "market-tweet-1", true)
        .await
        .expect("winners");
    assert_eq!(winners.len(), 1);
    assert_eq!(winners[0].0, "better-yes");
}

#[sqlx::test]
async fn twitter_oauth_token_upsert_lookup_and_rotation(pool: PgPool) {
    let key = [9u8; 32];

    // Store an encrypted refresh token; the DB must never hold plaintext.
    let enc = dugong_core::crypto::seal(&key, "refresh-token-v1").expect("seal");
    TwitterOAuthToken::upsert(&pool, "xid-1", &enc, None, None, Some("offline.access"))
        .await
        .expect("upsert");

    let row = TwitterOAuthToken::find_by_x_user_id(&pool, "xid-1")
        .await
        .expect("query")
        .expect("row exists");
    assert_ne!(row.refresh_token_enc, "refresh-token-v1");
    assert_eq!(row.scope.as_deref(), Some("offline.access"));
    let plaintext = dugong_core::crypto::open(&key, &row.refresh_token_enc).expect("open");
    assert_eq!(plaintext, "refresh-token-v1");

    // Upsert again (simulating refresh-token rotation) replaces in place.
    let enc2 = dugong_core::crypto::seal(&key, "refresh-token-v2").expect("seal");
    TwitterOAuthToken::upsert(&pool, "xid-1", &enc2, None, None, Some("offline.access"))
        .await
        .expect("rotate");
    let rotated = TwitterOAuthToken::find_by_x_user_id(&pool, "xid-1")
        .await
        .expect("query")
        .expect("row exists");
    assert_eq!(
        dugong_core::crypto::open(&key, &rotated.refresh_token_enc).expect("open"),
        "refresh-token-v2"
    );

    // Delete removes the credential so it cannot be retried.
    TwitterOAuthToken::delete(&pool, "xid-1")
        .await
        .expect("delete");
    assert!(TwitterOAuthToken::find_by_x_user_id(&pool, "xid-1")
        .await
        .expect("query")
        .is_none());
}
