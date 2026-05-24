// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

#[test_only]
module dugong::markets_tests {
    use sui::test_scenario::{Self as ts, Scenario};
    use sui::coin;
    use sui::sui::SUI;
    use dugong::core::{Self, DugongRegistry, DugongAccount};
    use dugong::markets::{Self, PredictionMarket, MarketRegistry};
    use dugong::account;

    // Test-only helpers
    fun creator(): address { @0xCAFE }
    fun better_a(): address { @0xA }
    fun better_b(): address { @0xB }
    fun treasury_addr(): address { @0xFEE }

    // Create a shared DugongAccount for testing with initial SUI balance
    fun setup_account(scenario: &mut Scenario, owner: address, xid: vector<u8>, handle: vector<u8>) {
        ts::next_tx(scenario, owner);
        {
            let mut registry = ts::take_shared<DugongRegistry>(scenario);
            account::init_account_no_signature(&mut registry, xid, handle, ts::ctx(scenario));
            ts::return_shared(registry);
        };
    }

    // Deposit SUI into an account (using test mint)
    fun deposit_sui(scenario: &mut Scenario, account_xid: vector<u8>, amount: u64) {
        ts::next_tx(scenario, creator());
        {
            let registry = ts::take_shared<DugongRegistry>(scenario);
            let account_id = core::registry_get_account_id(&registry, std::string::utf8(account_xid));
            let mut account = ts::take_shared_by_id<DugongAccount>(scenario, account_id);
            let coin = coin::mint_for_testing<SUI>(amount, ts::ctx(scenario));
            dugong::assets::deposit_coin<SUI>(&mut account, coin, ts::ctx(scenario));
            ts::return_shared(account);
            ts::return_shared(registry);
        };
    }

    // ====== Test: Create market ======

