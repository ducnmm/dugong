// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// Escrowed reward campaigns settled from X activity.
module dugong::reward_campaigns {
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

    const CAMPAIGN_TOP_REPLIES: u8 = 1;
    const CAMPAIGN_FIRST_HASHTAG: u8 = 2;
    const MAX_WINNERS: u64 = 10;

    const ECampaignClosed: u64 = 200;
    const ECampaignNotResolved: u64 = 201;
    const EInvalidCampaignType: u64 = 202;
    const EInvalidRewardAmount: u64 = 203;
    const ETooManyWinners: u64 = 204;
    const ENotCampaignCreator: u64 = 205;
    const ENoRewardEntitlement: u64 = 206;
    const ERewardAlreadyClaimed: u64 = 207;

    public struct RewardCampaign has key {
        id: UID,
        campaign_tweet_id: String,
        creator_xid: String,
        campaign_type: u8,
        target: String,
        status: u8,
        coin_type: ascii::String,
        reward_amount: u64,
        max_winners: u64,
        selected_winners: u64,
        claimed_winners: u64,
        escrow: Bag,
        entitlements: Table<String, RewardEntitlement>,
        created_at: u64,
        resolved_at: u64,
    }

    public struct RewardEntitlement has store {
        amount: u64,
        claimed: bool,
    }

    public fun create_campaign<T, E>(
        creator: &mut DugongAccount,
        campaign_tweet_id: vector<u8>,
        campaign_type: u8,
        target: vector<u8>,
        reward_amount: u64,
        max_winners: u64,
        coin_type: vector<u8>,
        timestamp: u64,
        _signature: &vector<u8>,
        _enclave: &Enclave<E>,
        ctx: &mut TxContext,
    ) {
        assert!(is_valid_campaign_type(campaign_type), EInvalidCampaignType);
        assert!(reward_amount > 0, EInvalidRewardAmount);
        assert!(max_winners > 0 && max_winners <= MAX_WINNERS, ETooManyWinners);

        let campaign_tweet_id_str = string::utf8(campaign_tweet_id);
        let processed_tweets = core::account_processed_tweets(creator);
        assert!(!processed_tweets.contains(campaign_tweet_id_str), core::e_tweet_already_processed());

        let expected_type = type_name::get<T>().into_string();
        assert!(coin_type == expected_type.into_bytes(), core::e_coin_type_mismatch());

        // let payload = core::new_create_reward_campaign_payload(
        //     core::account_xid(creator).into_bytes(),
        //     campaign_tweet_id,
        //     campaign_type,
        //     target,
        //     reward_amount,
        //     max_winners,
        //     coin_type,
        // );
        // let is_valid = enclave.verify_signature(
        //     core::create_reward_campaign_intent(),
        //     timestamp,
        //     payload,
        //     signature,
        // );
        // assert!(is_valid, core::e_invalid_signature());

        assert!(timestamp > core::account_last_timestamp(creator), core::e_replay_attempt());
        core::account_set_last_timestamp(creator, timestamp);
        core::account_add_processed_tweet(creator, campaign_tweet_id_str);

        let total_budget = reward_amount * max_winners;
        let mut campaign = RewardCampaign {
            id: object::new(ctx),
            campaign_tweet_id: campaign_tweet_id_str,
            creator_xid: core::account_xid(creator),
            campaign_type,
            target: string::utf8(target),
            status: STATUS_OPEN,
            coin_type: type_name::get<T>().into_string(),
            reward_amount,
            max_winners,
            selected_winners: 0,
            claimed_winners: 0,
            escrow: bag::new(ctx),
            entitlements: table::new(ctx),
            created_at: timestamp,
            resolved_at: 0,
        };

        transfer_from_account_to_campaign<T>(creator, &mut campaign, total_budget);

        events::emit_reward_campaign_created(
            object::id(&campaign),
            campaign.campaign_tweet_id,
            campaign.creator_xid,
            campaign.campaign_type,
            campaign.target,
            campaign.coin_type.to_string(),
            campaign.reward_amount,
            campaign.max_winners,
            total_budget,
            timestamp,
        );

        transfer::share_object(campaign);
    }

