//! `#[sqlx::test]` tests for each indexer event handler, asserting the rows
//! they write into Postgres. Migrations live in the core crate.

mod common;

use common::sui_event;
use dugong_core::db::models::{AccountBalance, DugongAccount, Market};
use dugong_indexer::handlers::account_created::AccountCreatedHandler;
use dugong_indexer::handlers::bet_placed::BetPlacedHandler;
use dugong_indexer::handlers::coin_deposited::CoinDepositedHandler;
use dugong_indexer::handlers::coin_transferred::TransferCompletedHandler;
use dugong_indexer::handlers::coin_withdrawn::CoinWithdrawnHandler;
use dugong_indexer::handlers::handle_updated::HandleUpdatedHandler;
use dugong_indexer::handlers::market_created::MarketCreatedHandler;
use dugong_indexer::handlers::market_resolved::MarketResolvedHandler;
use dugong_indexer::handlers::wallet_linked::WalletLinkedHandler;
use dugong_indexer::handlers::EventHandler;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = "../core/migrations")]
async fn account_created_upserts_account(pool: PgPool) {
    let event = sui_event(
        "0x9::events::AccountCreated",
        "DIGEST-acc",
        json!({ "xid": "user-1", "handle": "alice", "account_id": "0xobj1" }),
    );
    AccountCreatedHandler::handle(&pool, &event)
        .await
        .expect("handle");

    let acc = DugongAccount::find_by_x_user_id(&pool, "user-1")
        .await
        .unwrap()
        .expect("account exists");
    assert_eq!(acc.x_handle, "alice");
    assert_eq!(acc.sui_object_id, "0xobj1");
}

#[sqlx::test(migrations = "../core/migrations")]
async fn wallet_linked_sets_owner(pool: PgPool) {
    DugongAccount::create(&pool, "user-2", "bob", "0xobj2")
        .await
        .unwrap();
    let event = sui_event(
        "0x9::events::WalletLinked",
        "DIGEST-wl",
        json!({ "xid": "user-2", "owner_address": "0xowner2" }),
    );
    WalletLinkedHandler::handle(&pool, &event)
        .await
        .expect("handle");

    let acc = DugongAccount::find_by_owner_address(&pool, "0xowner2")
        .await
        .unwrap()
        .expect("found by owner");
    assert_eq!(acc.x_user_id, "user-2");
}

#[sqlx::test(migrations = "../core/migrations")]
async fn handle_updated_changes_handle(pool: PgPool) {
    DugongAccount::create(&pool, "user-3", "old", "0xobj3")
        .await
        .unwrap();
    let event = sui_event(
        "0x9::events::HandleUpdated",
        "DIGEST-hu",
        json!({ "xid": "user-3", "old_handle": "old", "new_handle": "new" }),
    );
    HandleUpdatedHandler::handle(&pool, &event)
        .await
        .expect("handle");

    let acc = DugongAccount::find_by_x_user_id(&pool, "user-3")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(acc.x_handle, "new");
}

#[sqlx::test(migrations = "../core/migrations")]
async fn coin_deposited_increases_balance(pool: PgPool) {
    let event = sui_event(
        "0x9::events::CoinDeposited",
        "DIGEST-dep",
        json!({ "xid": "user-4", "coin_type": "0x2::sui::SUI", "amount": "1000" }),
    );
    CoinDepositedHandler::handle(&pool, &event)
        .await
        .expect("handle");

    let balances = AccountBalance::find_by_x_user_id(&pool, "user-4")
        .await
        .unwrap();
    assert_eq!(balances.len(), 1);
    assert_eq!(balances[0].balance, 1000);
}

#[sqlx::test(migrations = "../core/migrations")]
async fn coin_withdrawn_decreases_balance(pool: PgPool) {
    // Seed a starting balance via a deposit, then withdraw part of it.
    CoinDepositedHandler::handle(
        &pool,
        &sui_event(
            "0x9::events::CoinDeposited",
            "DIGEST-dep2",
            json!({ "xid": "user-5", "coin_type": "0x2::sui::SUI", "amount": "1000" }),
        ),
    )
    .await
    .expect("deposit");

    CoinWithdrawnHandler::handle(
        &pool,
        &sui_event(
            "0x9::events::CoinWithdrawn",
            "DIGEST-wd",
            json!({ "xid": "user-5", "coin_type": "0x2::sui::SUI", "amount": "400" }),
        ),
    )
    .await
    .expect("withdraw");

    let balances = AccountBalance::find_by_x_user_id(&pool, "user-5")
        .await
        .unwrap();
    assert_eq!(balances[0].balance, 600);
}

