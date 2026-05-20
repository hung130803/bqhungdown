# Implementation Plan: Multi-Platform Video Downloader

## Overview

Triển khai theo chiều từ dưới lên: scaffold dự án Tauri 2 + React 18 + Vite + TypeScript, định nghĩa domain models cho cả TS và Rust, xây các module pure Rust (short_id, args_builder, filename_resolver, progress_parser, retry, FSM) trước khi lắp ráp QueueManager và sidecar adapter, sau đó đến storage (Settings JSON + History SQLite), Tauri commands/events, frontend foundation (routing, IPC wrappers, i18n, theme, stores), tới components, pages, rồi clipboard watcher và notifications. Property tests đi cùng module mà chúng kiểm tra để bắt lỗi sớm. Cuối cùng wire bootstrap backend và frontend, chạy smoke integration test.

## Tasks

- [ ] 1. Bootstrap project scaffold
  - [ ] 1.1 Initialize Tauri 2 + React 18 + Vite + TypeScript scaffold
    - Generate `package.json`, `vite.config.ts`, `tsconfig.json`, `index.html`, `src/main.tsx`, `src/App.tsx`
    - Generate `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json` (skeleton)
    - _Requirements: 14.1, 14.2, 16.4_
    - _Design: Project Structure_

  - [ ] 1.2 Configure sidecars and plugins in `src-tauri/tauri.conf.json`
    - Set `bundle.externalBin` for `binaries/yt-dlp` and `binaries/ffmpeg`
    - Enable plugins: `shell` (sidecar scope for yt-dlp/ffmpeg), `dialog`, `notification`, `sql`
    - _Requirements: 4.2, 5.1, 10.1, 15.1_
    - _Design: Sidecar Binary Setup, tauri.conf.json (trích)_

  - [ ] 1.3 Add Rust dependencies in `src-tauri/Cargo.toml`
    - tokio (full), tokio-util, serde, serde_json, thiserror, blake3, regex, url, indexmap, chrono, sqlx (sqlite), tauri-plugin-shell, tauri-plugin-dialog, tauri-plugin-notification, tauri-plugin-sql; dev-deps: proptest, tokio-test
    - _Requirements: 5.5, 6.1, 13.1, 16.4_
    - _Design: Backend Modules_

  - [ ] 1.4 Add frontend npm dependencies in `package.json`
    - react, react-dom, react-router-dom, zustand, i18next, react-i18next, @tauri-apps/api, @tauri-apps/plugin-shell, @tauri-apps/plugin-dialog, @tauri-apps/plugin-notification, @tauri-apps/plugin-sql; dev-deps: vitest, @testing-library/react, fast-check, jsdom
    - _Requirements: 14.1, 14.5_
    - _Design: Frontend UI, i18n_

  - [ ] 1.5 Create `scripts/fetch-sidecars.mjs`
    - Detect target triple, download yt-dlp/ffmpeg release per platform, name with `<bin>-<triple>[.exe]`, chmod +x on Unix
    - _Requirements: 5.1, 10.4_
    - _Design: scripts/fetch-sidecars.mjs (logic), Sidecar Binary Setup_

  - [ ] 1.6 Create `scripts/verify-sidecars.mjs` and wire `postinstall` in `package.json`
    - SHA256 verify against bundled manifest; fail postinstall on mismatch
    - _Requirements: 10.4_
    - _Design: scripts/fetch-sidecars.mjs (logic)_

- [ ] 2. Domain model definitions
  - [ ] 2.1 Define TypeScript domain types in `src/types/models.ts`
    - `DownloadMode`, `DownloadState`, `QualityFormat`, `SubtitleTrack`, `VideoMetadata`, `PlaylistEntry`, `DownloadRequest`, `DownloadItem`, `HistoryEntry`, `Settings`, `ConflictPolicy`, `ProgressSnapshot`
    - _Requirements: 2.2, 3.1, 6.1, 7.3, 13.1, 16.1_
    - _Design: Data Models — TypeScript_

  - [ ] 2.2 Define Rust domain structs in `src-tauri/src/models.rs`
    - Mirror TS types với `#[serde(rename_all = "camelCase")]`: `DownloadState`, `DownloadMode`, `QualityFormat`, `SubtitleTrack`, `VideoMetadata`, `PlaylistEntry`, `DownloadRequest`, `DownloadItem`, `HistoryEntry`, `Settings`, `ConflictPolicy`, `ProgressSnapshot`, `ProgressEvent`
    - _Requirements: 2.2, 3.1, 6.1, 7.3, 13.1, 16.1_
    - _Design: Data Models — Rust_

  - [ ] 2.3 Define `AppError` enum in `src-tauri/src/error.rs`
    - thiserror + `#[derive(Serialize)]` tagged union (`InvalidUrl`, `UnsupportedSite`, `YtDlpFailed`, `FfmpegMissing`, `SaveFolderUnavailable`, `Timeout`, `IllegalTransition`, `Io`, `ConfigCorrupt`, `InvalidSetting`); `AppResult<T>` alias
    - _Requirements: 1.4, 2.4, 4.5, 10.4, 16.5_
    - _Design: Error Handling_

