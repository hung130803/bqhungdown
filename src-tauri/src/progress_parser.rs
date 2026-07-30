//! Parse yt-dlp progress lines into ProgressSnapshot.
//!
//! Supports two formats:
//! 1) Custom `--progress-template "download:DLPROG|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.speed)s|%(progress.eta)s"`
//!    → Lines look like: `DLPROG|1234567|45678901|2543210.5|123`
//!    Any "NA" or empty value means unknown/None.
//! 2) Fallback default yt-dlp `[download]` progress lines:
//!    `[download]   1.2% of  120.50MiB at  3.10MiB/s ETA 00:39`
//!
//! Plus stage markers (`[Merger]`, `[ExtractAudio]`) for state UI hints.

use crate::models::ProgressSnapshot;
use regex::Regex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Parse a single line from yt-dlp stdout. Returns None for non-progress lines.
pub fn parse_progress(line: &str) -> Option<ProgressSnapshot> {
    if let Some(snap) = parse_dlprog(line) { return Some(snap); }
    if let Some(snap) = parse_aria2(line) { return Some(snap); }
    parse_fallback(line)
}

fn parse_dlprog(line: &str) -> Option<ProgressSnapshot> {
    let line = line.trim();
    let rest = line.strip_prefix("DLPROG|")?;
    let mut parts = rest.split('|');
    let downloaded = parse_u64(parts.next()?);
    let total      = parse_u64_opt(parts.next()?);
    let speed      = parse_f64_opt(parts.next()?);
    let eta        = parse_u64_opt(parts.next().unwrap_or(""));

    let downloaded = downloaded?;
    let percent = match total {
        Some(t) if t > 0 => Some(((downloaded as f64) / (t as f64) * 100.0) as f32),
        _ => None,
    };
    Some(ProgressSnapshot {
        bytes_downloaded: downloaded,
        bytes_total: total,
        speed_bps: speed,
        eta_sec: eta,
        percent,
    })
}

/// Parse aria2c summary line. When yt-dlp delegates to aria2c, the
/// `--summary-interval=1` flag makes aria2c emit lines like:
///
/// ```text
/// [#abc123 12MiB/100MiB(12%) CN:8 DL:5.5MiB ETA:15s]
/// [#abc123 50.0MiB/100MiB(50%) CN:16 DL:8.2MiB]
/// ```
///
/// Some variants without total size: `[#abc123 12MiB CN:8 DL:5.5MiB]`
///
/// Field meanings:
/// - `<dl>/<total>(<pct>%)` — current/total bytes, percent (total may be absent)
/// - `CN:<n>`  — number of active connections
/// - `DL:<rate>` — download speed (bytes/sec, with KiB/MiB suffix)
/// - `ETA:<sec>` — estimated remaining seconds (may be absent)
fn parse_aria2(line: &str) -> Option<ProgressSnapshot> {
    static RE_FULL: OnceLock<Regex> = OnceLock::new();
    static RE_NO_TOTAL: OnceLock<Regex> = OnceLock::new();

    let line = line.trim();
    if !line.starts_with('[') { return None; }

    // Try full form: `<dl>/<total>(<pct>%) ... DL:<rate> [ETA:<sec>]`
    let re_full = RE_FULL.get_or_init(|| {
        Regex::new(
            r"^\[#[A-Za-z0-9]+\s+([\d.]+)([KMG]i?B)?/([\d.]+)([KMG]i?B)?\((\d+)%\).*?DL:([\d.]+)([KMG]i?B)?(?:\s+ETA:(\d+)([smhd])?)?",
        )
        .unwrap()
    });
    if let Some(caps) = re_full.captures(line) {
        let dl_v: f64 = caps.get(1)?.as_str().parse().ok()?;
        let dl_u = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let total_v: f64 = caps.get(3)?.as_str().parse().ok()?;
        let total_u = caps.get(4).map(|m| m.as_str()).unwrap_or("");
        let pct: f32 = caps.get(5)?.as_str().parse().ok()?;
        let speed_v: f64 = caps.get(6)?.as_str().parse().ok()?;
        let speed_u = caps.get(7).map(|m| m.as_str()).unwrap_or("");
        let eta_v: Option<u64> = caps.get(8).and_then(|m| m.as_str().parse().ok());
        let eta_u = caps.get(9).map(|m| m.as_str()).unwrap_or("s");

        let downloaded = (dl_v * unit_mult(dl_u)) as u64;
        let total = (total_v * unit_mult(total_u)) as u64;
        let speed = speed_v * unit_mult(speed_u);
        let eta_sec = eta_v.map(|v| match eta_u {
            "m" => v * 60,
            "h" => v * 3600,
            "d" => v * 86_400,
            _ => v,
        });

        return Some(ProgressSnapshot {
            bytes_downloaded: downloaded,
            bytes_total: Some(total),
            speed_bps: Some(speed),
            eta_sec,
            percent: Some(pct),
        });
    }

    // Fallback form without total / pct (rare, e.g. when content-length unknown).
    let re_no_total = RE_NO_TOTAL.get_or_init(|| {
        Regex::new(r"^\[#[A-Za-z0-9]+\s+([\d.]+)([KMG]i?B)?.*?DL:([\d.]+)([KMG]i?B)?").unwrap()
    });
    if let Some(caps) = re_no_total.captures(line) {
        let dl_v: f64 = caps.get(1)?.as_str().parse().ok()?;
        let dl_u = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let speed_v: f64 = caps.get(3)?.as_str().parse().ok()?;
        let speed_u = caps.get(4).map(|m| m.as_str()).unwrap_or("");
        return Some(ProgressSnapshot {
            bytes_downloaded: (dl_v * unit_mult(dl_u)) as u64,
            bytes_total: None,
            speed_bps: Some(speed_v * unit_mult(speed_u)),
            eta_sec: None,
            percent: None,
        });
    }

    None
}

