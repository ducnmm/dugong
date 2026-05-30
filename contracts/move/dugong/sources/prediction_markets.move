// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// On-chain prediction markets with escrowed Dugong balances.
module dugong::prediction_markets {
    use std::ascii;
    use std::string::{Self, String};
    use std::type_name;
    use sui::bag::{Self, Bag};
    use sui::balance::Balance;
    use sui::table::{Self, Table};
    use dugong::core::{Self, DugongAccount};
    use dugong::events;
    use enclave::enclave::Enclave;

    const STATUS_OPEN: u8 = 0;
    const STATUS_RESOLVED: u8 = 1;

    const CHOICE_NONE: u8 = 0;
    const CHOICE_YES: u8 = 1;
    const CHOICE_NO: u8 = 2;

    const EMarketClosed: u64 = 100;
    const EMarketNotResolved: u64 = 101;
    const EInvalidChoice: u64 = 102;
    const ENotMarketCreator: u64 = 103;
    const EMarketCoinTypeMismatch: u64 = 104;
    const EBetAlreadyProcessed: u64 = 105;
    const ENoBetPosition: u64 = 106;
    const ENoWinningStake: u64 = 107;
    const EPayoutAlreadyClaimed: u64 = 108;

    public struct PredictionMarket has key {
        id: UID,
        market_tweet_id: String,
        creator_xid: String,
        question: String,
        status: u8,
        outcome: u8,
        coin_type: Option<ascii::String>,
        escrow: Bag,
        positions: Table<String, BetPosition>,
        processed_bet_tweets: Table<String, bool>,
        yes_pool: u64,
        no_pool: u64,
        remaining_winning_pool: u64,
        created_at: u64,
        resolved_at: u64,
    }

    public struct BetPosition has store {
        yes_amount: u64,
        no_amount: u64,
        claimed: bool,
    }

    public fun create_market<E>(
        creator: &mut DugongAccount,
        market_tweet_id: vector<u8>,
        question: vector<u8>,
        timestamp: u64,
        _signature: &vector<u8>,
        _enclave: &Enclave<E>,
        ctx: &mut TxContext,
    ) {
        let market_tweet_id_str = string::utf8(market_tweet_id);

        let processed_tweets = core::account_processed_tweets(creator);
        assert!(!processed_tweets.contains(market_tweet_id_str), core::e_tweet_already_processed());

        // let payload = core::new_create_prediction_market_payload(
        //     core::account_xid(creator).into_bytes(),
        //     market_tweet_id,
        //     question,
        // );
        // let is_valid = enclave.verify_signature(
        //     core::create_prediction_market_intent(),
        //     timestamp,
        //     payload,
        //     signature,
        // );
        // assert!(is_valid, core::e_invalid_signature());

        assert!(timestamp > core::account_last_timestamp(creator), core::e_replay_attempt());
        core::account_set_last_timestamp(creator, timestamp);
        core::account_add_processed_tweet(creator, market_tweet_id_str);

        let market = PredictionMarket {
            id: object::new(ctx),
            market_tweet_id: market_tweet_id_str,
            creator_xid: core::account_xid(creator),
            question: string::utf8(question),
            status: STATUS_OPEN,
            outcome: CHOICE_NONE,
            coin_type: option::none(),
            escrow: bag::new(ctx),
            positions: table::new(ctx),
            processed_bet_tweets: table::new(ctx),
            yes_pool: 0,
            no_pool: 0,
            remaining_winning_pool: 0,
            created_at: timestamp,
            resolved_at: 0,
        };

        events::emit_prediction_market_created(
            object::id(&market),
            market.market_tweet_id,
            market.creator_xid,
            market.question,
            timestamp,
        );

        transfer::share_object(market);
    }

