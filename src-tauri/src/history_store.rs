use crate::error::{AppError, AppResult};
use crate::models::{DownloadMode, HistoryEntry, HistoryStatus};
use chrono::{DateTime, TimeZone, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::PathBuf;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_channel_thumb.sql"),
    include_str!("../migrations/0003_edited_flag.sql"),
];

pub struct HistoryStore {
    conn: Mutex<Connection>,
}

impl HistoryStore {
    /// Open (or create) the SQLite database at `db_path` and run idempotent migrations.
    pub fn open(db_path: PathBuf) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;
        // Run migrations one by one; ignore "duplicate column" errors so the
        // ALTER TABLE statements in 0002 are effectively idempotent.
        for sql in MIGRATIONS {
            if let Err(e) = conn.execute_batch(sql) {
                let msg = e.to_string();
                if !msg.contains("duplicate column name") {
                    return Err(e.into());
                }
            }
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Insert (or replace) a history entry keyed by `short_id`.
    ///
    /// NOTE: We use `INSERT ... ON CONFLICT DO UPDATE` instead of
    /// `INSERT OR REPLACE` so the `edited` / `edited_at` columns (set
    /// independently by `set_edited`) are preserved when the runner emits a
    /// fresh entry for the same `short_id`.
    pub fn insert(&self, e: &HistoryEntry) -> AppResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO history (
                short_id, url, title, extractor, format_id, mode,
                save_folder, output_path, status, error, finished_at,
                channel, thumbnail
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(short_id) DO UPDATE SET
                url=excluded.url,
                title=excluded.title,
                extractor=excluded.extractor,
                format_id=excluded.format_id,
                mode=excluded.mode,
                save_folder=excluded.save_folder,
                output_path=excluded.output_path,
                status=excluded.status,
                error=excluded.error,
                finished_at=excluded.finished_at,
                channel=excluded.channel,
                thumbnail=excluded.thumbnail",
            params![
                e.short_id,
                e.url,
                e.title,
                e.extractor,
                e.format_id,
                mode_str(&e.mode),
                e.save_folder.to_string_lossy().to_string(),
                e.output_path.as_ref().map(|p| p.to_string_lossy().to_string()),
                status_str(&e.status),
                e.error,
                dt_to_ts(&e.finished_at),
                e.channel,
                e.thumbnail,
            ],
        )?;
        Ok(())
    }

    /// Mark several entries as edited (or unedited). Returns count of rows
    /// actually changed.
    pub fn set_edited(&self, short_ids: &[String], edited: bool) -> AppResult<u64> {
        if short_ids.is_empty() {
            return Ok(0);
        }
        let conn = self.conn.lock();
        let placeholders = std::iter::repeat("?").take(short_ids.len()).collect::<Vec<_>>().join(",");
        let now = chrono::Utc::now().timestamp_millis();
        let edited_at: Option<i64> = if edited { Some(now) } else { None };
        let sql = format!(
            "UPDATE history SET edited = ?1, edited_at = ?2 WHERE short_id IN ({})",
            placeholders
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(short_ids.len() + 2);
        params.push(Box::new(if edited { 1i64 } else { 0i64 }));
        params.push(Box::new(edited_at));
        for id in short_ids {
            params.push(Box::new(id.clone()));
        }
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let affected = conn.execute(&sql, refs.as_slice())?;
        Ok(affected as u64)
    }

    /// List entries sorted by `finished_at DESC`. When `query` is `Some`, performs a
    /// case-insensitive substring match against both `title` and `url`.
    pub fn list(
        &self,
        query: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<HistoryEntry>> {
        let conn = self.conn.lock();
        let mut entries = Vec::new();

        match query {
            Some(q) => {
                let mut stmt = conn.prepare(
                    "SELECT short_id, url, title, extractor, format_id, mode,
                            save_folder, output_path, status, error, finished_at,
                            channel, thumbnail, edited, edited_at
                     FROM history
                     WHERE title LIKE '%' || ?1 || '%' COLLATE NOCASE
                        OR url   LIKE '%' || ?1 || '%' COLLATE NOCASE
                     ORDER BY finished_at DESC
                     LIMIT ?2 OFFSET ?3",
                )?;
                let rows = stmt.query_map(params![q, limit, offset], row_to_entry)?;
                for row in rows {
                    entries.push(row?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT short_id, url, title, extractor, format_id, mode,
                            save_folder, output_path, status, error, finished_at,
                            channel, thumbnail, edited, edited_at
                     FROM history
                     ORDER BY finished_at DESC
                     LIMIT ?1 OFFSET ?2",
                )?;
                let rows = stmt.query_map(params![limit, offset], row_to_entry)?;
                for row in rows {
                    entries.push(row?);
                }
            }
        }

        Ok(entries)
    }

    /// Delete the entry identified by `short_id`. Returns `NotFound` if no row was removed.
    pub fn delete(&self, short_id: &str) -> AppResult<()> {
        let conn = self.conn.lock();
        let affected = conn.execute(
            "DELETE FROM history WHERE short_id = ?1",
            params![short_id],
        )?;
        if affected == 0 {
            return Err(AppError::NotFound(short_id.to_string()));
        }
        Ok(())
    }

    /// Wipe every entry from history. Used by "Xoá tất cả lịch sử".
    pub fn clear_all(&self) -> AppResult<u64> {
        let conn = self.conn.lock();
        let affected = conn.execute("DELETE FROM history", [])?;
        Ok(affected as u64)
    }

    /// Return the set of all `short_id` values currently stored in history.
    pub fn known_short_ids(&self) -> AppResult<HashSet<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT short_id FROM history")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = HashSet::new();
        for row in rows {
            ids.insert(row?);
        }
        Ok(ids)
    }

    /// Fetch a single entry by `short_id`, or `Ok(None)` if absent.
    pub fn get(&self, short_id: &str) -> AppResult<Option<HistoryEntry>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT short_id, url, title, extractor, format_id, mode,
                    save_folder, output_path, status, error, finished_at,
                    channel, thumbnail, edited, edited_at
             FROM history
             WHERE short_id = ?1",
        )?;
        let entry = stmt
            .query_row(params![short_id], row_to_entry)
            .optional()?;
        Ok(entry)
    }
}

