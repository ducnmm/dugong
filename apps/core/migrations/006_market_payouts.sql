-- Track market payouts mirrored by the processor.
--
-- The on-chain pay_winner function is idempotent per market/coin/winner, but
-- this table prevents the backend from repeatedly submitting no-op payout
-- transactions after an auto-pay or manual claim has already succeeded.
CREATE TABLE IF NOT EXISTS market_payouts (
    id SERIAL PRIMARY KEY,
    market_tweet_id VARCHAR(64) NOT NULL REFERENCES markets(market_tweet_id),
    winner_xid VARCHAR(64) NOT NULL,
    coin_type VARCHAR(256) NOT NULL,
    payout_tweet_id VARCHAR(64),
    tx_digest VARCHAR(66),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (market_tweet_id, winner_xid, coin_type)
);

CREATE INDEX IF NOT EXISTS idx_market_payouts_market_tweet_id ON market_payouts(market_tweet_id);
CREATE INDEX IF NOT EXISTS idx_market_payouts_winner_xid ON market_payouts(winner_xid);