- [ ] 3. Core domain logic — Rust pure modules
  - [ ] 3.1 Implement Short_ID generator in `src-tauri/src/short_id.rs`
    - `generate(url, ts_ms, taken: &HashSet<String>) -> String` dùng blake3 → base32 truncate, length 6→8 với nonce khi va chạm
    - _Requirements: 6.1, 6.3, 6.4_
    - _Design: Short_ID, Short_ID Generation_

  - [ ]* 3.2 Write proptest for Short_ID generator
    - **Property 9: Short_ID format and uniqueness**
    - **Validates: Requirements 6.1, 6.3, 6.4**
    - File: `src-tauri/src/short_id.rs` (`#[cfg(test)] mod proptests`)
    - _Design: Property 9_

  - [ ] 3.3 Implement URL validator + extractor table
    - `src-tauri/src/url_validator.rs::validate_url(s) -> AppResult<UrlValidation>`
    - `src-tauri/src/extractors.rs::resolve_extractor(url) -> Option<&'static str>` + Featured_Platform map cho 6 site
    - _Requirements: 1.2, 1.3, 1.4, 1.5_
    - _Design: domain::url_validator, extractors.rs_

  - [ ]* 3.4 Write proptest for `resolve_extractor`
    - **Property 2: Host → extractor resolution is total and consistent**
    - **Validates: Requirements 1.3, 1.4**
    - File: `src-tauri/src/extractors.rs` (proptest module)
    - _Design: Property 2_

  - [ ] 3.5 Implement `args_builder::build_argv` in `src-tauri/src/args_builder.rs`
    - Pure: `(item: &DownloadItem, resume: bool, settings: &Settings) -> Vec<String>`; tham số `-o`, `-N 16`, `--newline`, `--no-mtime`, `-f`, `-x --audio-format mp3 --audio-quality 0`, `--write-subs`, `--write-auto-subs`, `--downloader aria2c …`, `--continue`, `--yes-playlist`, `--force-overwrites`, `--retries 0`
    - _Requirements: 5.1, 5.2, 7.1, 7.2, 8.3, 9.5, 10.1, 10.3, 11.2, 11.3, 11.4_
    - _Design: yt-dlp Argument Matrix_

  - [ ]* 3.6 Write proptest for `build_argv` mode invariants
    - **Property 3: yt-dlp argv builder mode invariants**
    - **Validates: Requirements 5.1, 5.2, 7.1, 7.2, 8.3, 9.5, 10.1, 10.3, 11.2, 11.3, 11.4**
    - File: `src-tauri/src/args_builder.rs` (proptest module)
    - _Design: Property 3_

  - [ ] 3.7 Implement `filename_resolver` in `src-tauri/src/filename_resolver.rs`
    - `sanitize(title)`, `auto_rename_suffix(folder, stem, ext, &existing)`, `FilenameResolver::resolve(folder, title, ext, exists_fn, ConflictStrategy) -> ResolveOutcome`
    - _Requirements: 4.5, 7.1, 7.2, 7.3, 7.4, 7.5, 7.6_
    - _Design: filename_resolver.rs, File Naming & Conflict Resolution_

  - [ ]* 3.8 Write proptest for `auto_rename_suffix`
    - **Property 7: Filename auto-rename uses smallest free suffix**
    - **Validates: Requirements 7.1, 7.2, 7.4**
    - File: `src-tauri/src/filename_resolver.rs` (proptest module)
    - _Design: Property 7_

  - [ ]* 3.9 Write proptest for `sanitize`
    - **Property 8: Filename sanitizer preserves valid titles**
    - **Validates: Requirements 7.1, 7.2**
    - File: `src-tauri/src/filename_resolver.rs` (proptest module)
    - _Design: Property 8_

  - [ ] 3.10 Implement `progress_parser` in `src-tauri/src/progress_parser.rs`
    - Regex parse `^[download]\s+(\d+(?:\.\d+)?)% of …`; `parse_progress_line(&str) -> Option<ProgressSnapshot>`; nhận diện `Destination:` / `[Merger]`
    - _Requirements: 5.3_
    - _Design: Progress parser_

  - [ ]* 3.11 Write proptest for `parse_progress_line`
    - **Property 15: Progress line parser is total and structured**
    - **Validates: Requirements 5.3**
    - File: `src-tauri/src/progress_parser.rs` (proptest module)
    - _Design: Property 15_

  - [ ] 3.12 Implement retry policy in `src-tauri/src/retry.rs`
    - `next_delay(attempt: u8) -> Option<Duration>` trả `[2s, 5s, 10s]` rồi `None`; helper classify network error từ stderr
    - _Requirements: 5.5_
    - _Design: Concurrency Control & Retry Policy_

  - [ ]* 3.13 Write proptest for retry schedule
    - **Property 11: Retry schedule**
    - **Validates: Requirements 5.5**
    - File: `src-tauri/src/retry.rs` (proptest module)
    - _Design: Property 11_

  - [ ] 3.14 Implement FSM transition table in `src-tauri/src/queue.rs`
    - `transition(state, event) -> AppResult<DownloadState>` với bảng legal transitions; trả `IllegalTransition` cho mọi case khác
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 13.3_
    - _Design: Queue Manager — State Machine_

  - [ ]* 3.15 Write proptest for FSM transitions
    - **Property 12: FSM legal transitions**
    - **Validates: Requirements 8.1, 8.2, 8.3, 8.4, 8.5**
    - File: `src-tauri/src/queue.rs` (proptest module)
    - _Design: Property 12_

  - [ ] 3.16 Implement `QueueManager` scheduler in `src-tauri/src/queue.rs`
    - Struct với `RwLock<IndexMap>`, `Mutex<HashMap<ChildHandle>>`, `Arc<Semaphore>`, `worker_loop`, methods `enqueue`, `pause`, `resume`, `cancel`, `retry`, `list`, `set_concurrency` (semaphore resize ≤ 2s); gọi `YtDlpRunner` cho từng slot, áp dụng `retry::next_delay` trên transient error, commit history khi terminal
    - _Requirements: 5.4, 5.5, 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 13.1_
    - _Design: Queue_Manager, queue.rs, Concurrency Control_

  - [ ]* 3.17 Write proptest for concurrency cap invariant (with `tokio::time::pause`)
    - **Property 10: Queue concurrency invariant**
    - **Validates: Requirements 5.4, 8.6**
    - File: `src-tauri/tests/queue_concurrency.rs`
    - _Design: Property 10_

