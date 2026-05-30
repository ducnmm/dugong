// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

#[test_only]
module dugong::reward_campaigns_tests {
    use sui::test_scenario::{Self as ts, Scenario};
    use sui::coin;
    use sui::sui::SUI;
    use dugong::core::{Self, DugongRegistry, DugongAccount};
    use dugong::reward_campaigns::{Self, RewardCampaign};
    use dugong::account;
    use dugong::assets;

    fun creator(): address { @0xCAFE }
    fun winner_a(): address { @0xA }

    const FIVE_SUI: u64 = 5_000_000_000;

    // ====== Helpers ======

    fun init_core(scenario: &mut Scenario) {
        ts::next_tx(scenario, creator());
        { core::init_for_testing(ts::ctx(scenario)); };
    }

    fun setup_account(scenario: &mut Scenario, owner: address, xid: vector<u8>, handle: vector<u8>) {
        ts::next_tx(scenario, owner);
        {
            let mut registry = ts::take_shared<DugongRegistry>(scenario);
            account::init_account_no_signature(&mut registry, xid, handle, ts::ctx(scenario));
            ts::return_shared(registry);
        };
    }

    fun deposit_sui(scenario: &mut Scenario, sender: address, xid: vector<u8>, amount: u64) {
        ts::next_tx(scenario, sender);
        {
            let registry = ts::take_shared<DugongRegistry>(scenario);
            let account_id = core::registry_get_account_id(&registry, std::string::utf8(xid));
            let mut account = ts::take_shared_by_id<DugongAccount>(scenario, account_id);
            let coin = coin::mint_for_testing<SUI>(amount, ts::ctx(scenario));
            assets::deposit_coin<SUI>(&mut account, coin, ts::ctx(scenario));
            ts::return_shared(account);
            ts::return_shared(registry);
        };
    }

    fun sui_type(): vector<u8> {
        std::type_name::get<SUI>().into_string().into_bytes()
    }

    fun balance_of(scenario: &mut Scenario, sender: address, xid: vector<u8>): u64 {
        ts::next_tx(scenario, sender);
        let registry = ts::take_shared<DugongRegistry>(scenario);
        let account_id = core::registry_get_account_id(&registry, std::string::utf8(xid));
        let account = ts::take_shared_by_id<DugongAccount>(scenario, account_id);
        let bal = assets::get_balance<SUI>(&account);
        ts::return_shared(account);
        ts::return_shared(registry);
        bal
    }

    /// Create a `top N replies, FIVE_SUI each` campaign funded by `creator`.
    fun create_campaign(scenario: &mut Scenario, max_winners: u64, timestamp_ms: u64) {
        ts::next_tx(scenario, creator());
        {
            let registry = ts::take_shared<DugongRegistry>(scenario);
            let creator_id = core::registry_get_account_id(&registry, std::string::utf8(b"creator_xid"));
            let mut creator_account = ts::take_shared_by_id<DugongAccount>(scenario, creator_id);
            reward_campaigns::create_campaign<SUI>(
                &mut creator_account,
                b"campaign_tweet",
                reward_campaigns::campaign_top_replies(),
                b"replies",
                FIVE_SUI,
                max_winners,
                sui_type(),
                timestamp_ms,
                &b"sig",
                ts::ctx(scenario),
            );
            ts::return_shared(creator_account);
            ts::return_shared(registry);
        };
    }

    // ====== Tests ======

