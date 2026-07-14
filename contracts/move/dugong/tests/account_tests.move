// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

#[test_only]
module dugong::account_tests {
    use sui::clock;
    use sui::test_scenario::{Self as ts};
    use dugong::account;
    use dugong::assets;
    use dugong::dug::{Self, DugongAccount, DugongRegistry};

    fun creator(): address { @0xCAFE }
    fun stranger(): address { @0xBEEF }

    /// Initialize the module, create an account for `xid`, and link `owner` as
    /// its wallet — the shared state every faucet test starts from.
    fun setup_owned_account(scenario: &mut ts::Scenario, xid: vector<u8>, owner: address) {
        ts::next_tx(scenario, owner);
        { dug::init_for_testing(ts::ctx(scenario)); };

        ts::next_tx(scenario, owner);
        {
            let mut registry = ts::take_shared<DugongRegistry>(scenario);
            account::init_account_no_signature(&mut registry, xid, b"handle", ts::ctx(scenario));
            ts::return_shared(registry);
        };

        ts::next_tx(scenario, owner);
        {
            let registry = ts::take_shared<DugongRegistry>(scenario);
            let account_id = dug::registry_get_account_id(&registry, std::string::utf8(xid));
            let mut account = ts::take_shared_by_id<DugongAccount>(scenario, account_id);
            dug::account_set_owner_address(&mut account, owner);
            ts::return_shared(account);
            ts::return_shared(registry);
        };
    }

    #[test]
    fun test_account_creation_grants_starter_dug() {
        let mut scenario = ts::begin(creator());

        ts::next_tx(&mut scenario, creator());
        { dug::init_for_testing(ts::ctx(&mut scenario)); };

        ts::next_tx(&mut scenario, creator());
        {
            let mut registry = ts::take_shared<DugongRegistry>(&scenario);
            account::init_account_no_signature(
                &mut registry,
                b"xid_starter",
                b"starter",
                ts::ctx(&mut scenario),
            );
            ts::return_shared(registry);
        };

        ts::next_tx(&mut scenario, creator());
        {
            let registry = ts::take_shared<DugongRegistry>(&scenario);
            let account_id = dug::registry_get_account_id(
                &registry,
                std::string::utf8(b"xid_starter"),
            );
            let account = ts::take_shared_by_id<DugongAccount>(&scenario, account_id);

            assert!(
                assets::get_balance<dug::DUG>(&account) == dug::starter_dug_balance(),
            );

            ts::return_shared(account);
            ts::return_shared(registry);
        };

        ts::end(scenario);
    }

    #[test]
    fun test_faucet_grants_dug() {
        let mut scenario = ts::begin(creator());
        setup_owned_account(&mut scenario, b"xid_faucet", creator());

        ts::next_tx(&mut scenario, creator());
        {
            let mut registry = ts::take_shared<DugongRegistry>(&scenario);
            let account_id = dug::registry_get_account_id(&registry, std::string::utf8(b"xid_faucet"));
            let mut account = ts::take_shared_by_id<DugongAccount>(&scenario, account_id);

            let mut clock = clock::create_for_testing(ts::ctx(&mut scenario));
            clock::set_for_testing(&mut clock, 1_000);

            assets::faucet(&mut registry, &mut account, &clock, ts::ctx(&mut scenario));

            // Balance = starter grant (on creation) + one faucet claim.
            assert!(
                assets::get_balance<dug::DUG>(&account)
                    == dug::starter_dug_balance() + dug::faucet_dug_amount(),
            );

            clock::destroy_for_testing(clock);
            ts::return_shared(account);
            ts::return_shared(registry);
        };

        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 6, location = dugong::assets)] // EFaucetCooldown
    fun test_faucet_cooldown_enforced() {
        let mut scenario = ts::begin(creator());
        setup_owned_account(&mut scenario, b"xid_cooldown", creator());

        ts::next_tx(&mut scenario, creator());
        {
            let mut registry = ts::take_shared<DugongRegistry>(&scenario);
            let account_id = dug::registry_get_account_id(&registry, std::string::utf8(b"xid_cooldown"));
            let mut account = ts::take_shared_by_id<DugongAccount>(&scenario, account_id);

            let mut clock = clock::create_for_testing(ts::ctx(&mut scenario));
            clock::set_for_testing(&mut clock, 100_000);

            // First claim records the timestamp; a second claim inside the same
            // cooldown window aborts.
            assets::faucet(&mut registry, &mut account, &clock, ts::ctx(&mut scenario));
            assets::faucet(&mut registry, &mut account, &clock, ts::ctx(&mut scenario));

            clock::destroy_for_testing(clock);
            ts::return_shared(account);
            ts::return_shared(registry);
        };

        ts::end(scenario);
    }

    #[test]
    fun test_faucet_succeeds_after_cooldown() {
        let mut scenario = ts::begin(creator());
        setup_owned_account(&mut scenario, b"xid_after", creator());

        ts::next_tx(&mut scenario, creator());
        {
            let mut registry = ts::take_shared<DugongRegistry>(&scenario);
            let account_id = dug::registry_get_account_id(&registry, std::string::utf8(b"xid_after"));
            let mut account = ts::take_shared_by_id<DugongAccount>(&scenario, account_id);

            let mut clock = clock::create_for_testing(ts::ctx(&mut scenario));
            clock::set_for_testing(&mut clock, 100_000);
            assets::faucet(&mut registry, &mut account, &clock, ts::ctx(&mut scenario));

            // Advance past the cooldown window, then claim again.
            clock::set_for_testing(&mut clock, 100_000 + dug::faucet_cooldown_ms());
            assets::faucet(&mut registry, &mut account, &clock, ts::ctx(&mut scenario));

            assert!(
                assets::get_balance<dug::DUG>(&account)
                    == dug::starter_dug_balance() + 2 * dug::faucet_dug_amount(),
            );

            clock::destroy_for_testing(clock);
            ts::return_shared(account);
            ts::return_shared(registry);
        };

        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 1, location = dugong::assets)] // ENotOwner
    fun test_faucet_rejects_non_owner() {
        let mut scenario = ts::begin(creator());
        setup_owned_account(&mut scenario, b"xid_nonowner", creator());

        // A sender other than the linked owner attempts to claim.
        ts::next_tx(&mut scenario, stranger());
        {
            let mut registry = ts::take_shared<DugongRegistry>(&scenario);
            let account_id = dug::registry_get_account_id(&registry, std::string::utf8(b"xid_nonowner"));
            let mut account = ts::take_shared_by_id<DugongAccount>(&scenario, account_id);

            let mut clock = clock::create_for_testing(ts::ctx(&mut scenario));
            clock::set_for_testing(&mut clock, 100_000);

            assets::faucet(&mut registry, &mut account, &clock, ts::ctx(&mut scenario));

            clock::destroy_for_testing(clock);
            ts::return_shared(account);
            ts::return_shared(registry);
        };

        ts::end(scenario);
    }
}
