-- Add `channel` (uploader name) and `thumbnail` URL columns to history.
-- Both nullable; older rows inserted before this migration will have NULL.
ALTER TABLE history ADD COLUMN channel    TEXT;
ALTER TABLE history ADD COLUMN thumbnail  TEXT;