- [ ] 4. Sidecar adapter
  - [ ] 4.1 Implement `YtDlpRunner` in `src-tauri/src/ytdlp_runner.rs`
    - sidecar spawn qua `tauri_plugin_shell::process::Command::sidecar("yt-dlp")`; `fetch_metadata(url)` với `--dump-single-json` + `tokio::time::timeout(30s)`; `run_download(item, cancel, progress_tx)` đọc stdout line-buffered và đẩy `ProgressEvent`; capture stderr → `AppError::YtDlpFailed`
    - _Requirements: 2.1, 2.4, 2.5, 5.3, 8.2, 8.4_
    - _Design: ytdlp_runner.rs_

  - [ ] 4.2 Implement Ffmpeg detection in `src-tauri/src/sidecar/ffmpeg.rs`
    - `ensure_available(app: &AppHandle) -> AppResult<()>`; expose flag để command audio trả `FfmpegMissing` khi vắng
    - _Requirements: 10.4_
    - _Design: sidecar::ffmpeg_

  - [ ] 4.3 Implement Aria2c detection in `src-tauri/src/sidecar/aria2c.rs`
    - `is_available() -> bool` qua `which`/`where`; expose cho settings UI cảnh báo khi `aria2c_enabled=true` mà thiếu
    - _Requirements: 5.2_
    - _Design: aria2c (optional)_

