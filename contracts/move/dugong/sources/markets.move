// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// Prediction market module: create yes/no markets from tweets, escrow bets,
/// and distribute the parimutuel pool to winners on resolution.
module dugong::markets {
    use std::ascii;
    use std::string::{Self, String};
    use std::type_name;
    use sui::balance::{Self, Balance};
    use sui::bag::{Self, Bag};
    use sui::table::{Self, Table};
    use dugong::core::{Self, DugongAccount};
    use dugong::events;

    // ====== Market Status Constants ======

    const STATUS_OPEN: u8 = 0;
    const STATUS_RESOLVED: u8 = 1;

    // ====== Core Structs ======

    /// Registry mapping market_tweet_id -> market object ID.
    public struct MarketRegistry has key {
        id: UID,
        tweet_id_to_market: Table<String, ID>,
    }

    /// Per-coin escrow pool stored inside a PredictionMarket's Bag.
    public struct CoinPool<phantom T> has store {
        yes_balance: Balance<T>,
        no_balance: Balance<T>,
        /// better_xid -> cumulative staked amount on yes
        yes_stakes: Table<String, u64>,
        /// better_xid -> cumulative staked amount on no
        no_stakes: Table<String, u64>,
        yes_total: u64,
        no_total: u64,
        /// bet_tweet_id -> true (idempotency guard)
        processed_bets: Table<String, bool>,
        /// winner_xid -> true (prevents double payout)
        paid_winners: Table<String, bool>,
        /// Set by resolve_market: distributable amount on winning side after fee
        distributable: u64,
    }

    /// Shared prediction market object.
    public struct PredictionMarket has key {
        id: UID,
        market_tweet_id: String,
        creator_xid: String,
        question: String,
        status: u8,
        outcome: bool, // meaningful only when status == STATUS_RESOLVED
        pools: Bag,    // ascii::String (coin type) -> CoinPool<T>
        fee_bps: u16,
        created_at_ms: u64,
        resolved_at_ms: u64,
    }

    // ====== Module Initializer ======

    public struct MARKETS has drop {}

    fun init(_otw: MARKETS, ctx: &mut TxContext) {
        let registry = MarketRegistry {
            id: object::new(ctx),
            tweet_id_to_market: table::new(ctx),
        };
        transfer::share_object(registry);
    }

    // ====== Public Getters ======

    public fun market_tweet_id(m: &PredictionMarket): String { m.market_tweet_id }
    public fun market_creator_xid(m: &PredictionMarket): String { m.creator_xid }
    public fun market_question(m: &PredictionMarket): String { m.question }
    public fun market_status(m: &PredictionMarket): u8 { m.status }
    public fun market_outcome(m: &PredictionMarket): bool { m.outcome }
    public fun market_fee_bps(m: &PredictionMarket): u16 { m.fee_bps }
    public fun market_id(m: &PredictionMarket): ID { object::id(m) }

    public fun registry_contains(reg: &MarketRegistry, tweet_id: String): bool {
        reg.tweet_id_to_market.contains(tweet_id)
    }
    public fun registry_get_market_id(reg: &MarketRegistry, tweet_id: String): ID {
        *reg.tweet_id_to_market.borrow(tweet_id)
    }

    // ====== Create Market ======

    /// Create a new binary (yes/no) prediction market from a tweet.
    /// The market_tweet_id uniquely identifies the market and is registered
    /// in the MarketRegistry for off-chain lookup.
    public fun create_market(
        registry: &mut MarketRegistry,
        creator_xid: vector<u8>,
        market_tweet_id: vector<u8>,
        question: vector<u8>,
        fee_bps: u16,
        timestamp_ms: u64,
        _signature: &vector<u8>,
        ctx: &mut TxContext,
    ) {
        let market_tweet_id_str = string::utf8(market_tweet_id);
        let creator_xid_str = string::utf8(creator_xid);
        let question_str = string::utf8(question);

        assert!(
            !registry.tweet_id_to_market.contains(market_tweet_id_str),
            core::e_market_tweet_already_used(),
        );

        let market = PredictionMarket {
            id: object::new(ctx),
            market_tweet_id: market_tweet_id_str,
            creator_xid: creator_xid_str,
            question: question_str,
            status: STATUS_OPEN,
            outcome: false,
            pools: bag::new(ctx),
            fee_bps,
            created_at_ms: timestamp_ms,
            resolved_at_ms: 0,
        };

        let market_id = object::id(&market);
        registry.tweet_id_to_market.add(market_tweet_id_str, market_id);

        events::emit_market_created(market_tweet_id_str, market_id, creator_xid_str, question_str, fee_bps);

        transfer::share_object(market);
    }

