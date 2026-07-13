-- Idempotency guard for the event indexer.
--
-- Balance handlers apply increment-style updates (balance = balance ± amount),
-- so re-processing an event double-counts. The indexer persists its cursor
-- only after a page is processed, and cursor re-anchoring (GraphQL migration /
-- expired-cursor recovery) adds more re-fetch paths — this ledger lets the
-- processor skip any (tx_digest, event_seq) it has already handled.
CREATE TABLE IF NOT EXISTS indexer_processed_events (
    tx_digest    TEXT        NOT NULL,
    event_seq    TEXT        NOT NULL,
    processed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tx_digest, event_seq)
);