- [ ] 5. Storage layer
  - [ ] 5.1 Implement `SettingsStore` (JSON) in `src-tauri/src/settings_store.rs`
    - Load từ `app_config_dir/settings.json`; defaults khi corrupt (`max_concurrency=3`, theme `system`, language `vi`); clamp `max_concurrency ∈ [1,10]`; `update<F>` với debounced persist 500ms; OS Downloads làm `default_folder` lần đầu
    - _Requirements: 4.4, 12.4, 16.2, 16.3, 16.4, 16.5_
    - _Design: settings_store.rs_

  - [ ]* 5.2 Write proptest for settings round-trip + clamping + corrupt recovery
    - **Property 20: Settings persistence round-trip and clamping**
    - **Validates: Requirements 16.2, 16.3, 16.5**
    - File: `src-tauri/src/settings_store.rs` (proptest module)
    - _Design: Property 20_

  - [ ] 5.3 Implement `HistoryStore` schema + open in `src-tauri/src/history_store.rs`
    - `CREATE TABLE history(...)` + index `idx_history_finished_at` và `idx_history_title`; mở SQLite qua sqlx pool tại `app_data_dir/state.db`; idempotent migration
    - _Requirements: 13.1, 13.2_
    - _Design: history_store.rs (schema)_

  - [ ] 5.4 Implement `HistoryStore` CRUD in `src-tauri/src/history_store.rs`
    - `insert(&HistoryEntry)`, `list(query, limit, offset)` sort `finished_at DESC`, case-insensitive search trên `title`/`url`, `delete(short_id)`, `known_short_ids() -> HashSet<String>`
    - _Requirements: 13.1, 13.2, 13.4, 13.5_
    - _Design: history_store.rs_

  - [ ]* 5.5 Write proptest for history persistence + ordering
    - **Property 17: History persistence round-trip and ordering**
    - **Validates: Requirements 13.1, 13.2, 13.5**
    - File: `src-tauri/tests/history_repo.rs`
    - _Design: Property 17_

  - [ ]* 5.6 Write proptest for history search semantics
    - **Property 18: History search semantics**
    - **Validates: Requirements 13.4**
    - File: `src-tauri/tests/history_repo.rs`
    - _Design: Property 18_

- [ ] 6. Tauri commands and events
  - [ ] 6.1 Define event constants and emit helpers in `src-tauri/src/events.rs`
    - String const cho `download://progress|state|conflict|completed|failed`, `clipboard://detected`, `notification://clicked`, `settings://changed`, `queue://updated`; payload structs typed; `emit_progress`, `emit_state_changed`, …
    - _Requirements: 5.3, 7.3, 12.2, 15.1, 15.2, 15.3_
    - _Design: Tauri Events_

  - [ ] 6.2 Implement metadata commands in `src-tauri/src/commands/metadata.rs`
    - `validate_url(url) -> AppResult<UrlValidation>`, `fetch_metadata(url) -> AppResult<VideoMetadata>` với 30s timeout; parse playlist entries từ JSON yt-dlp
    - _Requirements: 1.2, 1.3, 1.4, 2.1, 2.2, 2.4, 2.5_
    - _Design: Tauri Commands (validate_url, fetch_metadata)_

  - [ ]* 6.3 Write proptest for playlist metadata count
    - **Property 4: Playlist metadata count**
    - **Validates: Requirements 2.5, 9.3**
    - File: `src-tauri/src/commands/metadata.rs` (proptest module)
    - _Design: Property 4_

  - [ ] 6.4 Implement enqueue commands in `src-tauri/src/commands/enqueue.rs`
    - `enqueue_download`, `enqueue_batch` (gọi `parse_batch(text) -> Vec<String>`), `enqueue_playlist`, `resolve_conflict`; sinh Short_ID, kiểm tra folder writable trước khi enqueue
    - _Requirements: 4.5, 6.1, 7.3, 9.1, 9.2, 9.3, 9.4, 9.5_
    - _Design: Tauri Commands (enqueue_*)_

  - [ ]* 6.5 Write proptest for batch parser
    - **Property 13: Batch parser yields exactly the valid URLs**
    - **Validates: Requirements 9.1, 9.2**
    - File: `src-tauri/src/commands/enqueue.rs` (proptest module)
    - _Design: Property 13_

  - [ ]* 6.6 Write proptest for playlist selection round-trip
    - **Property 14: Playlist selection round-trip**
    - **Validates: Requirements 9.4**
    - File: `src-tauri/src/commands/enqueue.rs` (proptest module)
    - _Design: Property 14_

  - [ ] 6.7 Implement queue control commands in `src-tauri/src/commands/control.rs`
    - `pause_download`, `resume_download`, `cancel_download`, `retry_download`, `list_queue`; forward về `QueueManager`
    - _Requirements: 8.2, 8.3, 8.4, 8.5_
    - _Design: Tauri Commands_

  - [ ] 6.8 Implement settings commands in `src-tauri/src/commands/settings.rs`
    - `get_settings`, `update_settings(patch)`, `set_clipboard_watcher(enabled)`; merge + clamp + persist + emit `settings://changed`; gọi `set_concurrency` khi đổi max_concurrency
    - _Requirements: 8.6, 12.4, 16.1, 16.3_
    - _Design: Tauri Commands, settings_store.rs_

  - [ ] 6.9 Implement history commands in `src-tauri/src/commands/history.rs`
    - `list_history(query, limit, offset)`, `delete_history_entry(short_id)`, `redownload_from_history(short_id)` tạo `DownloadItem` mới với cùng url/format_id và Short_ID mới
    - _Requirements: 6.3, 13.2, 13.3, 13.4, 13.5_
    - _Design: Tauri Commands_

  - [ ]* 6.10 Write proptest for redownload preserves URL+format
    - **Property 19: Re-download preserves URL and format**
    - **Validates: Requirements 13.3, 6.3**
    - File: `src-tauri/src/commands/history.rs` (proptest module)
    - _Design: Property 19_

  - [ ] 6.11 Implement folder commands in `src-tauri/src/commands/folder.rs`
    - `pick_folder` (dialog), `check_folder_writable(path)` (probe write/remove temp), `open_in_folder(path)`, `open_url(url)`
    - _Requirements: 4.2, 4.5_
    - _Design: Tauri Commands_

  - [ ] 6.12 Implement `app_bootstrap` command in `src-tauri/src/commands/bootstrap.rs`
    - Trả `BootstrapPayload { settings, queue_snapshot, ffmpeg_available, aria2c_available }`
    - _Requirements: 14.4, 16.4_
    - _Design: Tauri Commands (app_bootstrap)_

  - [ ] 6.13 Register all commands and managed state in `src-tauri/src/lib.rs`
    - `tauri::Builder` `.manage(Arc<QueueManager>)`, `.manage(Arc<SettingsStore>)`, `.manage(Arc<HistoryStore>)`, `.manage(Arc<YtDlpRunner>)`; `.invoke_handler(generate_handler![…])` toàn bộ commands; `mod commands;` re-export
    - _Requirements: 16.4_
    - _Design: Backend, lib.rs_

