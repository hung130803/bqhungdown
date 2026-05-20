CREATE TABLE IF NOT EXISTS history (
  short_id      TEXT PRIMARY KEY,
  url           TEXT NOT NULL,
  title         TEXT NOT NULL,
  extractor     TEXT NOT NULL,
  format_id     TEXT,
  mode          TEXT NOT NULL,
  save_folder   TEXT NOT NULL,
  output_path   TEXT,
  status        TEXT NOT NULL,
  error         TEXT,
  finished_at   INTEGER NOT NULL  -- unix epoch ms
);
CREATE INDEX IF NOT EXISTS idx_history_finished_at ON history(finished_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_title       ON history(title);