    // ====== Place Bet ======

    /// Stake `amount` of coin T on `side` (true = yes, false = no).
    /// Debits the better's DugongAccount and escrows in the market pool for T.
    /// Idempotent on bet_tweet_id.
    public fun place_bet<T>(
        market: &mut PredictionMarket,
        better_account: &mut DugongAccount,
        amount: u64,
        side: bool,
        bet_tweet_id: vector<u8>,
        coin_type: vector<u8>,
        timestamp_ms: u64,
        _signature: &vector<u8>,
        ctx: &mut TxContext,
    ) {
        assert!(market.status == STATUS_OPEN, core::e_market_closed());

        // Verify coin_type matches generic T
        let expected_type = type_name::get<T>().into_string().into_bytes();
        assert!(coin_type == expected_type, core::e_coin_type_mismatch());

        let bet_tweet_id_str = string::utf8(bet_tweet_id);
        let type_key = type_name::get<T>().into_string();
        let better_xid = core::account_xid(better_account);

        // Lazily create pool for this coin type
        if (!market.pools.contains(type_key)) {
            market.pools.add(type_key, CoinPool<T> {
                yes_balance: balance::zero<T>(),
                no_balance: balance::zero<T>(),
                yes_stakes: table::new<String, u64>(ctx),
                no_stakes: table::new<String, u64>(ctx),
                yes_total: 0,
                no_total: 0,
                processed_bets: table::new<String, bool>(ctx),
                paid_winners: table::new<String, bool>(ctx),
                distributable: 0,
            });
        };

        let pool = market.pools.borrow_mut<ascii::String, CoinPool<T>>(type_key);
        assert!(!pool.processed_bets.contains(bet_tweet_id_str), core::e_bet_already_processed());

        // Debit from better's balance
        let account_balances = core::account_balances_mut(better_account);
        assert!(account_balances.contains(type_key), core::e_insufficient_balance());
        let from_bal = account_balances.borrow_mut<ascii::String, Balance<T>>(type_key);
        assert!(from_bal.value() >= amount, core::e_insufficient_balance());
        let staked = from_bal.split(amount);

        if (side) {
            pool.yes_balance.join(staked);
            pool.yes_total = pool.yes_total + amount;
            if (pool.yes_stakes.contains(better_xid)) {
                let prev = pool.yes_stakes.borrow_mut(better_xid);
                *prev = *prev + amount;
            } else {
                pool.yes_stakes.add(better_xid, amount);
            };
        } else {
            pool.no_balance.join(staked);
            pool.no_total = pool.no_total + amount;
            if (pool.no_stakes.contains(better_xid)) {
                let prev = pool.no_stakes.borrow_mut(better_xid);
                *prev = *prev + amount;
            } else {
                pool.no_stakes.add(better_xid, amount);
            };
        };

        pool.processed_bets.add(bet_tweet_id_str, true);

        events::emit_bet_placed(
            market.market_tweet_id,
            bet_tweet_id_str,
            better_xid,
            side,
            type_name::get<T>().into_string().to_string(),
            amount,
            timestamp_ms,
        );
    }

    // ====== Resolve Market ======

