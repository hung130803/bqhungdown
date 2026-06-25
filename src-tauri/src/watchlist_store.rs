//! Persistent store for auto-watched channels (the "Theo dõi kênh" feature).
//!
//! A small JSON file (`app_config_dir/watchlist.json`) holding a list of
//! [`WatchedChannel`]. Mirrors the atomic write strategy of `settings_store`
//! (write tmp + rename) so a crash mid-write can't corrupt the list. Kept
//! independent of `AppHandle`: the caller resolves the path and passes it in.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::error::AppResult;
use crate::models::WatchedChannel;

pub struct WatchlistStore {
    inner: RwLock<Vec<WatchedChannel>>,
    path: PathBuf,
}

impl WatchlistStore {
    /// Load the watchlist from `path`. Missing or corrupt file → empty list
    /// (non-fatal; the user just hasn't added channels yet).
    pub fn load(path: PathBuf) -> Self {
        let inner = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Vec<WatchedChannel>>(&text).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        Self {
            inner: RwLock::new(inner),
            path,
        }
    }

    pub fn list(&self) -> Vec<WatchedChannel> {
        self.inner.read().unwrap().clone()
    }

    pub fn get(&self, id: &str) -> Option<WatchedChannel> {
        self.inner.read().unwrap().iter().find(|c| c.id == id).cloned()
    }

    /// True if a channel with the same (normalised) URL is already watched.
    pub fn contains_url(&self, url: &str) -> bool {
        let u = url.trim().trim_end_matches('/').to_lowercase();
        self.inner
            .read()
            .unwrap()
            .iter()
            .any(|c| c.url.trim().trim_end_matches('/').to_lowercase() == u)
    }

    pub fn add(&self, channel: WatchedChannel) -> AppResult<()> {
        {
            let mut guard = self.inner.write().unwrap();
            guard.push(channel);
        }
        self.persist()
    }

    pub fn remove(&self, id: &str) -> AppResult<()> {
        {
            let mut guard = self.inner.write().unwrap();
            guard.retain(|c| c.id != id);
        }
        self.persist()
    }

    /// Mutate one channel by id (if present) and persist. Returns the updated
    /// channel, or `None` when the id no longer exists.
    pub fn update<F>(&self, id: &str, f: F) -> AppResult<Option<WatchedChannel>>
    where
        F: FnOnce(&mut WatchedChannel),
    {
        let updated = {
            let mut guard = self.inner.write().unwrap();
            match guard.iter_mut().find(|c| c.id == id) {
                Some(c) => {
                    f(c);
                    Some(c.clone())
                }
                None => None,
            }
        };
        if updated.is_some() {
            self.persist()?;
        }
        Ok(updated)
    }

    /// Atomic write: serialize → tmp → rename.
    fn persist(&self) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let snapshot = self.inner.read().unwrap().clone();
        let json = serde_json::to_string_pretty(&snapshot)?;
        let tmp = tmp_sibling(&self.path);
        fs::write(&tmp, json.as_bytes())?;
        if let Err(e) = fs::rename(&tmp, &self.path) {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("watchlist.json"));
    name.push(".tmp");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}
