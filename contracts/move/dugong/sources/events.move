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

    // ====== Transfer Events ======

    public struct TransferCompleted has copy, drop {
        from_xid: String,
        to_xid: String,
        tweet_id: String,
        coin_type: String,
        amount: u64,
        timestamp: u64,
    }

    // ====== Prediction Market Events ======

    public struct PredictionMarketCreated has copy, drop {
        market_id: ID,
        market_tweet_id: String,
        creator_xid: String,
        question: String,
        timestamp: u64,
    }

    public struct PredictionBetPlaced has copy, drop {
        market_id: ID,
        market_tweet_id: String,
        bet_tweet_id: String,
        bettor_xid: String,
        choice: String,
        coin_type: String,
        amount: u64,
        yes_pool: u64,
        no_pool: u64,
        timestamp: u64,
    }

    public struct PredictionMarketResolved has copy, drop {
        market_id: ID,
        market_tweet_id: String,
        solve_tweet_id: String,
        creator_xid: String,
        outcome: String,
        timestamp: u64,
    }

    public struct PredictionPayoutClaimed has copy, drop {
        market_id: ID,
        market_tweet_id: String,
        bettor_xid: String,
        outcome: String,
        coin_type: String,
        amount: u64,
        timestamp: u64,
    }

    // ====== Reward Campaign Events ======

    public struct RewardCampaignCreated has copy, drop {
        campaign_id: ID,
        campaign_tweet_id: String,
        creator_xid: String,
        campaign_type: u8,
        target: String,
        coin_type: String,
        reward_amount: u64,
        max_winners: u64,
        total_budget: u64,
        timestamp: u64,
    }

    public struct RewardCampaignResolved has copy, drop {
        campaign_id: ID,
        campaign_tweet_id: String,
        solve_tweet_id: String,
        creator_xid: String,
        winner_xids: vector<String>,
        winner_count: u64,
        unallocated_refund: u64,
        coin_type: String,
        timestamp: u64,
    }

    public struct RewardCampaignClaimed has copy, drop {
        campaign_id: ID,
        campaign_tweet_id: String,
        winner_xid: String,
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

    public(package) fun emit_prediction_market_created(
        market_id: ID,
        market_tweet_id: String,
        creator_xid: String,
        question: String,
        timestamp: u64,
    ) {
        sui::event::emit(PredictionMarketCreated {
            market_id,
            market_tweet_id,
            creator_xid,
            question,
            timestamp,
        });
    }

    public(package) fun emit_prediction_bet_placed(
        market_id: ID,
        market_tweet_id: String,
        bet_tweet_id: String,
        bettor_xid: String,
        choice: String,
        coin_type: String,
        amount: u64,
        yes_pool: u64,
        no_pool: u64,
        timestamp: u64,
    ) {
        sui::event::emit(PredictionBetPlaced {
            market_id,
            market_tweet_id,
            bet_tweet_id,
            bettor_xid,
            choice,
            coin_type,
            amount,
            yes_pool,
            no_pool,
            timestamp,
        });
    }

    public(package) fun emit_prediction_market_resolved(
        market_id: ID,
        market_tweet_id: String,
        solve_tweet_id: String,
        creator_xid: String,
        outcome: String,
        timestamp: u64,
    ) {
        sui::event::emit(PredictionMarketResolved {
            market_id,
            market_tweet_id,
            solve_tweet_id,
            creator_xid,
            outcome,
            timestamp,
        });
    }

    public(package) fun emit_prediction_payout_claimed(
        market_id: ID,
        market_tweet_id: String,
        bettor_xid: String,
        outcome: String,
        coin_type: String,
        amount: u64,
        timestamp: u64,
    ) {
        sui::event::emit(PredictionPayoutClaimed {
            market_id,
            market_tweet_id,
            bettor_xid,
            outcome,
            coin_type,
            amount,
            timestamp,
        });
    }

    public(package) fun emit_reward_campaign_created(
        campaign_id: ID,
        campaign_tweet_id: String,
        creator_xid: String,
        campaign_type: u8,
        target: String,
        coin_type: String,
        reward_amount: u64,
        max_winners: u64,
        total_budget: u64,
        timestamp: u64,
    ) {
        sui::event::emit(RewardCampaignCreated {
            campaign_id,
            campaign_tweet_id,
            creator_xid,
            campaign_type,
            target,
            coin_type,
            reward_amount,
            max_winners,
            total_budget,
            timestamp,
        });
    }

    public(package) fun emit_reward_campaign_resolved(
        campaign_id: ID,
        campaign_tweet_id: String,
        solve_tweet_id: String,
        creator_xid: String,
        winner_xids: vector<String>,
        winner_count: u64,
        unallocated_refund: u64,
        coin_type: String,
        timestamp: u64,
    ) {
        sui::event::emit(RewardCampaignResolved {
            campaign_id,
            campaign_tweet_id,
            solve_tweet_id,
            creator_xid,
            winner_xids,
            winner_count,
            unallocated_refund,
            coin_type,
            timestamp,
        });
    }

    public(package) fun emit_reward_campaign_claimed(
        campaign_id: ID,
        campaign_tweet_id: String,
        winner_xid: String,
        coin_type: String,
        amount: u64,
        timestamp: u64,
    ) {
        sui::event::emit(RewardCampaignClaimed {
            campaign_id,
            campaign_tweet_id,
            winner_xid,
            coin_type,
            amount,
            timestamp,
        });
    }

}