    /// Mark the market as resolved with `outcome`, skim protocol fee to treasury.
    /// After this call, invoke `pay_winner<T>` once per winning better.
    ///
    /// Authorization: resolver_xid must equal creator_xid (enforced on-chain).
    ///
    /// Edge cases (per spec):
    ///   W == 0: no winners; distributable = 0 (pay_winner issues full refunds to all stakers).
    ///   L == 0: winners get their stake back; no fee charged.
    public fun resolve_market<T>(
        market: &mut PredictionMarket,
        treasury: &mut DugongAccount,
        resolver_xid: vector<u8>,
        outcome: bool,
        timestamp_ms: u64,
        _signature: &vector<u8>,
    ) {
        assert!(market.status == STATUS_OPEN, core::e_market_already_resolved());
        assert!(
            string::utf8(resolver_xid) == market.creator_xid,
            core::e_not_market_creator(),
        );

        market.status = STATUS_RESOLVED;
        market.outcome = outcome;
        market.resolved_at_ms = timestamp_ms;

        let type_key = type_name::get<T>().into_string();
        if (!market.pools.contains(type_key)) {
            // No bets in this coin; nothing to settle.
            events::emit_market_resolved(
                market.market_tweet_id,
                string::utf8(resolver_xid),
                outcome,
                timestamp_ms,
            );
            return
        };

        let pool = market.pools.borrow_mut<ascii::String, CoinPool<T>>(type_key);
        let (winning_total, losing_total) = if (outcome) {
            (pool.yes_total, pool.no_total)
        } else {
            (pool.no_total, pool.yes_total)
        };

        // W == 0: no winners → full refund mode; pay_winner will refund both sides
        // L == 0: no losers → return stakes, no fee
        // Normal: skim fee, merge losing pool into winning pool for distribution
        if (winning_total > 0 && losing_total > 0) {
            let grand_total = winning_total + losing_total;
            let fee_bps = (market.fee_bps as u64);
            let fee = grand_total * fee_bps / 10_000;

            // Split fee from losing pool and credit to treasury
            if (fee > 0) {
                let fee_balance = if (outcome) {
                    pool.no_balance.split(fee)
                } else {
                    pool.yes_balance.split(fee)
                };
                let treasury_balances = core::account_balances_mut(treasury);
                if (treasury_balances.contains(type_key)) {
                    treasury_balances
                        .borrow_mut<ascii::String, Balance<T>>(type_key)
                        .join(fee_balance);
                } else {
                    treasury_balances.add(type_key, fee_balance);
                };
            };

            // Merge remaining losing pool into winning pool
            let losing_remainder = if (outcome) {
                let v = pool.no_balance.value();
                pool.no_balance.split(v)
            } else {
                let v = pool.yes_balance.value();
                pool.yes_balance.split(v)
            };
            if (outcome) {
                pool.yes_balance.join(losing_remainder);
            } else {
                pool.no_balance.join(losing_remainder);
            };

            pool.distributable = if (outcome) {
                pool.yes_balance.value()
            } else {
                pool.no_balance.value()
            };
        } else {
            // W==0 or L==0: no fee; distributable tracks the winning-side balance as-is
            pool.distributable = if (outcome) {
                pool.yes_balance.value()
            } else {
                pool.no_balance.value()
            };
        };

        events::emit_market_resolved(
            market.market_tweet_id,
            string::utf8(resolver_xid),
            outcome,
            timestamp_ms,
        );
    }

