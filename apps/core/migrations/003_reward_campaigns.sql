-- Migration 003: reward campaigns tables

-- Reward campaigns table
CREATE TABLE IF NOT EXISTS reward_campaigns (
    id SERIAL PRIMARY KEY,
    campaign_tweet_id VARCHAR(64) NOT NULL UNIQUE,
    sui_object_id VARCHAR(66) NOT NULL UNIQUE,
    creator_xid VARCHAR(64) NOT NULL,
    campaign_type SMALLINT NOT NULL,             -- 1 = top replies, 2 = first hashtag
    target TEXT NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'open',   -- open | resolved
    coin_type VARCHAR(256) NOT NULL,
    reward_amount BIGINT NOT NULL,                -- per-winner equal share
    max_winners BIGINT NOT NULL,
    selected_winners BIGINT NOT NULL DEFAULT 0,
    unallocated_refund BIGINT NOT NULL DEFAULT 0,
    tx_digest VARCHAR(66),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reward_campaigns_campaign_tweet_id ON reward_campaigns(campaign_tweet_id);
CREATE INDEX IF NOT EXISTS idx_reward_campaigns_creator_xid ON reward_campaigns(creator_xid);
CREATE INDEX IF NOT EXISTS idx_reward_campaigns_status ON reward_campaigns(status);

-- Reward campaign winners (entitlements) table
CREATE TABLE IF NOT EXISTS reward_campaign_winners (
    id SERIAL PRIMARY KEY,
    campaign_tweet_id VARCHAR(64) NOT NULL REFERENCES reward_campaigns(campaign_tweet_id),
    winner_xid VARCHAR(64) NOT NULL,
    amount BIGINT NOT NULL,
    claimed BOOLEAN NOT NULL DEFAULT FALSE,
    claim_tweet_id VARCHAR(64),
    tx_digest VARCHAR(66),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (campaign_tweet_id, winner_xid)
);

CREATE INDEX IF NOT EXISTS idx_reward_campaign_winners_campaign_tweet_id ON reward_campaign_winners(campaign_tweet_id);
CREATE INDEX IF NOT EXISTS idx_reward_campaign_winners_winner_xid ON reward_campaign_winners(winner_xid);
