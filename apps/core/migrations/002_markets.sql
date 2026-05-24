-- Migration 002: prediction markets tables

-- Markets table
CREATE TABLE IF NOT EXISTS markets (
    id SERIAL PRIMARY KEY,
    market_tweet_id VARCHAR(64) NOT NULL UNIQUE,
    sui_object_id VARCHAR(66) NOT NULL UNIQUE,
    creator_xid VARCHAR(64) NOT NULL,
    question TEXT NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'open',  -- open | resolved
    outcome BOOLEAN,                              -- NULL until resolved
    fee_bps SMALLINT NOT NULL DEFAULT 100,
    tx_digest VARCHAR(66),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_markets_market_tweet_id ON markets(market_tweet_id);
CREATE INDEX IF NOT EXISTS idx_markets_creator_xid ON markets(creator_xid);
CREATE INDEX IF NOT EXISTS idx_markets_status ON markets(status);

-- Market bets table
CREATE TABLE IF NOT EXISTS market_bets (
    id SERIAL PRIMARY KEY,
    market_tweet_id VARCHAR(64) NOT NULL REFERENCES markets(market_tweet_id),
    bet_tweet_id VARCHAR(64) NOT NULL UNIQUE,
    better_xid VARCHAR(64) NOT NULL,
    side BOOLEAN NOT NULL,      -- true = yes, false = no
    coin_type VARCHAR(256) NOT NULL,
    amount BIGINT NOT NULL,
    tx_digest VARCHAR(66),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_market_bets_market_tweet_id ON market_bets(market_tweet_id);
CREATE INDEX IF NOT EXISTS idx_market_bets_better_xid ON market_bets(better_xid);
CREATE INDEX IF NOT EXISTS idx_market_bets_bet_tweet_id ON market_bets(bet_tweet_id);