    public fun resolve_campaign<T, E>(
        campaign: &mut RewardCampaign,
        creator: &mut DugongAccount,
        winner_xids: vector<vector<u8>>,
        coin_type: vector<u8>,
        solve_tweet_id: vector<u8>,
        timestamp: u64,
        _signature: &vector<u8>,
        _enclave: &Enclave<E>,
    ) {
        assert!(campaign.status == STATUS_OPEN, ECampaignClosed);
        assert!(core::account_xid(creator) == campaign.creator_xid, ENotCampaignCreator);

        let expected_type = type_name::get<T>().into_string();
        assert!(coin_type == expected_type.into_bytes(), core::e_coin_type_mismatch());
        assert!(campaign.coin_type == type_name::get<T>().into_string(), core::e_coin_type_mismatch());

        let solve_tweet_id_str = string::utf8(solve_tweet_id);
        let processed_tweets = core::account_processed_tweets(creator);
        assert!(!processed_tweets.contains(solve_tweet_id_str), core::e_tweet_already_processed());

        // let payload = core::new_resolve_reward_campaign_payload(
        //     core::account_xid(creator).into_bytes(),
        //     campaign.campaign_tweet_id.into_bytes(),
        //     solve_tweet_id,
        // );
        // let is_valid = enclave.verify_signature(
        //     core::resolve_reward_campaign_intent(),
        //     timestamp,
        //     payload,
        //     signature,
        // );
        // assert!(is_valid, core::e_invalid_signature());

        assert!(timestamp > core::account_last_timestamp(creator), core::e_replay_attempt());
        core::account_set_last_timestamp(creator, timestamp);
        core::account_add_processed_tweet(creator, solve_tweet_id_str);

        let submitted_count = vector::length(&winner_xids);
        assert!(submitted_count <= campaign.max_winners, ETooManyWinners);

        let mut selected_winner_xids = vector::empty<String>();
        let mut i = 0;
        while (i < submitted_count) {
            let winner_xid = string::utf8(*vector::borrow(&winner_xids, i));
            if (!campaign.entitlements.contains(winner_xid)) {
                campaign.entitlements.add(winner_xid, RewardEntitlement {
                    amount: campaign.reward_amount,
                    claimed: false,
                });
                campaign.selected_winners = campaign.selected_winners + 1;
                vector::push_back(&mut selected_winner_xids, winner_xid);
            };
            i = i + 1;
        };

        let unallocated_slots = campaign.max_winners - campaign.selected_winners;
        let unallocated_refund = unallocated_slots * campaign.reward_amount;
        if (unallocated_refund > 0) {
            transfer_from_campaign_to_account<T>(campaign, creator, unallocated_refund);
        };

        campaign.status = STATUS_RESOLVED;
        campaign.resolved_at = timestamp;

        events::emit_reward_campaign_resolved(
            object::id(campaign),
            campaign.campaign_tweet_id,
            solve_tweet_id_str,
            campaign.creator_xid,
            selected_winner_xids,
            campaign.selected_winners,
            unallocated_refund,
            campaign.coin_type.to_string(),
            timestamp,
        );
    }

    public fun claim_reward<T>(
        campaign: &mut RewardCampaign,
        winner: &mut DugongAccount,
        coin_type: vector<u8>,
        timestamp: u64,
    ) {
        assert!(campaign.status == STATUS_RESOLVED, ECampaignNotResolved);

        let expected_type = type_name::get<T>().into_string();
        assert!(coin_type == expected_type.into_bytes(), core::e_coin_type_mismatch());
        assert!(campaign.coin_type == type_name::get<T>().into_string(), core::e_coin_type_mismatch());

        let winner_xid = core::account_xid(winner);
        assert!(campaign.entitlements.contains(winner_xid), ENoRewardEntitlement);
        let entitlement = campaign.entitlements.borrow_mut(winner_xid);
        assert!(!entitlement.claimed, ERewardAlreadyClaimed);

        let amount = entitlement.amount;
        entitlement.claimed = true;
        campaign.claimed_winners = campaign.claimed_winners + 1;

        transfer_from_campaign_to_account<T>(campaign, winner, amount);

        events::emit_reward_campaign_claimed(
            object::id(campaign),
            campaign.campaign_tweet_id,
            winner_xid,
            campaign.coin_type.to_string(),
            amount,
            timestamp,
        );
    }

    public fun is_open(campaign: &RewardCampaign): bool {
        campaign.status == STATUS_OPEN
    }

    public fun is_resolved(campaign: &RewardCampaign): bool {
        campaign.status == STATUS_RESOLVED
    }

    public fun campaign_tweet_id(campaign: &RewardCampaign): String {
        campaign.campaign_tweet_id
    }

    public fun creator_xid(campaign: &RewardCampaign): String {
        campaign.creator_xid
    }

    public fun selected_winners(campaign: &RewardCampaign): u64 {
        campaign.selected_winners
    }

    public fun campaign_top_replies(): u8 {
        CAMPAIGN_TOP_REPLIES
    }

    public fun campaign_first_hashtag(): u8 {
        CAMPAIGN_FIRST_HASHTAG
    }

    fun is_valid_campaign_type(campaign_type: u8): bool {
        campaign_type == CAMPAIGN_TOP_REPLIES || campaign_type == CAMPAIGN_FIRST_HASHTAG
    }

    fun transfer_from_account_to_campaign<T>(
        account: &mut DugongAccount,
        campaign: &mut RewardCampaign,
        amount: u64,
    ) {
        let type_key = type_name::get<T>().into_string();
        let account_balances = core::account_balances_mut(account);
        assert!(account_balances.contains(type_key), core::e_insufficient_balance());

        let account_balance = account_balances.borrow_mut<ascii::String, Balance<T>>(type_key);
        assert!(account_balance.value() >= amount, core::e_insufficient_balance());
        let stake = account_balance.split(amount);

        if (campaign.escrow.contains(type_key)) {
            let escrow_balance = campaign.escrow.borrow_mut<ascii::String, Balance<T>>(type_key);
            escrow_balance.join(stake);
        } else {
            campaign.escrow.add(type_key, stake);
        };
    }

    fun transfer_from_campaign_to_account<T>(
        campaign: &mut RewardCampaign,
        account: &mut DugongAccount,
        amount: u64,
    ) {
        let type_key = type_name::get<T>().into_string();
        assert!(campaign.escrow.contains(type_key), core::e_insufficient_balance());

        let escrow_balance = campaign.escrow.borrow_mut<ascii::String, Balance<T>>(type_key);
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

}
