-- ============================================================================
-- Prediction market MVP
-- ============================================================================

DO $$ BEGIN
    CREATE TYPE prediction_market_status AS ENUM (
        'open',
        'resolved',
        'cancelled'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

DO $$ BEGIN
    CREATE TYPE prediction_bet_choice AS ENUM (
        'yes',
        'no'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

CREATE TABLE IF NOT EXISTS prediction_markets (
    id SERIAL PRIMARY KEY,
    market_object_id VARCHAR(66) UNIQUE,
    market_tweet_id VARCHAR(64) NOT NULL UNIQUE,
    creator_xid VARCHAR(64) NOT NULL,
    creator_handle VARCHAR(64) NOT NULL,
    question TEXT NOT NULL,
    create_tx_digest VARCHAR(66),
    status prediction_market_status NOT NULL DEFAULT 'open',
    outcome prediction_bet_choice,
    resolved_by_tweet_id VARCHAR(64),
    resolve_tx_digest VARCHAR(66),
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_prediction_markets_object_id ON prediction_markets(market_object_id);
CREATE INDEX IF NOT EXISTS idx_prediction_markets_tweet_id ON prediction_markets(market_tweet_id);
CREATE INDEX IF NOT EXISTS idx_prediction_markets_creator_xid ON prediction_markets(creator_xid);
CREATE INDEX IF NOT EXISTS idx_prediction_markets_status ON prediction_markets(status);

CREATE TABLE IF NOT EXISTS prediction_market_bets (
    id SERIAL PRIMARY KEY,
    market_id INTEGER NOT NULL REFERENCES prediction_markets(id) ON DELETE CASCADE,
    bet_tweet_id VARCHAR(64) NOT NULL UNIQUE,
    bettor_xid VARCHAR(64) NOT NULL,
    bettor_handle VARCHAR(64) NOT NULL,
    choice prediction_bet_choice NOT NULL,
    coin_type VARCHAR(256) NOT NULL,
    amount BIGINT NOT NULL,
    bet_tx_digest VARCHAR(66) NOT NULL,
    payout_tx_digest VARCHAR(66),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_prediction_market_bets_market_id ON prediction_market_bets(market_id);
CREATE INDEX IF NOT EXISTS idx_prediction_market_bets_bettor_xid ON prediction_market_bets(bettor_xid);
CREATE INDEX IF NOT EXISTS idx_prediction_market_bets_choice ON prediction_market_bets(choice);
CREATE INDEX IF NOT EXISTS idx_prediction_market_bets_bet_tweet_id ON prediction_market_bets(bet_tweet_id);

-- ============================================================================
-- Reward campaign MVP
-- ============================================================================

DO $$ BEGIN
    CREATE TYPE reward_campaign_type AS ENUM (
        'top_replies',
        'first_hashtag'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

DO $$ BEGIN
    CREATE TYPE reward_campaign_status AS ENUM (
        'open',
        'resolved',
        'cancelled'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

CREATE TABLE IF NOT EXISTS reward_campaigns (
    id SERIAL PRIMARY KEY,
    campaign_object_id VARCHAR(66) UNIQUE,
    campaign_tweet_id VARCHAR(64) NOT NULL UNIQUE,
    creator_xid VARCHAR(64) NOT NULL,
    creator_handle VARCHAR(64) NOT NULL,
    campaign_type reward_campaign_type NOT NULL,
    target VARCHAR(256) NOT NULL,
    coin_type VARCHAR(256) NOT NULL,
    reward_amount BIGINT NOT NULL,
    max_winners BIGINT NOT NULL CHECK (max_winners BETWEEN 1 AND 10),
    create_tx_digest VARCHAR(66),
    status reward_campaign_status NOT NULL DEFAULT 'open',
    resolved_by_tweet_id VARCHAR(64),
    resolve_tx_digest VARCHAR(66),
    selected_winner_count INTEGER NOT NULL DEFAULT 0,
    paid_winner_count INTEGER NOT NULL DEFAULT 0,
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reward_campaigns_object_id ON reward_campaigns(campaign_object_id);
CREATE INDEX IF NOT EXISTS idx_reward_campaigns_tweet_id ON reward_campaigns(campaign_tweet_id);
CREATE INDEX IF NOT EXISTS idx_reward_campaigns_creator_xid ON reward_campaigns(creator_xid);
CREATE INDEX IF NOT EXISTS idx_reward_campaigns_status ON reward_campaigns(status);

CREATE TABLE IF NOT EXISTS reward_campaign_winners (
    id SERIAL PRIMARY KEY,
    campaign_id INTEGER NOT NULL REFERENCES reward_campaigns(id) ON DELETE CASCADE,
    winner_xid VARCHAR(64) NOT NULL,
    winner_handle VARCHAR(64) NOT NULL,
    winner_tweet_id VARCHAR(64),
    rank INTEGER NOT NULL,
    reward_amount BIGINT NOT NULL,
    claim_tx_digest VARCHAR(66),
    selected_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    UNIQUE(campaign_id, winner_xid)
);

CREATE INDEX IF NOT EXISTS idx_reward_campaign_winners_campaign_id ON reward_campaign_winners(campaign_id);
CREATE INDEX IF NOT EXISTS idx_reward_campaign_winners_winner_xid ON reward_campaign_winners(winner_xid);