- [ ] 7. Backend checkpoint
  - Ensure all backend tests pass, ask the user if questions arise.

- [ ] 8. Frontend foundation
  - [ ] 8.1 Set up routing and App shell in `src/App.tsx` + `src/main.tsx`
    - `react-router-dom` routes: `/`, `/queue`, `/history`, `/settings`; nav shell với 4 tab tiếng Việt
    - _Requirements: 14.1_
    - _Design: Frontend UI — Pages_

  - [ ] 8.2 Implement IPC command wrappers in `src/ipc/commands.ts`
    - Typed functions cho mọi backend command (validate_url, fetch_metadata, enqueue_*, pause/resume/cancel/retry, get/update_settings, list/delete/redownload history, pick_folder, check_folder_writable, open_*, app_bootstrap, resolve_conflict, set_clipboard_watcher)
    - _Requirements: 1.2, 2.1, 4.2, 5.1, 7.3, 8.2, 13.2, 16.1_
    - _Design: ipc/commands.ts_

  - [ ] 8.3 Implement IPC event listeners in `src/ipc/events.ts`
    - `onDownloadProgress`, `onDownloadStateChanged`, `onDownloadConflict`, `onDownloadCompleted`, `onDownloadFailed`, `onClipboardDetected`, `onNotificationClicked`, `onSettingsChanged`
    - _Requirements: 5.3, 7.3, 12.2, 15.1, 15.2_
    - _Design: ipc/events.ts_

  - [ ] 8.4 Implement i18n setup in `src/i18n/index.ts` + `src/i18n/vi.json` + `src/i18n/en.json`
    - i18next init `lng: "vi"`, fallback `vi`; bundles cho keys: `app.*`, `nav.*`, `home.*`, `queue.*`, `history.*`, `settings.*`, `errors.*`, `notif.*`, `conflict.*`
    - _Requirements: 14.1, 14.5_
    - _Design: Theme & i18n_

  - [ ]* 8.5 Write Vitest test for i18n bundle completeness
    - **Property 21: i18n bundle completeness**
    - **Validates: Requirements 14.1, 14.5**
    - File: `tests/web/i18n.test.ts`
    - _Design: Property 21_

  - [ ] 8.6 Implement theme tokens + provider in `src/styles/theme.css` + `src/components/ThemeProvider.tsx`
    - CSS variables `--bg`, `--fg`, `--accent`, `--surface`, `--border` cho light/dark; provider set `data-theme` trên `<html>`; subscribe `prefers-color-scheme` khi `theme === "system"`; áp dụng trong < 200ms
    - _Requirements: 14.2, 14.3, 14.4_
    - _Design: Theme (CSS Variables)_

  - [ ] 8.7 Implement frontend lib helpers in `src/lib/url.ts` + `src/lib/format.ts`
    - `isValidUrl(s)`, `extractHost(s)`, `parseBatch(text)`; `formatBytes(n)`, `formatEta(sec)`, `formatSpeed(bps)`
    - _Requirements: 1.2, 5.3, 9.1_
    - _Design: src/lib/_

  - [ ]* 8.8 Write fast-check property for `isValidUrl`
    - **Property 1: URL syntactic validation**
    - **Validates: Requirements 1.2**
    - File: `tests/web/url.property.test.ts`
    - _Design: Property 1_

  - [ ] 8.9 Implement `selectBest` in `src/lib/best-format.ts`
    - Pure: chọn `QualityFormat` theo lex order `(height, fps, tbr)`; `selectBestAudio(formats)` trả audio-only có `abr` lớn nhất
    - _Requirements: 3.2, 3.3_
    - _Design: src/lib/best-format.ts_

  - [ ]* 8.10 Write fast-check property for `selectBest` dominance
    - **Property 5: Best-format selector dominates**
    - **Validates: Requirements 3.2**
    - File: `tests/web/best-format.property.test.ts`
    - _Design: Property 5_

  - [ ]* 8.11 Write fast-check property for audio max-bitrate selection
    - **Property 6: Audio-mode selects max bitrate**
    - **Validates: Requirements 3.3**
    - File: `tests/web/best-format.property.test.ts`
    - _Design: Property 6_

  - [ ] 8.12 Implement Zustand stores in `src/stores/`
    - `useQueueStore` (subscribe `download://progress|state`, methods pause/resume/cancel/retry), `useSettingsStore` (hydrate qua `app_bootstrap`, push `update_settings`), `useHistoryStore` (paged + query), `useUrlStore`, `useClipboardStore`
    - _Requirements: 8.1, 13.2, 14.4, 16.4_
    - _Design: Stores (Zustand)_