    #[test]
    fun test_create_market() {
        let mut scenario = ts::begin(creator());

        // Init core registry and markets registry
        ts::next_tx(&mut scenario, creator());
        { core::init_for_testing(ts::ctx(&mut scenario)); };

        ts::next_tx(&mut scenario, creator());
        { markets::init_for_testing(ts::ctx(&mut scenario)); };

        ts::next_tx(&mut scenario, creator());
        {
            let mut registry = ts::take_shared<MarketRegistry>(&scenario);
            markets::create_market(
                &mut registry,
                b"creator_xid",
                b"tweet_001",
                b"Will BTC hit 100K before March?",
                100, // 1% fee
                1000,
                &b"sig",
                ts::ctx(&mut scenario),
            );
            assert!(markets::registry_contains(&registry, std::string::utf8(b"tweet_001")));
            ts::return_shared(registry);
        };

        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 16)] // EMarketTweetAlreadyUsed
    fun test_create_market_duplicate_rejected() {
        let mut scenario = ts::begin(creator());

        ts::next_tx(&mut scenario, creator());
        { core::init_for_testing(ts::ctx(&mut scenario)); };

        ts::next_tx(&mut scenario, creator());
        { markets::init_for_testing(ts::ctx(&mut scenario)); };

        ts::next_tx(&mut scenario, creator());
        {
            let mut registry = ts::take_shared<MarketRegistry>(&scenario);
            markets::create_market(
                &mut registry, b"creator_xid", b"tweet_dup",
                b"Q?", 100, 1000, &b"sig", ts::ctx(&mut scenario),
            );
            // Second call with same tweet ID should abort
            markets::create_market(
                &mut registry, b"creator_xid", b"tweet_dup",
                b"Q?", 100, 1001, &b"sig", ts::ctx(&mut scenario),
            );
            ts::return_shared(registry);
        };

        ts::end(scenario);
    }

    // ====== Test: Place bet ======

    #[test]
    fun test_place_bet_debits_account() {
        let mut scenario = ts::begin(creator());

        ts::next_tx(&mut scenario, creator());
        { core::init_for_testing(ts::ctx(&mut scenario)); };
        ts::next_tx(&mut scenario, creator());
        { markets::init_for_testing(ts::ctx(&mut scenario)); };

        // Create accounts
        setup_account(&mut scenario, better_a(), b"xid_a", b"alice");
        deposit_sui(&mut scenario, b"xid_a", 10_000_000_000); // 10 SUI

        // Create market
        ts::next_tx(&mut scenario, creator());
        {
            let mut market_reg = ts::take_shared<MarketRegistry>(&scenario);
            markets::create_market(
                &mut market_reg, b"creator_xid", b"tweet_m1",
                b"Q?", 0, 1000, &b"sig", ts::ctx(&mut scenario),
            );
            ts::return_shared(market_reg);
        };

        // Place bet
        ts::next_tx(&mut scenario, better_a());
        {
            let market_reg = ts::take_shared<MarketRegistry>(&scenario);
            let market_id = markets::registry_get_market_id(&market_reg, std::string::utf8(b"tweet_m1"));
            let mut market = ts::take_shared_by_id<PredictionMarket>(&scenario, market_id);

            let acct_reg = ts::take_shared<DugongRegistry>(&scenario);
            let acct_id = core::registry_get_account_id(&acct_reg, std::string::utf8(b"xid_a"));
            let mut better = ts::take_shared_by_id<DugongAccount>(&scenario, acct_id);

            let sui_type = std::type_name::get<SUI>().into_string().into_bytes();
            markets::place_bet<SUI>(
                &mut market, &mut better,
                5_000_000_000, // 5 SUI
                true,          // yes side
                b"bet_tweet_1",
                sui_type,
                2000, &b"sig", ts::ctx(&mut scenario),
            );

            assert!(markets::pool_yes_total<SUI>(&market) == 5_000_000_000);
            assert!(markets::pool_no_total<SUI>(&market) == 0);

            ts::return_shared(market);
            ts::return_shared(acct_reg);
            ts::return_shared(better);
            ts::return_shared(market_reg);
        };

        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 14)] // EBetAlreadyProcessed
    fun test_bet_idempotency() {
        let mut scenario = ts::begin(creator());
        ts::next_tx(&mut scenario, creator());
        { core::init_for_testing(ts::ctx(&mut scenario)); };
        ts::next_tx(&mut scenario, creator());
        { markets::init_for_testing(ts::ctx(&mut scenario)); };

        setup_account(&mut scenario, better_a(), b"xid_a2", b"alice2");
        deposit_sui(&mut scenario, b"xid_a2", 20_000_000_000);

        ts::next_tx(&mut scenario, creator());
        {
            let mut market_reg = ts::take_shared<MarketRegistry>(&scenario);
            markets::create_market(
                &mut market_reg, b"creator_xid", b"tweet_idem",
                b"Q?", 0, 1000, &b"sig", ts::ctx(&mut scenario),
            );
            ts::return_shared(market_reg);
        };

        ts::next_tx(&mut scenario, better_a());
        {
            let market_reg = ts::take_shared<MarketRegistry>(&scenario);
            let market_id = markets::registry_get_market_id(&market_reg, std::string::utf8(b"tweet_idem"));
            let mut market = ts::take_shared_by_id<PredictionMarket>(&scenario, market_id);
            let acct_reg = ts::take_shared<DugongRegistry>(&scenario);
            let acct_id = core::registry_get_account_id(&acct_reg, std::string::utf8(b"xid_a2"));
            let mut better = ts::take_shared_by_id<DugongAccount>(&scenario, acct_id);
            let sui_type = std::type_name::get<SUI>().into_string().into_bytes();

            markets::place_bet<SUI>(&mut market, &mut better, 1_000_000_000, true, b"dup_bet", sui_type, 2000, &b"sig", ts::ctx(&mut scenario));
            // Same bet_tweet_id should abort
            markets::place_bet<SUI>(&mut market, &mut better, 1_000_000_000, true, b"dup_bet", sui_type, 2001, &b"sig", ts::ctx(&mut scenario));

            ts::return_shared(market);
            ts::return_shared(acct_reg);
            ts::return_shared(better);
            ts::return_shared(market_reg);
        };

        ts::end(scenario);
    }

    // ====== Test: Resolve + parimutuel payout ======

    #[test]
    fun test_resolve_parimutuel_payout() {
        let mut scenario = ts::begin(creator());
        ts::next_tx(&mut scenario, creator());
        { core::init_for_testing(ts::ctx(&mut scenario)); };
        ts::next_tx(&mut scenario, creator());
        { markets::init_for_testing(ts::ctx(&mut scenario)); };

        setup_account(&mut scenario, better_a(), b"xid_pa", b"alice_p");
        setup_account(&mut scenario, better_b(), b"xid_pb", b"bob_p");
        setup_account(&mut scenario, treasury_addr(), b"xid_treasury", b"treasury");
        deposit_sui(&mut scenario, b"xid_pa", 10_000_000_000); // 10 SUI yes
        deposit_sui(&mut scenario, b"xid_pb", 10_000_000_000); // 10 SUI no

        ts::next_tx(&mut scenario, creator());
        {
            let mut market_reg = ts::take_shared<MarketRegistry>(&scenario);
            markets::create_market(
                &mut market_reg, b"creator_xid", b"tweet_resolve",
                b"Q?", 1000, // 10% fee for easy verification
                1000, &b"sig", ts::ctx(&mut scenario),
            );
            ts::return_shared(market_reg);
        };

        // Alice bets 6 SUI yes, Bob bets 4 SUI no
        ts::next_tx(&mut scenario, better_a());
        {
            let market_reg = ts::take_shared<MarketRegistry>(&scenario);
            let market_id = markets::registry_get_market_id(&market_reg, std::string::utf8(b"tweet_resolve"));
            let mut market = ts::take_shared_by_id<PredictionMarket>(&scenario, market_id);
            let acct_reg = ts::take_shared<DugongRegistry>(&scenario);
            let acct_id = core::registry_get_account_id(&acct_reg, std::string::utf8(b"xid_pa"));
            let mut better = ts::take_shared_by_id<DugongAccount>(&scenario, acct_id);
            let sui_type = std::type_name::get<SUI>().into_string().into_bytes();
            markets::place_bet<SUI>(&mut market, &mut better, 6_000_000_000, true, b"b1", sui_type, 2000, &b"sig", ts::ctx(&mut scenario));
            ts::return_shared(market);
            ts::return_shared(acct_reg);
            ts::return_shared(better);
            ts::return_shared(market_reg);
        };

        ts::next_tx(&mut scenario, better_b());
        {
            let market_reg = ts::take_shared<MarketRegistry>(&scenario);
            let market_id = markets::registry_get_market_id(&market_reg, std::string::utf8(b"tweet_resolve"));
            let mut market = ts::take_shared_by_id<PredictionMarket>(&scenario, market_id);
            let acct_reg = ts::take_shared<DugongRegistry>(&scenario);
            let acct_id = core::registry_get_account_id(&acct_reg, std::string::utf8(b"xid_pb"));
            let mut better = ts::take_shared_by_id<DugongAccount>(&scenario, acct_id);
            let sui_type = std::type_name::get<SUI>().into_string().into_bytes();
            markets::place_bet<SUI>(&mut market, &mut better, 4_000_000_000, false, b"b2", sui_type, 3000, &b"sig", ts::ctx(&mut scenario));
            ts::return_shared(market);
            ts::return_shared(acct_reg);
            ts::return_shared(better);
            ts::return_shared(market_reg);
        };

        // Resolve: yes wins. grand=10, fee=10%=1, distributable=9, alice's payout=9 (sole yes winner)
        ts::next_tx(&mut scenario, creator());
        {
            let market_reg = ts::take_shared<MarketRegistry>(&scenario);
            let market_id = markets::registry_get_market_id(&market_reg, std::string::utf8(b"tweet_resolve"));
            let mut market = ts::take_shared_by_id<PredictionMarket>(&scenario, market_id);
            let acct_reg = ts::take_shared<DugongRegistry>(&scenario);
            let treasury_id = core::registry_get_account_id(&acct_reg, std::string::utf8(b"xid_treasury"));
            let mut treasury_acct = ts::take_shared_by_id<DugongAccount>(&scenario, treasury_id);

            markets::resolve_market<SUI>(
                &mut market, &mut treasury_acct,
                b"creator_xid", true, 4000, &b"sig",
            );

            // distributable = 9 SUI (10 - 1 fee)
            assert!(markets::pool_distributable<SUI>(&market) == 9_000_000_000);

            ts::return_shared(market);
            ts::return_shared(acct_reg);
            ts::return_shared(treasury_acct);
            ts::return_shared(market_reg);
        };

        // Pay winner Alice
        ts::next_tx(&mut scenario, better_a());
        {
            let market_reg = ts::take_shared<MarketRegistry>(&scenario);
            let market_id = markets::registry_get_market_id(&market_reg, std::string::utf8(b"tweet_resolve"));
            let mut market = ts::take_shared_by_id<PredictionMarket>(&scenario, market_id);
            let acct_reg = ts::take_shared<DugongRegistry>(&scenario);
            let acct_id = core::registry_get_account_id(&acct_reg, std::string::utf8(b"xid_pa"));
            let mut winner = ts::take_shared_by_id<DugongAccount>(&scenario, acct_id);

            markets::pay_winner<SUI>(&mut market, &mut winner);

            // Alice should have received 9 SUI (was 4 SUI after staking 6, now 4+9=13)
            let balance = dugong::assets::get_balance<SUI>(&winner);
            assert!(balance == 13_000_000_000, 0);

            ts::return_shared(market);
            ts::return_shared(acct_reg);
            ts::return_shared(winner);
            ts::return_shared(market_reg);
        };

        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 13)] // ENotMarketCreator
    fun test_unauthorized_resolve() {
        let mut scenario = ts::begin(creator());
        ts::next_tx(&mut scenario, creator());
        { core::init_for_testing(ts::ctx(&mut scenario)); };
        ts::next_tx(&mut scenario, creator());
        { markets::init_for_testing(ts::ctx(&mut scenario)); };
        setup_account(&mut scenario, treasury_addr(), b"xid_t2", b"t2");

        ts::next_tx(&mut scenario, creator());
        {
            let mut market_reg = ts::take_shared<MarketRegistry>(&scenario);
            markets::create_market(
                &mut market_reg, b"creator_xid", b"tweet_unauth",
                b"Q?", 0, 1000, &b"sig", ts::ctx(&mut scenario),
            );
            ts::return_shared(market_reg);
        };

        ts::next_tx(&mut scenario, better_a());
        {
            let market_reg = ts::take_shared<MarketRegistry>(&scenario);
            let market_id = markets::registry_get_market_id(&market_reg, std::string::utf8(b"tweet_unauth"));
            let mut market = ts::take_shared_by_id<PredictionMarket>(&scenario, market_id);
            let acct_reg = ts::take_shared<DugongRegistry>(&scenario);
            let treasury_id = core::registry_get_account_id(&acct_reg, std::string::utf8(b"xid_t2"));
            let mut treasury_acct = ts::take_shared_by_id<DugongAccount>(&scenario, treasury_id);
            // Wrong resolver_xid → should abort
            markets::resolve_market<SUI>(&mut market, &mut treasury_acct, b"wrong_xid", true, 4000, &b"sig");
            ts::return_shared(market);
            ts::return_shared(acct_reg);
            ts::return_shared(treasury_acct);
            ts::return_shared(market_reg);
        };
        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 12)] // EMarketAlreadyResolved
    fun test_double_resolve_rejected() {
        let mut scenario = ts::begin(creator());
        ts::next_tx(&mut scenario, creator());
        { core::init_for_testing(ts::ctx(&mut scenario)); };
        ts::next_tx(&mut scenario, creator());
        { markets::init_for_testing(ts::ctx(&mut scenario)); };
        setup_account(&mut scenario, treasury_addr(), b"xid_t3", b"t3");

        ts::next_tx(&mut scenario, creator());
        {
            let mut market_reg = ts::take_shared<MarketRegistry>(&scenario);
            markets::create_market(
                &mut market_reg, b"creator_xid", b"tweet_dbl",
                b"Q?", 0, 1000, &b"sig", ts::ctx(&mut scenario),
            );
            ts::return_shared(market_reg);
        };

        ts::next_tx(&mut scenario, creator());
        {
            let market_reg = ts::take_shared<MarketRegistry>(&scenario);
            let market_id = markets::registry_get_market_id(&market_reg, std::string::utf8(b"tweet_dbl"));
            let mut market = ts::take_shared_by_id<PredictionMarket>(&scenario, market_id);
            let acct_reg = ts::take_shared<DugongRegistry>(&scenario);
            let treasury_id = core::registry_get_account_id(&acct_reg, std::string::utf8(b"xid_t3"));
            let mut treasury_acct = ts::take_shared_by_id<DugongAccount>(&scenario, treasury_id);
            markets::resolve_market<SUI>(&mut market, &mut treasury_acct, b"creator_xid", true, 4000, &b"sig");
            // Second resolve should abort
            markets::resolve_market<SUI>(&mut market, &mut treasury_acct, b"creator_xid", false, 4001, &b"sig");
            ts::return_shared(market);
            ts::return_shared(acct_reg);
            ts::return_shared(treasury_acct);
            ts::return_shared(market_reg);
        };
        ts::end(scenario);
    }
}