fn parse_fallback(line: &str) -> Option<ProgressSnapshot> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^\[download\]\s+(\d+(?:\.\d+)?)%\s+of\s+~?\s*([\d.]+)([KMG]i?B)\s+at\s+([\d.]+)([KMG]i?B)/s\s+ETA\s+([\d:]+)").unwrap()
    });
    let caps = re.captures(line.trim())?;
    let pct: f32 = caps.get(1)?.as_str().parse().ok()?;
    let total_v: f64 = caps.get(2)?.as_str().parse().ok()?;
    let total_u = caps.get(3)?.as_str();
    let speed_v: f64 = caps.get(4)?.as_str().parse().ok()?;
    let speed_u = caps.get(5)?.as_str();
    let eta_str = caps.get(6)?.as_str();

    let total_bytes = (total_v * unit_mult(total_u)) as u64;
    let speed_bps = speed_v * unit_mult(speed_u);
    let eta_sec = parse_eta(eta_str);
    let downloaded = ((total_bytes as f64) * (pct as f64) / 100.0) as u64;

    Some(ProgressSnapshot {
        bytes_downloaded: downloaded,
        bytes_total: Some(total_bytes),
        speed_bps: Some(speed_bps),
        eta_sec,
        percent: Some(pct),
    })
}

fn parse_u64(s: &str) -> Option<u64> { s.trim().parse::<u64>().ok().or_else(|| s.trim().parse::<f64>().ok().map(|v| v as u64)) }
fn parse_u64_opt(s: &str) -> Option<u64> { let s = s.trim(); if s.is_empty() || s == "NA" { None } else { parse_u64(s) } }
fn parse_f64_opt(s: &str) -> Option<f64> { let s = s.trim(); if s.is_empty() || s == "NA" { None } else { s.parse().ok() } }

fn unit_mult(u: &str) -> f64 {
    match u {
        "KiB" => 1024.0, "KB" => 1024.0, "K" => 1024.0,
        "MiB" => 1024.0 * 1024.0, "MB" => 1024.0 * 1024.0, "M" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0, "GB" => 1024.0 * 1024.0 * 1024.0, "G" => 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    }
}

fn parse_eta(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split(':').collect();
    let n: Vec<u64> = parts.iter().filter_map(|p| p.parse().ok()).collect();
    if n.len() != parts.len() { return None; }
    match n.len() {
        2 => Some(n[0] * 60 + n[1]),
        3 => Some(n[0] * 3600 + n[1] * 60 + n[2]),
        1 => Some(n[0]),
        _ => None,
    }
}

/// Stage marker: returns true on `[Merger]`, `[ExtractAudio]`, etc.
pub fn is_stage_marker(line: &str) -> bool {
    let l = line.trim_start();
    l.starts_with("[Merger]") || l.starts_with("[ExtractAudio]") || l.starts_with("[FixupM4a]") || l.starts_with("[VideoConvertor]")
}

/// Throttle helper: returns true if at least `min_interval` has elapsed since `last`.
/// Updates `last` to `Instant::now()` when returning true.
pub fn should_emit(last: &mut Option<Instant>, min_interval: Duration) -> bool {
    let now = Instant::now();
    let yes = match last { Some(t) => now.duration_since(*t) >= min_interval, None => true };
    if yes { *last = Some(now); }
    yes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dlprog_basic() {
        let s = parse_progress("DLPROG|512000|1024000|256000.0|2").unwrap();
        assert_eq!(s.bytes_downloaded, 512000);
        assert_eq!(s.bytes_total, Some(1024000));
        assert_eq!(s.speed_bps, Some(256000.0));
        assert_eq!(s.eta_sec, Some(2));
        assert!((s.percent.unwrap() - 50.0).abs() < 0.01);
    }
    #[test]
    fn dlprog_unknown_total() {
        let s = parse_progress("DLPROG|100|NA|5000|NA").unwrap();
        assert_eq!(s.bytes_total, None);
        assert_eq!(s.percent, None);
        assert_eq!(s.eta_sec, None);
    }
    #[test]
    fn fallback_parses_default_line() {
        let s = parse_progress("[download]   1.2% of  120.50MiB at  3.10MiB/s ETA 00:39").unwrap();
        assert!((s.percent.unwrap() - 1.2).abs() < 0.001);
        assert!(s.bytes_total.unwrap() > 0);
    }
    #[test]
    fn returns_none_for_garbage() {
        assert!(parse_progress("[info] downloading webpage").is_none());
        assert!(parse_progress("").is_none());
    }
    #[test]
    fn stage_markers() {
        assert!(is_stage_marker("[Merger] Merging formats..."));
        assert!(!is_stage_marker("[download] something"));
    }

    #[test]
    fn aria2_full_line() {
        let s = parse_progress("[#abc123 12MiB/100MiB(12%) CN:8 DL:5.5MiB ETA:15s]").unwrap();
        assert_eq!(s.percent, Some(12.0));
        assert_eq!(s.bytes_total, Some(100 * 1024 * 1024));
        assert_eq!(s.bytes_downloaded, 12 * 1024 * 1024);
        assert!((s.speed_bps.unwrap() - 5.5 * 1024.0 * 1024.0).abs() < 1.0);
        assert_eq!(s.eta_sec, Some(15));
    }

    #[test]
    fn aria2_eta_minutes() {
        let s = parse_progress("[#xyz 50MiB/200MiB(25%) CN:16 DL:2.5MiB ETA:1m]").unwrap();
        assert_eq!(s.eta_sec, Some(60));
    }
}
