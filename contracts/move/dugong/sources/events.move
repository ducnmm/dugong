// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// Events for blockchain indexing
module dugong::events {
    use std::string::String;

    // ====== Account Events ======

    public struct AccountCreated has copy, drop {
        xid: String,
        handle: String,
        account_id: ID,
    }

    public struct WalletLinked has copy, drop {
        xid: String,
        owner_address: address,
    }

    public struct HandleUpdated has copy, drop {
        xid: String,
        old_handle: String,
        new_handle: String,
    }

    // ====== Asset Events ======

    public struct CoinDeposited has copy, drop {
        xid: String,
        coin_type: String,
        amount: u64,
    }

    public struct CoinWithdrawn has copy, drop {
        xid: String,
        coin_type: String,
        amount: u64,
    }

    // ====== Market Events ======

    public struct MarketCreated has copy, drop {
        market_tweet_id: String,
        market_id: ID,
        creator_xid: String,
        question: String,
        fee_bps: u16,
    }

    public struct BetPlaced has copy, drop {
        market_tweet_id: String,
        bet_tweet_id: String,
        better_xid: String,
        side: bool,
        coin_type: String,
        amount: u64,
        timestamp: u64,
    }

    public struct MarketResolved has copy, drop {
        market_tweet_id: String,
        resolver_xid: String,
        outcome: bool,
        timestamp: u64,
    }

    // ====== Transfer Events ======

    public struct TransferCompleted has copy, drop {
        from_xid: String,
        to_xid: String,
        tweet_id: String,
        coin_type: String,
        amount: u64,
        timestamp: u64,
    }

    // ====== Event Emission Functions ======

    public(package) fun emit_account_created(xid: String, handle: String, account_id: ID) {
        sui::event::emit(AccountCreated {
            xid,
            handle,
            account_id,
        });
    }

    public(package) fun emit_wallet_linked(xid: String, owner_address: address) {
        sui::event::emit(WalletLinked {
            xid,
            owner_address,
        });
    }

    public(package) fun emit_handle_updated(xid: String, old_handle: String, new_handle: String) {
        sui::event::emit(HandleUpdated {
            xid,
            old_handle,
            new_handle,
        });
    }

    public(package) fun emit_coin_deposited(xid: String, coin_type: String, amount: u64) {
        sui::event::emit(CoinDeposited {
            xid,
            coin_type,
            amount,
        });
    }

    public(package) fun emit_coin_withdrawn(xid: String, coin_type: String, amount: u64) {
        sui::event::emit(CoinWithdrawn {
            xid,
            coin_type,
            amount,
        });
    }

    public(package) fun emit_market_created(
        market_tweet_id: String,
        market_id: ID,
        creator_xid: String,
        question: String,
        fee_bps: u16,
    ) {
        sui::event::emit(MarketCreated { market_tweet_id, market_id, creator_xid, question, fee_bps });
    }

    public(package) fun emit_bet_placed(
        market_tweet_id: String,
        bet_tweet_id: String,
        better_xid: String,
        side: bool,
        coin_type: String,
        amount: u64,
        timestamp: u64,
    ) {
        sui::event::emit(BetPlaced {
            market_tweet_id,
            bet_tweet_id,
            better_xid,
            side,
            coin_type,
            amount,
            timestamp,
        });
    }

    public(package) fun emit_market_resolved(
        market_tweet_id: String,
        resolver_xid: String,
        outcome: bool,
        timestamp: u64,
    ) {
        sui::event::emit(MarketResolved { market_tweet_id, resolver_xid, outcome, timestamp });
    }

    public(package) fun emit_transfer_completed(
        from_xid: String,
        to_xid: String,
        tweet_id: String,
        coin_type: String,
        amount: u64,
        timestamp: u64,
    ) {
        sui::event::emit(TransferCompleted {
            from_xid,
            to_xid,
            tweet_id,
            coin_type,
            amount,
            timestamp,
        });
    }

}
