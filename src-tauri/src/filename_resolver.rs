use crate::models::ConflictPolicy;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// The final absolute path the downloader should write to.
    Path(PathBuf),
    /// User must choose; backend should emit `download://conflict` event.
    /// `suggested` is the auto-rename candidate so frontend can preview it.
    AskUser { suggested: PathBuf, conflicting: PathBuf },
    /// User policy was Skip; downloader should mark item skipped without writing.
    SkipItem,
}

/// Sanitize a video title for use as a filename:
/// - strip path separators and OS-forbidden chars: `< > : " / \ | ? *`
/// - strip control chars (0x00..0x1F, 0x7F)
/// - trim leading/trailing whitespace and dots
/// - cap to 200 chars (UTF-8 boundary safe)
/// - fallback to "video" if empty after sanitization
pub fn sanitize(title: &str) -> String {
    const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let cleaned: String = title
        .chars()
        .filter(|c| !FORBIDDEN.contains(c) && !c.is_control())
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();

    // Cap to 200 chars (UTF-8 codepoint count, not bytes; cheap approximation)
    let capped: String = trimmed.chars().take(200).collect();
    if capped.is_empty() { "video".to_string() } else { capped }
}

/// Build "<stem>.<ext>" or "<stem> (n).<ext>" path that does not exist in the folder.
/// `existing` returns true if a path is occupied; this is parameterized so tests can
/// pass an in-memory set without touching disk.
pub fn auto_rename<F: Fn(&Path) -> bool>(
    save_folder: &Path,
    stem: &str,
    ext: &str,
    existing: F,
) -> PathBuf {
    let initial = save_folder.join(format!("{stem}.{ext}"));
    if !existing(&initial) { return initial; }
    let mut n: u32 = 1;
    loop {
        let candidate = save_folder.join(format!("{stem} ({n}).{ext}"));
        if !existing(&candidate) { return candidate; }
        n += 1;
        if n == u32::MAX { return candidate; } // safety stop
    }
}

/// Resolve the final output path given a `ConflictPolicy`. Pure function; no I/O.
/// `existing(path)` is the disk-existence oracle.
pub fn resolve<F: Fn(&Path) -> bool>(
    save_folder: &Path,
    title: &str,
    ext: &str,
    policy: ConflictPolicy,
    existing: F,
) -> ResolveOutcome {
    let sanitized = sanitize(title);
    let candidate = save_folder.join(format!("{sanitized}.{ext}"));
    if !existing(&candidate) {
        return ResolveOutcome::Path(candidate);
    }
    match policy {
        ConflictPolicy::Overwrite => ResolveOutcome::Path(candidate),
        ConflictPolicy::Skip => ResolveOutcome::SkipItem,
        ConflictPolicy::Rename => {
            let renamed = auto_rename(save_folder, &sanitized, ext, &existing);
            ResolveOutcome::Path(renamed)
        }
        ConflictPolicy::Ask => {
            let suggested = auto_rename(save_folder, &sanitized, ext, &existing);
            ResolveOutcome::AskUser { suggested, conflicting: candidate }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn s(p: &Path) -> String { p.to_string_lossy().to_string() }

    #[test]
    fn sanitize_strips_forbidden() {
        assert_eq!(sanitize("a/b\\c?d:e"), "abcde");
        assert_eq!(sanitize("   ...hello..."), "hello");
        assert_eq!(sanitize(""), "video");
        assert_eq!(sanitize(":\"<>?"), "video");
    }

    #[test]
    fn auto_rename_minimal_n() {
        let folder = PathBuf::from("C:/tmp");
        let mut existing: HashSet<String> = HashSet::new();
        existing.insert(s(&folder.join("song.mp3")));
        existing.insert(s(&folder.join("song (1).mp3")));
        let exists = |p: &Path| existing.contains(&s(p));
        let got = auto_rename(&folder, "song", "mp3", exists);
        assert_eq!(got, folder.join("song (2).mp3"));
    }

    #[test]
    fn resolve_skip_returns_skip_item() {
        let folder = PathBuf::from("C:/tmp");
        let mut existing: HashSet<String> = HashSet::new();
        existing.insert(s(&folder.join("v.mp4")));
        let exists = |p: &Path| existing.contains(&s(p));
        let out = resolve(&folder, "v", "mp4", ConflictPolicy::Skip, exists);
        assert_eq!(out, ResolveOutcome::SkipItem);
    }
}
