-- Track whether the user has finished editing the downloaded video. Used by
-- the "Đã edit" filter and badge in the History page so it's easy to see
-- which raw downloads still need to be processed in CapCut / Premiere.
ALTER TABLE history ADD COLUMN edited INTEGER NOT NULL DEFAULT 0;
ALTER TABLE history ADD COLUMN edited_at INTEGER;