- [ ] 9. Frontend components
  - [ ] 9.1 Implement `UrlInput` in `src/components/UrlInput.tsx`
    - Debounced 100ms validate (qua `isValidUrl` + `validate_url`), hiển thị lỗi tiếng Việt khi không hỗ trợ
    - _Requirements: 1.1, 1.2, 1.4_
    - _Design: UrlInput.tsx_

  - [ ] 9.2 Implement `PlatformBadge` in `src/components/PlatformBadge.tsx`
    - Map extractor → icon + tên cho 6 Featured_Platform; hiển thị cạnh `UrlInput`
    - _Requirements: 1.5_
    - _Design: PlatformBadge.tsx_

  - [ ] 9.3 Implement `MetadataCard` in `src/components/MetadataCard.tsx`
    - Title, thumbnail, channel, duration; loading + error states (cho retry sau timeout)
    - _Requirements: 2.2, 2.4_
    - _Design: MetadataCard.tsx_

  - [ ] 9.4 Implement `ModeToggle` + `QualityPicker` in `src/components/QualityPicker.tsx`
    - Tabs Video / Audio MP3; list `QualityFormat` (resolution/fps/codec/filesize) với "Best"; vô hiệu Video khi không có format video
    - _Requirements: 2.3, 3.1, 3.2, 3.3, 3.4, 3.5_
    - _Design: ModeToggle.tsx, QualityPicker.tsx_

  - [ ] 9.5 Implement `SubtitlePicker` in `src/components/SubtitlePicker.tsx`
    - Multi-select sub langs; toggle auto-translate target; hiển thị thông báo tiếng Việt khi không có sub
    - _Requirements: 11.1, 11.3, 11.5_
    - _Design: SubtitlePicker.tsx_

  - [ ] 9.6 Implement `FolderPicker` in `src/components/FolderPicker.tsx`
    - Hiển thị `defaultFolder`; nút "Chọn folder" gọi `pick_folder`; cập nhật Settings
    - _Requirements: 4.1, 4.2, 4.3_
    - _Design: FolderPicker.tsx_

  - [ ] 9.7 Implement `ProgressBar` in `src/components/ProgressBar.tsx`
    - Linear bar + %, dùng `formatBytes` / `formatSpeed` / `formatEta`
    - _Requirements: 5.3_
    - _Design: ProgressBar.tsx_

  - [ ] 9.8 Implement `DownloadItemCard` / `QueueRow` in `src/components/QueueRow.tsx`
    - Short_ID badge `#xxxxxxx`, title, `ProgressBar`, action buttons pause/resume/cancel/retry tuỳ state
    - _Requirements: 6.2, 8.1, 8.2, 8.3, 8.4, 8.5_
    - _Design: QueueRow.tsx, DownloadItemCard_

  - [ ] 9.9 Implement `ConflictDialog` in `src/components/ConflictDialog.tsx`
    - 3 nút: "Ghi đè", "Bỏ qua", "Tự động đổi tên"; gọi `resolve_conflict` với choice tương ứng
    - _Requirements: 7.3, 7.4, 7.5, 7.6_
    - _Design: ConflictDialog.tsx_

  - [ ] 9.10 Implement `BatchInput` + `PlaylistEntries` in `src/components/PlaylistEntries.tsx`
    - Textarea nhiều URL phân cách dòng mới; checkbox list cho playlist entry + "Chọn tất cả"
    - _Requirements: 9.1, 9.3, 9.4_
    - _Design: BatchInput.tsx, PlaylistEntryList.tsx_

  - [ ] 9.11 Implement `ClipboardBanner` in `src/components/ClipboardBanner.tsx`
    - Listen `clipboard://detected`, dedupe dismissed locally; nút "Tải nhanh"
    - _Requirements: 12.2, 12.3_
    - _Design: ClipboardBanner.tsx_

