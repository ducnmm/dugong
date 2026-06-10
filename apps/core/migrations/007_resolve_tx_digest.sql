-- Persist the on-chain digest of market / campaign resolution transactions so
-- the /tx/:digest endpoint can find them. Previously only the creation digest
-- was stored (markets.tx_digest / reward_campaigns.tx_digest), so opening a
-- resolve transaction returned 404.
ALTER TABLE markets ADD COLUMN IF NOT EXISTS resolve_tx_digest VARCHAR(66);
ALTER TABLE reward_campaigns ADD COLUMN IF NOT EXISTS resolve_tx_digest VARCHAR(66);