#[sqlx::test(migrations = "../core/migrations")]
async fn transfer_completed_moves_balance(pool: PgPool) {
    // Give the sender a balance to move.
    CoinDepositedHandler::handle(
        &pool,
        &sui_event(
            "0x9::events::CoinDeposited",
            "DIGEST-seed",
            json!({ "xid": "sender", "coin_type": "0x2::sui::SUI", "amount": "1000" }),
        ),
    )
    .await
    .expect("seed");

    TransferCompletedHandler::handle(
        &pool,
        &sui_event(
            "0x9::events::TransferCompleted",
            "DIGEST-xfer",
            json!({
                "from_xid": "sender",
                "to_xid": "receiver",
                "tweet_id": "tweet-1",
                "coin_type": "0x2::sui::SUI",
                "amount": "300",
                "timestamp": "1700000000000"
            }),
        ),
    )
    .await
    .expect("transfer");

    let sender = AccountBalance::find_by_x_user_id(&pool, "sender")
        .await
        .unwrap();
    let receiver = AccountBalance::find_by_x_user_id(&pool, "receiver")
        .await
        .unwrap();
    assert_eq!(sender[0].balance, 700);
    assert_eq!(receiver[0].balance, 300);
}

#[sqlx::test(migrations = "../core/migrations")]
async fn market_created_and_resolved(pool: PgPool) {
    MarketCreatedHandler::handle(
        &pool,
        &sui_event(
            "0x9::events::MarketCreated",
            "DIGEST-mkt",
            json!({
                "market_tweet_id": "mkt-tweet-1",
                "market_id": "0xmarket1",
                "creator_xid": "creator",
                "question": "Rain tomorrow?",
                "fee_bps": 100
            }),
        ),
    )
    .await
    .expect("market created");

    let market = Market::find_by_market_tweet_id(&pool, "mkt-tweet-1")
        .await
        .unwrap()
        .expect("market exists");
    assert_eq!(market.status, "open");
    assert_eq!(market.sui_object_id, "0xmarket1");

    MarketResolvedHandler::handle(
        &pool,
        &sui_event(
            "0x9::events::MarketResolved",
            "DIGEST-res",
            json!({
                "market_tweet_id": "mkt-tweet-1",
                "resolver_xid": "creator",
                "outcome": true,
                "timestamp": "1700000000000"
            }),
        ),
    )
    .await
    .expect("market resolved");

    let resolved = Market::find_by_market_tweet_id(&pool, "mkt-tweet-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.status, "resolved");
    assert_eq!(resolved.outcome, Some(true));
}

#[sqlx::test(migrations = "../core/migrations")]
async fn bet_placed_records_bet(pool: PgPool) {
    // A bet requires its market to exist first.
    MarketCreatedHandler::handle(
        &pool,
        &sui_event(
            "0x9::events::MarketCreated",
            "DIGEST-mkt2",
            json!({
                "market_tweet_id": "mkt-tweet-2",
                "market_id": "0xmarket2",
                "creator_xid": "creator",
                "question": "Heads?",
                "fee_bps": 100
            }),
        ),
    )
    .await
    .expect("market created");

    BetPlacedHandler::handle(
        &pool,
        &sui_event(
            "0x9::events::BetPlaced",
            "DIGEST-bet",
            json!({
                "market_tweet_id": "mkt-tweet-2",
                "bet_tweet_id": "bet-tweet-1",
                "better_xid": "better-1",
                "side": true,
                "coin_type": "0x2::sui::SUI",
                "amount": "500",
                "timestamp": "1700000000000"
            }),
        ),
    )
    .await
    .expect("bet placed");

    let coin_types = Market::find_bet_coin_types(&pool, "mkt-tweet-2")
        .await
        .unwrap();
    assert_eq!(coin_types, vec!["0x2::sui::SUI".to_string()]);
}

#[sqlx::test(migrations = "../core/migrations")]
async fn missing_parsed_json_is_error(pool: PgPool) {
    let event = sui_event(
        "0x9::events::AccountCreated",
        "DIGEST-bad",
        serde_json::Value::Null,
    );
    // parsed_json deserializes Null into None -> handler should error, not panic.
    let result = AccountCreatedHandler::handle(&pool, &event).await;
    assert!(result.is_err());
}