    public fun place_bet<T, E>(
        market: &mut PredictionMarket,
        bettor: &mut DugongAccount,
        choice: u8,
        amount: u64,
        coin_type: vector<u8>,
        bet_tweet_id: vector<u8>,
        timestamp: u64,
        _signature: &vector<u8>,
        _enclave: &Enclave<E>,
    ) {
        assert!(market.status == STATUS_OPEN, EMarketClosed);
        assert!(choice == CHOICE_YES || choice == CHOICE_NO, EInvalidChoice);

        let bet_tweet_id_str = string::utf8(bet_tweet_id);
        assert!(
            !market.processed_bet_tweets.contains(bet_tweet_id_str),
            EBetAlreadyProcessed,
        );

        let expected_type = type_name::get<T>().into_string().into_bytes();
        assert!(coin_type == expected_type, core::e_coin_type_mismatch());
        assert_market_coin_type<T>(market);

        // let payload = core::new_place_prediction_bet_payload(
        //     core::account_xid(bettor).into_bytes(),
        //     market.market_tweet_id.into_bytes(),
        //     bet_tweet_id,
        //     choice,
        //     amount,
        //     coin_type,
        // );
        // let is_valid = enclave.verify_signature(
        //     core::place_prediction_bet_intent(),
        //     timestamp,
        //     payload,
        //     signature,
        // );
        // assert!(is_valid, core::e_invalid_signature());

        assert!(timestamp > core::account_last_timestamp(bettor), core::e_replay_attempt());
        core::account_set_last_timestamp(bettor, timestamp);

        transfer_from_account_to_market<T>(bettor, market, amount);
        add_position(market, core::account_xid(bettor), choice, amount);
        market.processed_bet_tweets.add(bet_tweet_id_str, true);

        if (choice == CHOICE_YES) {
            market.yes_pool = market.yes_pool + amount;
        } else {
            market.no_pool = market.no_pool + amount;
        };

        events::emit_prediction_bet_placed(
            object::id(market),
            market.market_tweet_id,
            bet_tweet_id_str,
            core::account_xid(bettor),
            choice_to_string(choice),
            type_name::get<T>().into_string().to_string(),
            amount,
            market.yes_pool,
            market.no_pool,
            timestamp,
        );
    }

    public fun resolve_market<E>(
        market: &mut PredictionMarket,
        creator: &mut DugongAccount,
        outcome: u8,
        solve_tweet_id: vector<u8>,
        timestamp: u64,
        _signature: &vector<u8>,
        _enclave: &Enclave<E>,
    ) {
        assert!(market.status == STATUS_OPEN, EMarketClosed);
        assert!(outcome == CHOICE_YES || outcome == CHOICE_NO, EInvalidChoice);
        assert!(core::account_xid(creator) == market.creator_xid, ENotMarketCreator);

        let solve_tweet_id_str = string::utf8(solve_tweet_id);
        let processed_tweets = core::account_processed_tweets(creator);
        assert!(!processed_tweets.contains(solve_tweet_id_str), core::e_tweet_already_processed());

        // let payload = core::new_resolve_prediction_market_payload(
        //     core::account_xid(creator).into_bytes(),
        //     market.market_tweet_id.into_bytes(),
        //     solve_tweet_id,
        //     outcome,
        // );
        // let is_valid = enclave.verify_signature(
        //     core::resolve_prediction_market_intent(),
        //     timestamp,
        //     payload,
        //     signature,
        // );
        // assert!(is_valid, core::e_invalid_signature());

        assert!(timestamp > core::account_last_timestamp(creator), core::e_replay_attempt());
        core::account_set_last_timestamp(creator, timestamp);
        core::account_add_processed_tweet(creator, solve_tweet_id_str);

        market.status = STATUS_RESOLVED;
        market.outcome = outcome;
        market.remaining_winning_pool = if (outcome == CHOICE_YES) {
            market.yes_pool
        } else {
            market.no_pool
        };
        market.resolved_at = timestamp;

        events::emit_prediction_market_resolved(
            object::id(market),
            market.market_tweet_id,
            solve_tweet_id_str,
            market.creator_xid,
            choice_to_string(outcome),
            timestamp,
        );
    }

    public fun claim_winnings<T>(
        market: &mut PredictionMarket,
        bettor: &mut DugongAccount,
        coin_type: vector<u8>,
        timestamp: u64,
    ) {
        assert!(market.status == STATUS_RESOLVED, EMarketNotResolved);

        let expected_type = type_name::get<T>().into_string().into_bytes();
        assert!(coin_type == expected_type, core::e_coin_type_mismatch());
        assert_market_coin_type<T>(market);

        let bettor_xid = core::account_xid(bettor);
        assert!(market.positions.contains(bettor_xid), ENoBetPosition);
        let position = market.positions.borrow(bettor_xid);
        let yes_amount = position.yes_amount;
        let no_amount = position.no_amount;
        let already_claimed = position.claimed;
        assert!(!already_claimed, EPayoutAlreadyClaimed);

        let winning_stake = if (market.outcome == CHOICE_YES) {
            yes_amount
        } else {
            no_amount
        };

        let winning_pool = if (market.outcome == CHOICE_YES) {
            market.yes_pool
        } else {
            market.no_pool
        };
        let total_pool = market.yes_pool + market.no_pool;
        let payout = if (winning_pool == 0) {
            let refund = yes_amount + no_amount;
            assert!(refund > 0, ENoWinningStake);
            refund
        } else {
            assert!(winning_stake > 0, ENoWinningStake);

            let pro_rata = ((winning_stake as u128) * (total_pool as u128) / (winning_pool as u128) as u64);
            if (winning_stake == market.remaining_winning_pool) {
                escrow_value<T>(market)
            } else {
                pro_rata
            }
        };

        let position = market.positions.borrow_mut(bettor_xid);
        position.claimed = true;
        if (winning_pool > 0) {
            market.remaining_winning_pool = market.remaining_winning_pool - winning_stake;
        };
        transfer_from_market_to_account<T>(market, bettor, payout);

        events::emit_prediction_payout_claimed(
            object::id(market),
            market.market_tweet_id,
            bettor_xid,
            choice_to_string(market.outcome),
            type_name::get<T>().into_string().to_string(),
            payout,
            timestamp,
        );
    }

