// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

#[test_only]
module dugong::account_tests {
    use sui::test_scenario::{Self as ts};
    use dugong::account;
    use dugong::assets;
    use dugong::dug::{Self, DugongAccount, DugongRegistry};

    fun creator(): address { @0xCAFE }

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
}