    #[test]
    fun test_create_escrows_full_budget() {
        let mut scenario = ts::begin(creator());
        init_core(&mut scenario);
        setup_account(&mut scenario, creator(), b"creator_xid", b"boss");
        deposit_sui(&mut scenario, creator(), b"creator_xid", 3 * FIVE_SUI); // fund 15 SUI

        create_campaign(&mut scenario, 3, 1000);

        // Entire 15 SUI budget escrowed → creator balance drained to 0.
        assert!(balance_of(&mut scenario, creator(), b"creator_xid") == 0);
        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 22)] // EInvalidCampaignType
    fun test_invalid_campaign_type_rejected() {
        let mut scenario = ts::begin(creator());
        init_core(&mut scenario);
        setup_account(&mut scenario, creator(), b"creator_xid", b"boss");
        deposit_sui(&mut scenario, creator(), b"creator_xid", 3 * FIVE_SUI);

        ts::next_tx(&mut scenario, creator());
        {
            let registry = ts::take_shared<DugongRegistry>(&scenario);
            let creator_id = core::registry_get_account_id(&registry, std::string::utf8(b"creator_xid"));
            let mut creator_account = ts::take_shared_by_id<DugongAccount>(&scenario, creator_id);
            reward_campaigns::create_campaign<SUI>(
                &mut creator_account, b"campaign_tweet", 9, b"replies",
                FIVE_SUI, 3, sui_type(), 1000, &b"sig", ts::ctx(&mut scenario),
            );
            ts::return_shared(creator_account);
            ts::return_shared(registry);
        };
        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 23)] // EInvalidRewardAmount
    fun test_zero_reward_rejected() {
        let mut scenario = ts::begin(creator());
        init_core(&mut scenario);
        setup_account(&mut scenario, creator(), b"creator_xid", b"boss");

        ts::next_tx(&mut scenario, creator());
        {
            let registry = ts::take_shared<DugongRegistry>(&scenario);
            let creator_id = core::registry_get_account_id(&registry, std::string::utf8(b"creator_xid"));
            let mut creator_account = ts::take_shared_by_id<DugongAccount>(&scenario, creator_id);
            reward_campaigns::create_campaign<SUI>(
                &mut creator_account, b"campaign_tweet", reward_campaigns::campaign_top_replies(),
                b"replies", 0, 3, sui_type(), 1000, &b"sig", ts::ctx(&mut scenario),
            );
            ts::return_shared(creator_account);
            ts::return_shared(registry);
        };
        ts::end(scenario);
    }

    #[test]
    fun test_resolve_refunds_unallocated_slots() {
        let mut scenario = ts::begin(creator());
        init_core(&mut scenario);
        setup_account(&mut scenario, creator(), b"creator_xid", b"boss");
        deposit_sui(&mut scenario, creator(), b"creator_xid", 3 * FIVE_SUI); // 15 SUI
        create_campaign(&mut scenario, 3, 1000); // top 3 → budget 15

        // Resolve naming only 2 winners → 1 unused slot (5 SUI) refunded.
        ts::next_tx(&mut scenario, creator());
        {
            let registry = ts::take_shared<DugongRegistry>(&scenario);
            let creator_id = core::registry_get_account_id(&registry, std::string::utf8(b"creator_xid"));
            let mut creator_account = ts::take_shared_by_id<DugongAccount>(&scenario, creator_id);
            let mut campaign = ts::take_shared<RewardCampaign>(&scenario);
            let winners = vector[b"xid_a", b"xid_b"];
            reward_campaigns::resolve_campaign<SUI>(
                &mut campaign, &mut creator_account, winners,
                sui_type(), b"solve_tweet", 2000, &b"sig",
            );
            assert!(reward_campaigns::selected_winners(&campaign) == 2);
            assert!(reward_campaigns::is_resolved(&campaign));
            ts::return_shared(campaign);
            ts::return_shared(creator_account);
            ts::return_shared(registry);
        };

        assert!(balance_of(&mut scenario, creator(), b"creator_xid") == FIVE_SUI);
        ts::end(scenario);
    }

    #[test]
    fun test_winner_claims_equal_share() {
        let mut scenario = ts::begin(creator());
        init_core(&mut scenario);
        setup_account(&mut scenario, creator(), b"creator_xid", b"boss");
        setup_account(&mut scenario, winner_a(), b"xid_a", b"alice");
        deposit_sui(&mut scenario, creator(), b"creator_xid", 2 * FIVE_SUI); // 10 SUI
        create_campaign(&mut scenario, 2, 1000); // top 2 → budget 10

        // Resolve naming winner xid_a only.
        ts::next_tx(&mut scenario, creator());
        {
            let registry = ts::take_shared<DugongRegistry>(&scenario);
            let creator_id = core::registry_get_account_id(&registry, std::string::utf8(b"creator_xid"));
            let mut creator_account = ts::take_shared_by_id<DugongAccount>(&scenario, creator_id);
            let mut campaign = ts::take_shared<RewardCampaign>(&scenario);
            reward_campaigns::resolve_campaign<SUI>(
                &mut campaign, &mut creator_account, vector[b"xid_a"],
                sui_type(), b"solve_tweet", 2000, &b"sig",
            );
            ts::return_shared(campaign);
            ts::return_shared(creator_account);
            ts::return_shared(registry);
        };

        // Winner claims their FIVE_SUI share.
        ts::next_tx(&mut scenario, winner_a());
        {
            let registry = ts::take_shared<DugongRegistry>(&scenario);
            let winner_id = core::registry_get_account_id(&registry, std::string::utf8(b"xid_a"));
            let mut winner_account = ts::take_shared_by_id<DugongAccount>(&scenario, winner_id);
            let mut campaign = ts::take_shared<RewardCampaign>(&scenario);
            reward_campaigns::claim_reward<SUI>(&mut campaign, &mut winner_account, sui_type(), 3000);
            ts::return_shared(campaign);
            ts::return_shared(winner_account);
            ts::return_shared(registry);
        };

        assert!(balance_of(&mut scenario, winner_a(), b"xid_a") == FIVE_SUI);
        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 27)] // ERewardAlreadyClaimed
    fun test_double_claim_rejected() {
        let mut scenario = ts::begin(creator());
        init_core(&mut scenario);
        setup_account(&mut scenario, creator(), b"creator_xid", b"boss");
        setup_account(&mut scenario, winner_a(), b"xid_a", b"alice");
        deposit_sui(&mut scenario, creator(), b"creator_xid", 2 * FIVE_SUI);
        create_campaign(&mut scenario, 2, 1000);

        ts::next_tx(&mut scenario, creator());
        {
            let registry = ts::take_shared<DugongRegistry>(&scenario);
            let creator_id = core::registry_get_account_id(&registry, std::string::utf8(b"creator_xid"));
            let mut creator_account = ts::take_shared_by_id<DugongAccount>(&scenario, creator_id);
            let mut campaign = ts::take_shared<RewardCampaign>(&scenario);
            reward_campaigns::resolve_campaign<SUI>(
                &mut campaign, &mut creator_account, vector[b"xid_a"],
                sui_type(), b"solve_tweet", 2000, &b"sig",
            );
            ts::return_shared(campaign);
            ts::return_shared(creator_account);
            ts::return_shared(registry);
        };

        ts::next_tx(&mut scenario, winner_a());
        {
            let registry = ts::take_shared<DugongRegistry>(&scenario);
            let winner_id = core::registry_get_account_id(&registry, std::string::utf8(b"xid_a"));
            let mut winner_account = ts::take_shared_by_id<DugongAccount>(&scenario, winner_id);
            let mut campaign = ts::take_shared<RewardCampaign>(&scenario);
            reward_campaigns::claim_reward<SUI>(&mut campaign, &mut winner_account, sui_type(), 3000);
            // Second claim must abort.
            reward_campaigns::claim_reward<SUI>(&mut campaign, &mut winner_account, sui_type(), 3001);
            ts::return_shared(campaign);
            ts::return_shared(winner_account);
            ts::return_shared(registry);
        };
        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 25)] // ENotCampaignCreator
    fun test_non_creator_resolve_rejected() {
        let mut scenario = ts::begin(creator());
        init_core(&mut scenario);
        setup_account(&mut scenario, creator(), b"creator_xid", b"boss");
        setup_account(&mut scenario, winner_a(), b"xid_a", b"alice");
        deposit_sui(&mut scenario, creator(), b"creator_xid", 2 * FIVE_SUI);
        create_campaign(&mut scenario, 2, 1000);

        // Resolve attempted by a non-creator account.
        ts::next_tx(&mut scenario, winner_a());
        {
            let registry = ts::take_shared<DugongRegistry>(&scenario);
            let other_id = core::registry_get_account_id(&registry, std::string::utf8(b"xid_a"));
            let mut other_account = ts::take_shared_by_id<DugongAccount>(&scenario, other_id);
            let mut campaign = ts::take_shared<RewardCampaign>(&scenario);
            reward_campaigns::resolve_campaign<SUI>(
                &mut campaign, &mut other_account, vector[b"xid_a"],
                sui_type(), b"solve_tweet", 2000, &b"sig",
            );
            ts::return_shared(campaign);
            ts::return_shared(other_account);
            ts::return_shared(registry);
        };
        ts::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = 21)] // ECampaignNotResolved
    fun test_claim_before_resolve_rejected() {
        let mut scenario = ts::begin(creator());
        init_core(&mut scenario);
        setup_account(&mut scenario, creator(), b"creator_xid", b"boss");
        setup_account(&mut scenario, winner_a(), b"xid_a", b"alice");
        deposit_sui(&mut scenario, creator(), b"creator_xid", 2 * FIVE_SUI);
        create_campaign(&mut scenario, 2, 1000);

        // Claim while still open must abort.
        ts::next_tx(&mut scenario, winner_a());
        {
            let registry = ts::take_shared<DugongRegistry>(&scenario);
            let winner_id = core::registry_get_account_id(&registry, std::string::utf8(b"xid_a"));
            let mut winner_account = ts::take_shared_by_id<DugongAccount>(&scenario, winner_id);
            let mut campaign = ts::take_shared<RewardCampaign>(&scenario);
            reward_campaigns::claim_reward<SUI>(&mut campaign, &mut winner_account, sui_type(), 3000);
            ts::return_shared(campaign);
            ts::return_shared(winner_account);
            ts::return_shared(registry);
        };
        ts::end(scenario);
    }
}