- [ ] 10. Frontend pages
  - [ ] 10.1 Implement `HomePage` in `src/pages/HomePage.tsx`
    - Compose `UrlInput` + `PlatformBadge` + `MetadataCard` + `QualityPicker` + `SubtitlePicker` + `FolderPicker` + `BatchInput` + `PlaylistEntries` + nút "Tải"; trigger `enqueue_*` commands
    - _Requirements: 1.1, 2.2, 3.1, 4.1, 9.1, 9.3, 11.1_
    - _Design: HomePage / PasteUrlPage_

  - [ ] 10.2 Implement `QueuePage` in `src/pages/QueuePage.tsx`
    - Live list `useQueueStore`, render `QueueRow`, `ConflictDialog` khi có `download://conflict`; focus item theo query param `?focus=<shortId>`
    - _Requirements: 5.3, 7.3, 8.1, 8.2, 8.3, 8.4, 8.5, 15.2_
    - _Design: QueuePage_

  - [ ] 10.3 Implement `HistoryPage` in `src/pages/HistoryPage.tsx`
    - Search input (lọc title/url), table sắp xếp `finished_at DESC`, action "Tải lại" / "Xoá" / "Mở folder"
    - _Requirements: 13.2, 13.3, 13.4, 13.5_
    - _Design: HistoryPage_

  - [ ] 10.4 Implement `SettingsPage` in `src/pages/SettingsPage.tsx`
    - Form: max concurrency (slider/number 1-10), defaultFolder, theme (light/dark/system), language (vi/en), toggles clipboard watcher, notifications, aria2c; gọi `update_settings`; cảnh báo khi aria2c bật mà không có binary
    - _Requirements: 5.2, 12.4, 14.2, 14.5, 15.4, 16.1, 16.2, 16.3_
    - _Design: SettingsPage_

- [ ] 11. Clipboard watcher
  - [ ] 11.1 Implement `ClipboardWatcher` in `src-tauri/src/clipboard.rs`
    - Tokio task poll mỗi 1000ms khi `settings.clipboard_watcher`; dedupe URL trong 60s; chỉ emit khi `resolve_extractor` trả `Some`; emit `clipboard://detected`
    - _Requirements: 12.1, 12.2, 12.4, 12.5_
    - _Design: clipboard.rs, Clipboard Watcher_

  - [ ]* 11.2 Write proptest for clipboard dedup window
    - **Property 16: Clipboard dedup window**
    - **Validates: Requirements 12.1, 12.5**
    - File: `src-tauri/src/clipboard.rs` (proptest module dùng virtual time)
    - _Design: Property 16_

  - [ ] 11.3 Wire `ClipboardBanner` to fill `UrlInput` on "Tải nhanh"
    - Khi click, push URL vào `useUrlStore` và trigger `fetch_metadata`; modify `src/components/ClipboardBanner.tsx` thêm callback + `src/stores/useUrlStore.ts`
    - _Requirements: 12.2, 12.3_
    - _Design: ClipboardBanner.tsx_