    public fun is_open(market: &PredictionMarket): bool {
        market.status == STATUS_OPEN
    }

    public fun is_resolved(market: &PredictionMarket): bool {
        market.status == STATUS_RESOLVED
    }

    public fun market_tweet_id(market: &PredictionMarket): String {
        market.market_tweet_id
    }

    public fun creator_xid(market: &PredictionMarket): String {
        market.creator_xid
    }

    public fun yes_pool(market: &PredictionMarket): u64 {
        market.yes_pool
    }

    public fun no_pool(market: &PredictionMarket): u64 {
        market.no_pool
    }

    public fun choice_yes(): u8 {
        CHOICE_YES
    }

    public fun choice_no(): u8 {
        CHOICE_NO
    }

    fun assert_market_coin_type<T>(market: &mut PredictionMarket) {
        let type_key = type_name::get<T>().into_string();

        if (market.coin_type.is_none()) {
            market.coin_type.fill(type_key);
        } else {
            assert!(*market.coin_type.borrow() == type_key, EMarketCoinTypeMismatch);
        };
    }

    fun add_position(
        market: &mut PredictionMarket,
        bettor_xid: String,
        choice: u8,
        amount: u64,
    ) {
        if (market.positions.contains(bettor_xid)) {
            let position = market.positions.borrow_mut(bettor_xid);
            if (choice == CHOICE_YES) {
                position.yes_amount = position.yes_amount + amount;
            } else {
                position.no_amount = position.no_amount + amount;
            };
        } else {
            let position = if (choice == CHOICE_YES) {
                BetPosition {
                    yes_amount: amount,
                    no_amount: 0,
                    claimed: false,
                }
            } else {
                BetPosition {
                    yes_amount: 0,
                    no_amount: amount,
                    claimed: false,
                }
            };
            market.positions.add(bettor_xid, position);
        };
    }

    fun transfer_from_account_to_market<T>(
        account: &mut DugongAccount,
        market: &mut PredictionMarket,
        amount: u64,
    ) {
        let type_key = type_name::get<T>().into_string();
        let account_balances = core::account_balances_mut(account);
        assert!(account_balances.contains(type_key), core::e_insufficient_balance());

        let account_balance = account_balances.borrow_mut<ascii::String, Balance<T>>(type_key);
        assert!(account_balance.value() >= amount, core::e_insufficient_balance());
        let stake = account_balance.split(amount);

        if (market.escrow.contains(type_key)) {
            let escrow_balance = market.escrow.borrow_mut<ascii::String, Balance<T>>(type_key);
            escrow_balance.join(stake);
        } else {
            market.escrow.add(type_key, stake);
        };
    }

    fun transfer_from_market_to_account<T>(
        market: &mut PredictionMarket,
        account: &mut DugongAccount,
        amount: u64,
    ) {
        let type_key = type_name::get<T>().into_string();
        assert!(market.escrow.contains(type_key), core::e_insufficient_balance());

        let escrow_balance = market.escrow.borrow_mut<ascii::String, Balance<T>>(type_key);
        assert!(escrow_balance.value() >= amount, core::e_insufficient_balance());
        let payout = escrow_balance.split(amount);

        let account_balances = core::account_balances_mut(account);
        if (account_balances.contains(type_key)) {
            let account_balance = account_balances.borrow_mut<ascii::String, Balance<T>>(type_key);
            account_balance.join(payout);
        } else {
            account_balances.add(type_key, payout);
        };
    }

    fun escrow_value<T>(market: &PredictionMarket): u64 {
        let type_key = type_name::get<T>().into_string();
        assert!(market.escrow.contains(type_key), core::e_insufficient_balance());
        market.escrow.borrow<ascii::String, Balance<T>>(type_key).value()
    }

    fun choice_to_string(choice: u8): String {
        if (choice == CHOICE_YES) {
            string::utf8(b"yes")
        } else {
            string::utf8(b"no")
        }
    }
}
