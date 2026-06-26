//! Persistent store for saved channels/links (the "Đã lưu" feature) — a simple
//! bookmark list the user gathers to download or watch later. JSON on disk with
//! the same atomic write strategy as the other stores.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::error::AppResult;
use crate::models::Bookmark;

pub struct BookmarksStore {
    inner: RwLock<Vec<Bookmark>>,
    path: PathBuf,
}

impl BookmarksStore {
    pub fn load(path: PathBuf) -> Self {
        let inner = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Vec<Bookmark>>(&text).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        Self { inner: RwLock::new(inner), path }
    }

    pub fn list(&self) -> Vec<Bookmark> {
        self.inner.read().unwrap().clone()
    }

    pub fn add(&self, bm: Bookmark) -> AppResult<()> {
        {
            let mut g = self.inner.write().unwrap();
            // Newest on top; skip exact-duplicate URL.
            if !g.iter().any(|b| b.url == bm.url) {
                g.insert(0, bm);
            }
        }
        self.persist()
    }

    pub fn remove(&self, id: &str) -> AppResult<()> {
        self.inner.write().unwrap().retain(|b| b.id != id);
        self.persist()
    }

    pub fn update_note(&self, id: &str, note: String) -> AppResult<()> {
        {
            let mut g = self.inner.write().unwrap();
            if let Some(b) = g.iter_mut().find(|b| b.id == id) {
                b.note = note;
            }
        }
        self.persist()
    }

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
        .unwrap_or_else(|| std::ffi::OsString::from("bookmarks.json"));
    name.push(".tmp");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}