// ---------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let short_id: String = row.get(0)?;
    let url: String = row.get(1)?;
    let title: String = row.get(2)?;
    let extractor: String = row.get(3)?;
    let format_id: Option<String> = row.get(4)?;
    let mode_s: String = row.get(5)?;
    let save_folder_s: String = row.get(6)?;
    let output_path_s: Option<String> = row.get(7)?;
    let status_s: String = row.get(8)?;
    let error: Option<String> = row.get(9)?;
    let finished_at_ms: i64 = row.get(10)?;
    // Optional columns added in migration 0002. Use `.ok()` so old DBs that
    // somehow miss the column don't panic — they'll just return None.
    let channel: Option<String> = row.get(11).ok();
    let thumbnail: Option<String> = row.get(12).ok();
    let edited_int: i64 = row.get::<_, i64>(13).unwrap_or(0);
    let edited_at_ms: Option<i64> = row.get::<_, Option<i64>>(14).ok().flatten();

    Ok(HistoryEntry {
        short_id,
        url,
        title,
        extractor,
        format_id,
        mode: parse_mode(&mode_s),
        save_folder: PathBuf::from(save_folder_s),
        output_path: output_path_s.map(PathBuf::from),
        status: parse_status(&status_s),
        error,
        finished_at: ts_to_dt(finished_at_ms),
        channel,
        thumbnail,
        edited: edited_int != 0,
        edited_at: edited_at_ms.map(ts_to_dt),
    })
}

// ---------------------------------------------------------------------------
// Enum / time helpers
// ---------------------------------------------------------------------------

fn mode_str(m: &DownloadMode) -> &'static str {
    match m {
        DownloadMode::Video => "video",
        DownloadMode::Audio => "audio",
    }
}

fn parse_mode(s: &str) -> DownloadMode {
    match s {
        "audio" => DownloadMode::Audio,
        // "video" or any unexpected value defaults to Video to keep listing resilient.
        _ => DownloadMode::Video,
    }
}

fn status_str(s: &HistoryStatus) -> &'static str {
    match s {
        HistoryStatus::Completed => "completed",
        HistoryStatus::Failed => "failed",
        HistoryStatus::Cancelled => "cancelled",
    }
}

fn parse_status(s: &str) -> HistoryStatus {
    match s {
        "completed" => HistoryStatus::Completed,
        "cancelled" => HistoryStatus::Cancelled,
        // Unknown values map to Failed so corrupt rows surface as failures rather than
        // silently being treated as success.
        _ => HistoryStatus::Failed,
    }
}

fn ts_to_dt(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms).single().unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap())
}

fn dt_to_ts(dt: &DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}
