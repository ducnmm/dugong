-- The indexer now keeps a separate cursor per watched defining-package id (events
-- added in a package upgrade carry the upgraded id, so multiple ids are watched).
-- Non-primary cursors are namespaced as `dugong_events:<package_id>`, which exceeds
-- the original VARCHAR(64): "dugong_events:" (14) + a 0x-prefixed 32-byte id (66) = 80.
-- Widen the column so these cursor state rows fit.
ALTER TABLE indexer_state ALTER COLUMN name TYPE VARCHAR(128);