    /// Pay a winning better their proportional share after `resolve_market<T>`.
    /// For W == 0 (refund mode), refunds any staker on either side.
    /// For L == 0, returns each winner's original stake.
    /// For the normal case, distributes parimutuel share.
    ///
    /// Idempotent per winner XID.
    public fun pay_winner<T>(
        market: &mut PredictionMarket,
        winner_account: &mut DugongAccount,
    ) {
        assert!(market.status == STATUS_RESOLVED, core::e_market_closed());

        let type_key = type_name::get<T>().into_string();
        assert!(market.pools.contains(type_key), core::e_market_not_found());

        let pool = market.pools.borrow_mut<ascii::String, CoinPool<T>>(type_key);
        let winner_xid = core::account_xid(winner_account);

        // Idempotency
        if (pool.paid_winners.contains(winner_xid)) { return };

        let outcome = market.outcome;
        let yes_total = pool.yes_total;
        let no_total = pool.no_total;
        let (winning_total, _losing_total) = if (outcome) {
            (yes_total, no_total)
        } else {
            (no_total, yes_total)
        };

        // Determine stake and payout amount
        let (stake, payout, from_winning_side) = if (winning_total == 0) {
            // W == 0: refund mode — return stake to whoever bet (either side)
            let yes_stake = if (pool.yes_stakes.contains(winner_xid)) {
                *pool.yes_stakes.borrow(winner_xid)
            } else { 0 };
            let no_stake = if (pool.no_stakes.contains(winner_xid)) {
                *pool.no_stakes.borrow(winner_xid)
            } else { 0 };
            let total_stake = yes_stake + no_stake;
            if (total_stake == 0) {
                pool.paid_winners.add(winner_xid, true);
                return
            };
            // Refund from respective pools directly
            (total_stake, total_stake, false) // handled specially below
        } else {
            // Normal or L==0: only winners on winning side get paid
            let winning_stakes = if (outcome) { &pool.yes_stakes } else { &pool.no_stakes };
            if (!winning_stakes.contains(winner_xid)) {
                pool.paid_winners.add(winner_xid, true);
                return
            };
            let stake = *winning_stakes.borrow(winner_xid);
            // Parimutuel: floor(distributable * stake / winning_total)
            let payout = pool.distributable * stake / winning_total;
            (stake, payout, true)
        };

        let _ = stake; // suppress unused warning

        // Transfer payout from market pool to winner's account
        let payout_balance = if (winning_total == 0) {
            // Refund mode: return from the pool(s) the winner staked into
            let yes_stake = if (pool.yes_stakes.contains(winner_xid)) {
                *pool.yes_stakes.borrow(winner_xid)
            } else { 0 };
            if (yes_stake > 0 && pool.yes_balance.value() >= yes_stake) {
                let no_stake_refund = if (pool.no_stakes.contains(winner_xid)) {
                    let ns = *pool.no_stakes.borrow(winner_xid);
                    if (pool.no_balance.value() >= ns) { pool.no_balance.split(ns) }
                    else { balance::zero<T>() }
                } else {
                    balance::zero<T>()
                };
                let mut yes_refund = pool.yes_balance.split(yes_stake);
                yes_refund.join(no_stake_refund);
                yes_refund
            } else {
                let ns = if (pool.no_stakes.contains(winner_xid)) {
                    *pool.no_stakes.borrow(winner_xid)
                } else { 0 };
                if (ns > 0 && pool.no_balance.value() >= ns) {
                    pool.no_balance.split(ns)
                } else {
                    balance::zero<T>()
                }
            }
        } else if (from_winning_side) {
            if (outcome) {
                pool.yes_balance.split(payout)
            } else {
                pool.no_balance.split(payout)
            }
        } else {
            balance::zero<T>()
        };

        if (payout_balance.value() > 0) {
            let winner_balances = core::account_balances_mut(winner_account);
            if (winner_balances.contains(type_key)) {
                winner_balances
                    .borrow_mut<ascii::String, Balance<T>>(type_key)
                    .join(payout_balance);
            } else {
                winner_balances.add(type_key, payout_balance);
            };
        } else {
            payout_balance.destroy_zero();
        };

        // Update distributable after each payout (tracks remaining for dust)
        if (winning_total > 0) {
            pool.distributable = if (outcome) {
                pool.yes_balance.value()
            } else {
                pool.no_balance.value()
            };
        };

        pool.paid_winners.add(winner_xid, true);
    }

    // ====== Test-Only Helpers ======

    #[test_only]
    public fun init_for_testing(ctx: &mut TxContext) {
        init(MARKETS {}, ctx);
    }

    #[test_only]
    public fun pool_yes_total<T>(market: &PredictionMarket): u64 {
        let type_key = type_name::get<T>().into_string();
        if (!market.pools.contains(type_key)) { return 0 };
        market.pools.borrow<ascii::String, CoinPool<T>>(type_key).yes_total
    }

    #[test_only]
    public fun pool_no_total<T>(market: &PredictionMarket): u64 {
        let type_key = type_name::get<T>().into_string();
        if (!market.pools.contains(type_key)) { return 0 };
        market.pools.borrow<ascii::String, CoinPool<T>>(type_key).no_total
    }

    #[test_only]
    public fun pool_distributable<T>(market: &PredictionMarket): u64 {
        let type_key = type_name::get<T>().into_string();
        if (!market.pools.contains(type_key)) { return 0 };
        market.pools.borrow<ascii::String, CoinPool<T>>(type_key).distributable
    }
}