- [ ] 12. Notifications
  - [ ] 12.1 Implement notification dispatch in `src-tauri/src/notification.rs`
    - `notify_completed(app, settings, item)` và `notify_failed(app, settings, item, reason)`; gate `settings.notifications`; click handler emit `notification://clicked` với `short_id`
    - _Requirements: 15.1, 15.3, 15.4_
    - _Design: notification.rs, Notifications_

  - [ ]* 12.2 Write proptest for notification gating
    - **Property 22: Notification gating**
    - **Validates: Requirements 15.1, 15.3, 15.4**
    - File: `src-tauri/src/notification.rs` (proptest module)
    - _Design: Property 22_

  - [ ] 12.3 Wire frontend notification click → focus DownloadItem
    - Listen `notification://clicked` ở `src/hooks/useNotificationFocus.ts`, navigate `/queue?focus=<shortId>`, scrollIntoView
    - _Requirements: 15.2_
    - _Design: Notifications_

- [ ] 13. Final integration and smoke
  - [ ] 13.1 Wire bootstrap flow in `src-tauri/src/lib.rs`
    - `setup`: load `SettingsStore` → mở `HistoryStore` → khởi tạo `QueueManager` với semaphore theo settings → spawn `ClipboardWatcher` → ensure ffmpeg/aria2c flags; expose state qua `app_bootstrap`
    - _Requirements: 4.4, 14.4, 16.4, 16.5_
    - _Design: Backend, app_bootstrap_

  - [ ] 13.2 Wire frontend bootstrap in `src/App.tsx`
    - On mount: invoke `app_bootstrap` → hydrate `useSettingsStore` → apply theme + i18n trước khi render shell; subscribe `notification://clicked` → focus
    - _Requirements: 14.4, 15.2, 16.4_
    - _Design: Frontend Foundation_

  - [ ]* 13.3 Write Vitest integration test for paste → metadata → start → progress flow (mocked sidecar)
    - End-to-end happy path qua stores, components, mocked IPC
    - File: `tests/web/integration/paste-to-progress.test.tsx`
    - _Requirements: 1.1, 2.1, 5.3_
    - _Design: Testing Strategy_

- [ ] 14. Final checkpoint
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Sub-tasks gắn `*` là property tests / unit tests tùy chọn — có thể bỏ để rút ngắn MVP, nhưng nên chạy trước khi release.
- Mỗi property test reference rõ Property number và requirement clauses từ design.
- Backend (Tauri 2 + Rust + sidecar) hoàn tất ở task 7 trước khi gắn frontend.
- File `commands/*.rs` được tách thành các module nhỏ (metadata, enqueue, control, settings, history, folder, bootstrap) để cho phép song song hoá; `commands.rs` là `mod` re-export.
- Queue persistence không yêu cầu — restart App ⇒ queue rỗng (chỉ history persist).

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "1.4", "1.5", "2.1", "2.2", "2.3"] },
    { "id": 2, "tasks": ["1.6", "3.1", "3.3", "3.5", "3.7", "3.10", "3.12", "3.14", "4.2", "4.3", "5.1", "5.3", "6.1", "8.1", "8.2", "8.3", "8.4", "8.6", "8.7", "8.9"] },
    { "id": 3, "tasks": ["3.2", "3.4", "3.6", "3.8", "3.9", "3.11", "3.13", "3.15", "4.1", "5.2", "5.4", "8.5", "8.8", "8.10", "8.11", "8.12", "11.1"] },
    { "id": 4, "tasks": ["3.16", "5.5", "5.6", "6.2", "9.1", "9.2", "9.3", "9.4", "9.5", "9.6", "9.7", "9.8", "9.9", "9.10", "9.11", "11.2"] },
    { "id": 5, "tasks": ["3.17", "6.3", "6.4", "6.7", "6.8", "6.9", "6.11", "6.12", "10.1", "10.2", "10.3", "10.4", "11.3", "12.1"] },
    { "id": 6, "tasks": ["6.5", "6.6", "6.10", "6.13", "12.2", "12.3"] },
    { "id": 7, "tasks": ["13.1"] },
    { "id": 8, "tasks": ["13.2", "13.3"] }
  ]
}
```
