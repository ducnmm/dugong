// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// Transfer module for coin transfers between accounts
module dugong::transfers {
    use std::string::{Self};
    use dugong::dug::{Self, DugongAccount};
    use dugong::events;
    use enclave::enclave::Enclave;

    // ====== Coin Transfer Functions ======

    /// Transfer coin through the enclave-backed tweet flow.
    public fun transfer_coin<T, E>(
        from: &mut DugongAccount,
        to: &mut DugongAccount,
        amount: u64,
        coin_type: vector<u8>,
        tweet_id: vector<u8>,
        timestamp: u64,
        _signature: &vector<u8>,
        _enclave: &Enclave<E>,
    ) {
        let tweet_id_str = string::utf8(tweet_id);

        let processed_tweets = dug::account_processed_tweets(from);
        assert!(!processed_tweets.contains(tweet_id_str), dug::e_tweet_already_processed());

        dug::assert_coin_type<T>(coin_type);

        assert!(timestamp > dug::account_last_timestamp(from), dug::e_replay_attempt());
        dug::account_set_last_timestamp(from, timestamp);

        transfer_balance_internal<T>(from, to, amount);

        dug::account_add_processed_tweet(from, tweet_id_str);

        events::emit_transfer_completed(
            dug::account_xid(from),
            dug::account_xid(to),
            tweet_id_str,
            dug::coin_type_string<T>(),
            amount,
            timestamp,
        );
    }

    /// Transfer coin with wallet authentication (owner signs PTB from dApp)
    public fun transfer_coin_with_wallet<T>(
        from: &mut DugongAccount,
        to: &mut DugongAccount,
        amount: u64,
        ctx: &TxContext,
    ) {
        // Check owner
        assert!(dug::account_owner_address(from).is_some(), dug::e_owner_not_set());
        assert!(ctx.sender() == *dug::account_owner_address(from).borrow(), dug::e_not_owner());

        transfer_balance_internal<T>(from, to, amount);

        events::emit_transfer_completed(
            dug::account_xid(from),
            dug::account_xid(to),
            string::utf8(b""),
            dug::coin_type_string<T>(),
            amount,
            0,
        );
    }

    // ====== Internal Helper Functions ======

    fun transfer_balance_internal<T>(
        from: &mut DugongAccount,
        to: &mut DugongAccount,
        amount: u64,
    ) {
        let transfer_balance = dug::account_debit_balance<T>(from, amount);
        dug::account_credit_balance(to, transfer_balance);
    }
}
