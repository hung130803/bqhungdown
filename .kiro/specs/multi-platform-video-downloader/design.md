# Design Document
# npm run tauri:dev
## Overview

Ứng dụng `multi-platform-video-downloader` là một desktop app dạng Tauri 2 shell, với React 18 + TypeScript chạy trong webview làm Frontend và một Rust backend xử lý IPC, sidecar processes (yt-dlp, ffmpeg, aria2c) và filesystem. Backend đóng vai trò orchestrator: nhận lệnh từ Frontend qua IPC, build argument vector cho YtDlp_Sidecar, parse stdout để phát progress events, và quản lý Queue_Manager với concurrency. Frontend chịu trách nhiệm UI tiếng Việt (react-i18next), dark/light theme qua CSS variables, và quy trình Paste → Metadata → Quality → Folder → Download.

## Architecture

### Component Diagram

```
┌────────────────────────────────────────────────────────────────────┐
│                     React 18 + TypeScript (Webview)                │
│                                                                    │
│  Pages:  Home  Queue  History  Settings                            │
│                                                                    │
│  Components: UrlInput · QualitySelector · FolderPicker             │
│              DownloadItemCard · ProgressBar · ConflictDialog       │
│              ClipboardBanner · PlatformIcon · PlaylistEntries      │
│                                                                    │
│  State (Zustand): queueStore · historyStore · settingsStore        │
│  i18n: react-i18next (vi default, en optional)                     │
│  Theme: CSS variables, prefers-color-scheme fallback               │
└─────────────────────────────┬──────────────────────────────────────┘
                              │ Tauri IPC (invoke / event)
┌─────────────────────────────▼──────────────────────────────────────┐
│                       Rust Backend (Tauri 2)                       │
│                                                                    │
│  Command layer  ──▶  Domain layer (Queue_Manager,                  │
│  (commands.rs)         ShortId, ArgsBuilder, Conflict, History)    │
│        │                       │                                   │
│        │                       ▼                                   │
│        │              ┌──────────────────┐                         │
│        │              │ Sidecar Adapter  │                         │
│        │              │  - YtDlp         │                         │
│        │              │  - Ffmpeg        │                         │
│        │              │  - Aria2c (opt)  │                         │
│        │              └────────┬─────────┘                         │
│        │                       │ stdout/stderr (line-buffered)     │
│        │                       ▼                                   │
│  Storage layer   ──▶  SQLite (tauri-plugin-sql) via repositories   │
│  (sqlx)              history · settings · queue (in-memory mirror) │
│                                                                    │
│  Tokio runtime: per-download task + Semaphore(max_concurrency)     │
│  Event bus: emit("download_progress" | "download_state_changed"    │
│             | "clipboard_url_detected" | "file_conflict")          │
└─────────────────────────────┬──────────────────────────────────────┘
                              │ spawn (tauri-plugin-shell sidecar)
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
        ┌─────────┐     ┌─────────┐     ┌─────────┐
        │ yt-dlp  │     │ ffmpeg  │     │ aria2c  │
        └─────────┘     └─────────┘     └─────────┘
```

### Data Flow: Paste → Metadata → Quality → Folder → Download

1. **Paste**: User dán URL → `UrlInput` validate cú pháp client-side (debounced 100ms) → `invoke("validate_url", { url })` → backend resolve host vs extractor list.
2. **Metadata**: Frontend `invoke("fetch_metadata", { url })` → backend spawn `yt-dlp --dump-single-json --no-playlist <url>` (hoặc `--flat-playlist` nếu là playlist) → trả `VideoMetadata` về Frontend.
3. **Quality**: User chọn mode (Video / Audio-only) và Quality_Format từ `QualitySelector`. State giữ trên frontend cho đến khi commit.
4. **Folder**: `FolderPicker` show Default_Folder, nút "Chọn folder" gọi `invoke("choose_folder")` (Tauri dialog plugin). Khi user xác nhận → `invoke("update_settings", { defaultFolder })`.
5. **Download**: `invoke("start_download", { request })` → backend tạo `DownloadItem`, sinh Short_ID, kiểm tra file conflict trước khi tải, đẩy vào Queue_Manager. Tokio task chạy `yt-dlp` với args đã build, parse progress, emit events.

## Project Structure

```
prodowwn/
├── src/                              # React frontend
│   ├── main.tsx
│   ├── App.tsx
│   ├── pages/
│   │   ├── HomePage.tsx
│   │   ├── QueuePage.tsx
│   │   ├── HistoryPage.tsx
│   │   └── SettingsPage.tsx
│   ├── components/
│   │   ├── UrlInput.tsx
│   │   ├── QualitySelector.tsx
│   │   ├── FolderPicker.tsx
│   │   ├── DownloadItemCard.tsx
│   │   ├── ProgressBar.tsx
│   │   ├── ConflictDialog.tsx
│   │   ├── ClipboardBanner.tsx
│   │   ├── PlatformIcon.tsx
│   │   └── PlaylistEntries.tsx
│   ├── stores/                       # Zustand stores
│   │   ├── queueStore.ts
│   │   ├── historyStore.ts
│   │   └── settingsStore.ts
│   ├── ipc/                          # Typed wrappers around invoke/event
│   │   ├── commands.ts
│   │   └── events.ts
│   ├── i18n/
│   │   ├── index.ts
│   │   ├── vi.json                   # default
│   │   └── en.json
│   ├── theme/
│   │   └── tokens.css                # CSS variables (light + dark)
│   └── types/
│       └── domain.ts                 # Mirror of Rust structs
│
├── src-tauri/                        # Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── binaries/                     # Bundled sidecars
│   │   ├── yt-dlp-x86_64-pc-windows-msvc.exe
│   │   ├── ffmpeg-x86_64-pc-windows-msvc.exe
│   │   └── aria2c-x86_64-pc-windows-msvc.exe
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── commands.rs               # #[tauri::command] handlers
│   │   ├── events.rs                 # Event payload types + emit helpers
│   │   ├── domain/
│   │   │   ├── mod.rs
│   │   │   ├── download_item.rs
│   │   │   ├── short_id.rs
│   │   │   ├── queue_manager.rs
│   │   │   ├── args_builder.rs       # yt-dlp arg vector
│   │   │   ├── conflict.rs           # filename conflict resolution
│   │   │   ├── progress.rs           # stdout parser
│   │   │   ├── retry.rs              # backoff schedule
│   │   │   └── url_validator.rs
│   │   ├── sidecar/
│   │   │   ├── mod.rs
│   │   │   ├── ytdlp.rs
│   │   │   ├── ffmpeg.rs
│   │   │   └── aria2c.rs
│   │   ├── storage/
│   │   │   ├── mod.rs
│   │   │   ├── settings_repo.rs
│   │   │   └── history_repo.rs
│   │   ├── error.rs                  # AppError + From impls
│   │   └── i18n.rs                   # error → vi message map
│   └── migrations/
│       ├── 0001_init.sql
│       └── 0002_history.sql
│
├── tests/                            # Frontend Vitest + Playwright e2e
│   ├── unit/
│   ├── property/                     # fast-check property tests
│   └── e2e/
└── package.json
```

## Data Models

### TypeScript (`src/types/domain.ts`)

```typescript
export type DownloadMode = "video" | "audio";

export type DownloadState =
  | "queued"
  | "downloading"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled"
  | "skipped";

export interface QualityFormat {
  formatId: string;
  ext: string;
  resolution: string | null;   // "1920x1080"
  fps: number | null;
  vcodec: string | null;
  acodec: string | null;
  abr: number | null;          // audio bitrate kbps
  vbr: number | null;
  filesize: number | null;     // bytes
  isAudioOnly: boolean;
  isVideoOnly: boolean;
}

export interface SubtitleTrack {
  langCode: string;            // "vi", "en"
  langName: string;
  isAuto: boolean;
}

export interface VideoMetadata {
  url: string;
  extractor: string;
  title: string;
  channel: string | null;
  thumbnail: string | null;
  durationSec: number | null;
  formats: QualityFormat[];
  subtitles: SubtitleTrack[];
  playlistEntries: PlaylistEntry[] | null;
  playlistTotal: number | null;
}

export interface PlaylistEntry {
  url: string;
  title: string;
  durationSec: number | null;
}

export interface DownloadRequest {
  url: string;
  mode: DownloadMode;
  formatId: string | null;     // null ⇒ "best" (bv*+ba/b)
  saveFolder: string;
  subLangs: string[];
  autoTranslateTo: string | null;
  onConflict: "ask" | "overwrite" | "skip" | "rename";
}

export interface DownloadItem {
  shortId: string;             // 7 chars base62
  request: DownloadRequest;
  title: string;
  extractor: string;
  state: DownloadState;
  bytesDownloaded: number;
  bytesTotal: number | null;
  speedBps: number | null;
  etaSec: number | null;
  attempt: number;             // 0-based retry counter
  errorMessage: string | null;
  createdAt: string;           // ISO8601
  finishedAt: string | null;
}

export interface HistoryEntry {
  shortId: string;
  url: string;
  title: string;
  extractor: string;
  formatId: string | null;
  saveFolder: string;
  filePath: string | null;
  state: "completed" | "failed" | "cancelled";
  errorMessage: string | null;
  finishedAt: string;
}

export interface Settings {
  defaultFolder: string;
  maxConcurrency: number;      // 1..=10, default 3
  theme: "light" | "dark" | "system";
  language: "vi" | "en";
  clipboardWatcher: boolean;
  notifications: boolean;
  aria2cEnabled: boolean;
}
```

### Rust (`src-tauri/src/domain/`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DownloadState {
    Queued, Downloading, Paused, Completed, Failed, Cancelled, Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityFormat {
    pub format_id: String,
    pub ext: String,
    pub resolution: Option<String>,
    pub fps: Option<f32>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub abr: Option<f32>,
    pub vbr: Option<f32>,
    pub filesize: Option<u64>,
    pub is_audio_only: bool,
    pub is_video_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub url: String,
    pub mode: DownloadMode,
    pub format_id: Option<String>,
    pub save_folder: PathBuf,
    pub sub_langs: Vec<String>,
    pub auto_translate_to: Option<String>,
    pub on_conflict: ConflictPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub short_id: String,
    pub request: DownloadRequest,
    pub title: String,
    pub extractor: String,
    pub state: DownloadState,
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
    pub speed_bps: Option<f64>,
    pub eta_sec: Option<u64>,
    pub attempt: u8,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub default_folder: PathBuf,
    pub max_concurrency: u8,         // 1..=10
    pub theme: Theme,
    pub language: Language,
    pub clipboard_watcher: bool,
    pub notifications: bool,
    pub aria2c_enabled: bool,
}
```

## Components and Interfaces

This section enumerates the responsibilities and public interfaces of each component. Detailed signatures appear in the data-model and IPC sections below.

### Backend (Rust)

| Component | Responsibility | Key Interface |
|---|---|---|
| `commands` | Tauri IPC entry points | `#[tauri::command] fn validate_url / fetch_metadata / start_download / pause_download / resume_download / cancel_download / retry_download / list_extractors / get_settings / update_settings / list_queue / list_history / delete_history / choose_folder / resolve_conflict / get_subtitle_langs` |
| `domain::url_validator` | URL syntax + extractor host match | `validate_url(s: &str) -> AppResult<UrlValidation>`, `resolve_extractor(url: &str) -> Option<&'static str>` |
| `domain::short_id` | Short_ID generation + uniqueness | `generate_short_id(url, ts_ms, &taken) -> String` |
| `domain::args_builder` | Build yt-dlp argument vector from `DownloadRequest` + `Settings` | `build(req, settings, mode_hint) -> Vec<String>` |
| `domain::queue_manager` | State machine, semaphore, child handles | `enqueue / pause / resume / cancel / retry / set_max_concurrency`, `transition(state, event) -> AppResult<DownloadState>` |
| `domain::conflict` | Filename conflict detection + auto-rename | `auto_rename(stem, ext, &existing) -> String` |
| `domain::progress` | Parse `--progress-template` stdout | `parse_progress(line: &str) -> Option<ProgressUpdate>` |
| `domain::retry` | Retry schedule (2s/5s/10s) | `next_delay(attempt: u8) -> Option<Duration>` |
| `sidecar::ytdlp` | Spawn yt-dlp with built args, stream stdout/stderr | `run(args, on_progress, cancel_token) -> AppResult<ExitInfo>` |
| `sidecar::ffmpeg` | Detect availability, used implicitly via yt-dlp `-x` | `ensure_available() -> AppResult<()>` |
| `sidecar::aria2c` | Detect availability for opt-in path | `is_available() -> bool` |
| `storage::settings_repo` | Load/persist `Settings` JSON blob in SQLite | `load() -> AppResult<Settings>`, `save(&Settings) -> AppResult<()>` |
| `storage::history_repo` | CRUD on `HistoryEntry` | `insert`, `list(query)`, `delete(short_id)` |
| `events` | Strongly-typed event payloads + emit helpers | `emit_progress`, `emit_state_changed`, `emit_clipboard_url`, `emit_file_conflict` |
| `error` | `AppError` enum + serde representation | `AppResult<T> = Result<T, AppError>` |

### Frontend (React)

| Component | Responsibility | Key Props / API |
|---|---|---|
| `pages/HomePage` | Orchestrate paste → metadata → quality → folder → start | composes UrlInput, metadata view, QualitySelector, FolderPicker |
| `pages/QueuePage` | Live queue with progress + actions | subscribes to `download_progress` / `download_state_changed` |
| `pages/HistoryPage` | Filterable history with re-download/delete | calls `list_history`, `retry_download`, `delete_history` |
| `pages/SettingsPage` | Edit `Settings`, validate concurrency 1..=10 | calls `get_settings`, `update_settings` |
| `components/UrlInput` | Validate cú pháp + show platform icon | `props: { value, onChange, onValidated }` |
| `components/QualitySelector` | Mode tabs + format list | `props: { metadata, value, onChange }` |
| `components/FolderPicker` | Show + change folder | `props: { value, onChange }` |
| `components/DownloadItemCard` | Title + Short_ID badge + progress + actions | `props: { item: DownloadItem }` |
| `components/ProgressBar` | Render `bytes_downloaded / bytes_total` | `props: { downloaded, total, speed, eta }` |
| `components/ConflictDialog` | 3-choice modal | `props: { item, suggestedRename, onChoose }` |
| `components/ClipboardBanner` | Top toast for detected URL | listens `clipboard_url_detected`, dedupe on frontend |
| `components/PlatformIcon` | Map host → icon | `props: { extractor }` |
| `components/PlaylistEntries` | Multi-select playlist entries | `props: { entries, onSubmit }` |
| `stores/queueStore` | Mirror backend queue state | `subscribe()`, `byShortId(id)` |
| `stores/historyStore` | History list + search | `load()`, `setQuery(q)` |
| `stores/settingsStore` | Hydrated `Settings` | `update(patch)` |
| `ipc/commands` | Typed wrappers over `invoke` | one function per backend command |
| `ipc/events` | Typed listeners | `onDownloadProgress`, `onDownloadStateChanged`, `onClipboardUrl`, `onFileConflict` |
| `i18n` | react-i18next config + bundles | `t(key, params)` |
| `theme` | CSS variables + `data-theme` toggle | `applyTheme(theme)` |

## Tauri IPC

### Commands (`#[tauri::command]`)

| Command | Args | Returns | Notes |
|---|---|---|---|
| `validate_url` | `url: String` | `Result<UrlValidation>` | Cú pháp + extractor match |
| `fetch_metadata` | `url: String` | `Result<VideoMetadata>` | yt-dlp `--dump-single-json` |
| `start_download` | `request: DownloadRequest` | `Result<String>` (Short_ID) | Sinh Short_ID, kiểm tra conflict, enqueue |
| `pause_download` | `shortId: String` | `Result<()>` | SIGTERM yt-dlp child |
| `resume_download` | `shortId: String` | `Result<()>` | Re-spawn với `--continue` |
| `cancel_download` | `shortId: String` | `Result<()>` | Kill child, transition cancelled |
| `retry_download` | `shortId: String` | `Result<String>` | Tạo item mới, return Short_ID mới |
| `list_extractors` | – | `Result<Vec<ExtractorInfo>>` | Cache 24h |
| `get_settings` | – | `Result<Settings>` | |
| `update_settings` | `patch: SettingsPatch` | `Result<Settings>` | Persist trong 500ms |
| `list_queue` | – | `Result<Vec<DownloadItem>>` | Snapshot |
| `list_history` | `query: HistoryQuery` | `Result<Vec<HistoryEntry>>` | Filter title/url, sort desc |
| `delete_history` | `shortId: String` | `Result<()>` | |
| `choose_folder` | – | `Result<Option<PathBuf>>` | Tauri dialog |
| `resolve_conflict` | `shortId: String, choice: ConflictChoice` | `Result<()>` | Frontend respond to event |
| `get_subtitle_langs` | `url: String` | `Result<Vec<SubtitleTrack>>` | Subset of metadata |

### Events (backend → frontend)

| Event | Payload | When |
|---|---|---|
| `download_progress` | `{ shortId, bytesDownloaded, bytesTotal, speedBps, etaSec }` | Mỗi ≤500ms khi downloading |
| `download_state_changed` | `{ shortId, state, errorMessage? }` | Khi state chuyển đổi |
| `clipboard_url_detected` | `{ url, extractor }` | Watcher tick phát hiện URL mới |
| `file_conflict` | `{ shortId, targetPath, suggestedRename }` | Trước khi yt-dlp ghi file |
| `notification_clicked` | `{ shortId }` | OS notification click → focus item |

## Queue Manager

### State Machine

```
            ┌────────┐
   enqueue  │ queued │
   ───────▶ └───┬────┘
                │ start (slot available)
                ▼
          ┌────────────┐  pause   ┌────────┐
          │downloading ├─────────▶│ paused │
          └─┬───┬────┬─┘          └───┬────┘
            │   │    │ cancel         │ resume
   complete │   │    └──────────┐     ▼
            │   │ fail          │ ┌────────────┐
            ▼   ▼               │ │downloading │
      ┌─────────┐ ┌────────┐    │ └────────────┘
      │completed│ │ failed │    ▼
      └─────────┘ └────┬───┘ ┌──────────┐
                       │     │cancelled │
                  retry│     └──────────┘
                       ▼            │ retry
                   ┌────────┐       │
                   │ queued │◀──────┘
                   └────────┘
   skip (from conflict resolve while queued/downloading) → skipped
```

Legal transitions encoded in `QueueManager::transition(state, event) -> Result<DownloadState>`. Bất kỳ chuyển đổi nào không có trong bảng đều trả `AppError::IllegalTransition`.

### Concurrency Control

```rust
pub struct QueueManager {
    items: Arc<RwLock<HashMap<String, DownloadItem>>>,
    semaphore: Arc<Semaphore>,        // initialized to max_concurrency
    children: Arc<RwLock<HashMap<String, ChildHandle>>>,
    app_handle: AppHandle,
}

impl QueueManager {
    pub async fn enqueue(&self, item: DownloadItem) {
        self.items.write().await.insert(item.short_id.clone(), item);
        self.try_start_next().await;
    }

    async fn run(&self, short_id: String) {
        let permit = self.semaphore.clone().acquire_owned().await.unwrap();
        // permit dropped on task end ⇒ slot freed
        let result = self.run_ytdlp(&short_id).await;
        drop(permit);
        self.try_start_next().await;
        ...
    }

    pub async fn set_max_concurrency(&self, n: u8) {
        // Tăng: add_permits; Giảm: forget_permits trên các slot rảnh, áp dụng dần ≤2s
    }
}
```

Semaphore đảm bảo invariant `count(downloading) ≤ max_concurrency` tại mọi thời điểm.

## yt-dlp Invocation

### Base Args

```
yt-dlp
  --no-warnings
  --newline                            # one progress line per update
  --progress-template "DLPROG|%(progress.downloaded_bytes)d|%(progress.total_bytes)d|%(progress.speed)f|%(progress.eta)d"
  -N 16                                # multi-connection default
  -o "%(title)s.%(ext)s"               # filename giữ nguyên gốc
  --no-mtime
  --paths "home:<save_folder>"
  <url>
```

### Format Selection

| Mode | Format arg | Notes |
|---|---|---|
| Video + "Best" | `-f "bv*+ba/b"` | Best video + best audio, fallback merged best |
| Video + format_id | `-f "<format_id>+ba/b"` if video-only, else `-f "<format_id>"` | |
| Audio-only | `-x --audio-format mp3 --audio-quality 0` | yt-dlp gọi ffmpeg để chuyển MP3 |

### Aria2c (when `Settings.aria2c_enabled`)

```
--downloader aria2c
--downloader-args "aria2c:-x 16 -s 16 -k 1M --file-allocation=none"
```

### Subtitles

```
# When sub_langs not empty:
--write-subs --sub-langs <comma-separated> --convert-subs srt
# When auto_translate_to is Some:
--write-auto-subs --sub-langs <target_lang>
```

### Resume

Khi resume một paused item, append `--continue --no-overwrites` và spawn lại với cùng args + cùng output path.

### Progress Parsing

```rust
// Stdout line format from --progress-template
// "DLPROG|<downloaded>|<total>|<speed>|<eta>"
fn parse_progress(line: &str) -> Option<ProgressUpdate> {
    let rest = line.strip_prefix("DLPROG|")?;
    let mut parts = rest.split('|');
    Some(ProgressUpdate {
        bytes_downloaded: parts.next()?.parse().ok()?,
        bytes_total: parts.next()?.parse().ok(),
        speed_bps: parts.next()?.parse().ok(),
        eta_sec: parts.next()?.parse().ok(),
    })
}
```

Throttle emit `download_progress` events to one per ≥500ms per item bằng `Instant::now() - last_emit >= 500ms`.

## Filename Handling

1. Backend gọi `yt-dlp --simulate -O "%(title)s.%(ext)s"` (hoặc tận dụng metadata đã fetch) để lấy filename dự kiến **trước** khi tải.
2. Resolve absolute path = `save_folder.join(filename)`.
3. Nếu file tồn tại:
   - `request.on_conflict == "ask"` ⇒ emit `file_conflict` event, await `resolve_conflict` command.
   - `"overwrite"` ⇒ tiếp tục, yt-dlp `--force-overwrites`.
   - `"skip"` ⇒ transition skipped, finish.
   - `"rename"` ⇒ tính `auto_rename(stem, ext, existing_set)` với suffix ` (n)` n nhỏ nhất ≥1 sao cho không trùng, override `-o` template thành tên mới (giữ định dạng `%(ext)s`).
4. Filename luôn bắt nguồn từ `%(title)s.%(ext)s` — không có code path nào prepend timestamp/short_id/prefix vào filename trên đĩa. Short_ID chỉ là metadata UI.

```rust
pub fn auto_rename(stem: &str, ext: &str, existing: &HashSet<String>) -> String {
    if !existing.contains(&format!("{stem}.{ext}")) { return format!("{stem}.{ext}"); }
    for n in 1u32.. {
        let candidate = format!("{stem} ({n}).{ext}");
        if !existing.contains(&candidate) { return candidate; }
    }
    unreachable!()
}
```

## Short_ID

```rust
const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

pub fn generate_short_id(url: &str, ts_ms: i64, taken: &HashSet<String>) -> String {
    let mut salt: u64 = 0;
    loop {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        hasher.update(&ts_ms.to_be_bytes());
        hasher.update(&salt.to_be_bytes());
        let digest = hasher.finalize();
        let id = base62_encode(&digest[..6])  // 6 bytes ≈ 8 base62 chars
            .chars().take(7).collect::<String>();
        if !taken.contains(&id) { return id; }
        salt += 1;
    }
}
```

- 7 ký tự base62 ⇒ ~3.5×10¹² không gian, đủ cho queue + history.
- `taken` = union(short_ids đang trong queue, short_ids trong history).

## Frontend UI

### Pages

| Page | Route | Mục đích |
|---|---|---|
| `HomePage` | `/` | Paste URL, fetch metadata, chọn quality/folder, start download |
| `QueuePage` | `/queue` | Danh sách Download_Item active với progress/pause/resume/cancel |
| `HistoryPage` | `/history` | Lọc và tải lại các mục đã hoàn tất |
| `SettingsPage` | `/settings` | concurrency, default folder, theme, language, toggles |

### Key Components

- **UrlInput**: debounced validate, hiển thị `PlatformIcon` cho 6 Featured_Platform.
- **QualitySelector**: tabs Video / Audio-only, list `QualityFormat` với "Best" mặc định.
- **FolderPicker**: hiển thị `Settings.defaultFolder`, nút mở dialog.
- **DownloadItemCard**: title + Short_ID badge nhỏ (`#aB3xK9p`), progress bar, speed, ETA, actions.
- **ProgressBar**: linear, hiển thị % + tốc độ + ETA.
- **ConflictDialog**: 3 lựa chọn — "Ghi đè", "Bỏ qua", "Tự động đổi tên".
- **ClipboardBanner**: top-banner toast với URL + nút "Tải nhanh", dismiss.
- **PlaylistEntries**: checkbox list, "Chọn tất cả".

### Stores (Zustand)

- `queueStore`: `items: DownloadItem[]`, subscribe `download_progress` & `download_state_changed`.
- `historyStore`: lazy load qua `list_history`, hỗ trợ search.
- `settingsStore`: hydrate từ `get_settings` lúc bootstrap, push qua `update_settings`.

## Theme & i18n

### Theme (CSS Variables)

```css
:root[data-theme="light"] {
  --bg: #ffffff;
  --fg: #1a1a1a;
  --accent: #2563eb;
  --surface: #f4f4f5;
  --border: #e4e4e7;
}
:root[data-theme="dark"] {
  --bg: #0b0b0e;
  --fg: #f4f4f5;
  --accent: #60a5fa;
  --surface: #18181b;
  --border: #27272a;
}
```

`<html data-theme="...">` được set bởi `settingsStore`. Khi `theme === "system"` thì subscribe `prefers-color-scheme`.

### i18n

```ts
// src/i18n/index.ts
i18n.use(initReactI18next).init({
  lng: "vi",
  fallbackLng: "vi",
  resources: { vi: { translation: vi }, en: { translation: en } },
  interpolation: { escapeValue: false },
});
```

Tất cả chuỗi UI đi qua `t("key")`. `vi.json` là source of truth; `en.json` mirror khoá.

## Error Handling

```rust
#[derive(thiserror::Error, Debug, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum AppError {
    #[error("URL không hợp lệ")]
    InvalidUrl,
    #[error("Không hỗ trợ site này")]
    UnsupportedSite,
    #[error("yt-dlp thất bại: {0}")]
    YtDlpFailed(String),
    #[error("Không tìm thấy ffmpeg")]
    FfmpegMissing,
    #[error("Thư mục lưu không khả dụng")]
    SaveFolderUnavailable,
    #[error("Hết thời gian chờ")]
    Timeout,
    #[error("Trạng thái không hợp lệ: {from:?} → {event:?}")]
    IllegalTransition { from: DownloadState, event: String },
    #[error("Lỗi I/O: {0}")]
    Io(String),
    #[error("Lỗi cấu hình")]
    ConfigCorrupt,
}

pub type AppResult<T> = Result<T, AppError>;
```

Mọi `#[tauri::command]` trả `AppResult<T>`; serde serialize `AppError` thành discriminated union, frontend map sang chuỗi tiếng Việt qua `i18n` (key dạng `errors.invalidUrl`). Sidecar stderr được capture và chuyển vào `AppError::YtDlpFailed` với message tóm tắt (last non-empty line).

## Storage

- **SQLite** qua `tauri-plugin-sql` (sqlx driver), file `app_data_dir/state.db`.
- Tables:
  - `settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)` — single-row JSON blob đơn giản hoá migration.
  - `history(short_id TEXT PRIMARY KEY, url TEXT, title TEXT, extractor TEXT, format_id TEXT, save_folder TEXT, file_path TEXT, state TEXT, error_message TEXT, finished_at TEXT NOT NULL)` — index `finished_at DESC`, `title COLLATE NOCASE`.
- Queue **không** persist (chỉ in-memory) — restart App ⇒ queue rỗng. History lưu mọi terminal transition.

## Clipboard Watcher

```rust
async fn watch_clipboard(app: AppHandle, settings: Arc<RwLock<Settings>>) {
    let mut last_seen: HashMap<String, Instant> = HashMap::new();
    let mut interval = tokio::time::interval(Duration::from_millis(1000));
    loop {
        interval.tick().await;
        if !settings.read().await.clipboard_watcher { continue; }
        if let Some(text) = read_clipboard().ok() {
            if let Some(url) = extract_first_url(&text) {
                let now = Instant::now();
                let stale = last_seen.get(&url).map_or(true, |t| now.duration_since(*t) > Duration::from_secs(60));
                if stale && resolve_extractor(&url).is_some() {
                    last_seen.insert(url.clone(), now);
                    let _ = app.emit("clipboard_url_detected", json!({ "url": url }));
                }
            }
        }
    }
}
```

## Notifications

- Sử dụng `tauri-plugin-notification`. Khi `state_changed → completed|failed`, nếu `Settings.notifications`, emit:
  - completed: title = `t("notif.completed")`, body = `${title} (#${shortId})`.
  - failed: body = `${title} — ${errorMessage}`.
- `notification_clicked` event → frontend route `/queue?focus=<shortId>`.

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system — essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: URL syntax validation matches reference parser

For any string `s`, `validate_url(s)` returns `Ok` if and only if `s` parses as an absolute URL with scheme `http` or `https` and a non-empty host.

**Validates: Requirements 1.2**

### Property 2: Extractor host resolution is consistent

For any URL whose host matches a known extractor pattern, `resolve_extractor(url)` returns the corresponding extractor name; for any URL whose host is disjoint from every extractor pattern, it returns `None`.

**Validates: Requirements 1.3, 1.4**

### Property 3: Metadata rendering completeness

For any `VideoMetadata` value with non-empty `formats`, the rendered metadata view contains the title, channel, duration, and one row per `QualityFormat` showing resolution, fps, codec, and filesize when present; the rendered subtitle list shows one row per `SubtitleTrack`.

**Validates: Requirements 2.2, 2.3, 11.1**

### Property 4: Playlist entry count invariant

For any yt-dlp playlist JSON with `entries.length == n`, `parse_playlist(json).playlist_entries.len() == n` and `playlist_total == Some(n)`.

**Validates: Requirements 2.5**

### Property 5: Audio-only selector picks max-bitrate audio

For any non-empty list of `QualityFormat` containing at least one entry with `is_audio_only == true`, `select_audio_only(formats)` returns the audio-only format with the maximum `abr`; ties broken deterministically by `format_id`.

**Validates: Requirements 3.3**

### Property 6: yt-dlp argument construction reflects request

For any `DownloadRequest`, the argument vector built by `args_builder::build(req)` satisfies all of:
- contains `-N 16`
- contains `-o %(title)s.%(ext)s`
- contains `--paths home:<save_folder>`
- contains `--continue` if and only if `req.is_resume` is true
- contains `-f bv*+ba/b` when `mode == Video` and `format_id == None`, otherwise contains `-f <format_id>` (with `+ba/b` appended for video-only formats)
- contains `-x --audio-format mp3 --audio-quality 0` if and only if `mode == Audio`
- contains `--downloader aria2c --downloader-args aria2c:-x 16 -s 16 -k 1M` if and only if `settings.aria2c_enabled`
- contains `--write-subs --sub-langs <joined>` whenever `sub_langs` is non-empty, and `--write-auto-subs --sub-langs <target>` whenever `auto_translate_to` is `Some`
- contains `--yes-playlist` if and only if `req.playlist_all` is true

**Validates: Requirements 3.2, 3.4, 5.1, 5.2, 7.1, 7.2, 8.3, 9.5, 10.1, 11.2, 11.3**

### Property 7: Concurrency cap invariant

For any sequence of operations `(enqueue | start | complete | fail | cancel | set_max_concurrency)` applied to the queue, after each operation the count of items in state `Downloading` is less than or equal to the current `max_concurrency` setting.

**Validates: Requirements 5.4, 8.6**

### Property 8: Retry schedule

For any download item, the retry policy schedules at most 3 automatic retries with delays `[2s, 5s, 10s]` in order; on the 4th failure the item transitions to `Failed`.

**Validates: Requirements 5.5**

### Property 9: Short_ID shape and uniqueness

For any URL, timestamp, and `taken: HashSet<String>` (Short_IDs already in queue ∪ history), `generate_short_id(url, ts, &taken)` returns a string of length exactly 7 composed only of `[0-9A-Za-z]`, and the returned id is not in `taken`.

**Validates: Requirements 6.1, 6.3, 6.4**

### Property 10: Auto-rename suffix is minimal

For any stem, ext, and `existing: HashSet<String>`:
- `auto_rename(stem, ext, existing)` is not in `existing`.
- If `"<stem>.<ext>"` is not in `existing`, the result equals `"<stem>.<ext>"`.
- Otherwise the result equals `"<stem> (n).<ext>"` where `n` is the smallest positive integer such that the resulting name is not in `existing`.

**Validates: Requirements 7.4**

### Property 11: Queue state machine and retry round-trip

For any current `DownloadState` and `QueueEvent`, `transition(state, event)` returns the unique successor state defined by the legal-transition table or `Err(IllegalTransition)`; in particular, for any item ending in `Failed` or `Cancelled`, `retry(item)` produces a new item with the same `url` and `format_id` and `state == Queued`.

**Validates: Requirements 8.1, 8.2, 8.4, 8.5, 13.3**

### Property 12: Batch input parsing

For any multi-line string `text`, `parse_batch(text)` returns a list whose length equals the number of lines containing a syntactically valid URL, in input order, with each output URL equal to its source line trimmed.

**Validates: Requirements 9.2, 9.4**

### Property 13: Clipboard detection and dedupe window

For any sequence of clipboard tick observations of URLs `u₁, u₂, ...` with timestamps `t₁ < t₂ < ...`, the watcher emits `clipboard_url_detected{u}` exactly once per URL `u` within any 60-second sliding window, and only when `resolve_extractor(u)` is `Some` and the watcher setting is enabled.

**Validates: Requirements 12.2, 12.5**

### Property 14: History recording on terminal transition and delete round-trip

For any download item that transitions into `Completed`, `Failed`, or `Cancelled`, exactly one `HistoryEntry` is appended with matching `url`, `short_id`, `title`, `format_id`, `save_folder`, `state`, and a `finished_at` timestamp; for any history entry, `delete_history(short_id)` followed by `list_history()` yields a list that does not contain that `short_id`.

**Validates: Requirements 13.1, 13.5**

### Property 15: History sort order

For any list of `HistoryEntry`, `list_history({ sort: desc })` returns the entries sorted by `finished_at` in non-increasing order.

**Validates: Requirements 13.2**

### Property 16: History search filter correctness

For any history list `H` and query `q`, `search_history(H, q)` returns a sublist `R ⊆ H` such that for every `e ∈ R`, `e.title.contains_ignore_case(q) || e.url.contains_ignore_case(q)`, and for every `e ∈ H \ R`, neither field contains `q`.

**Validates: Requirements 13.4**

### Property 17: Settings round-trip and validation

For any settings patch `p`:
- If `p.max_concurrency` is outside `1..=10`, `update_settings(p)` returns `Err(InvalidSetting)` and the persisted store is unchanged.
- Otherwise, `update_settings(p)` followed by `get_settings()` returns a `Settings` value whose fields equal the patch-applied prior settings.

**Validates: Requirements 4.3, 12.4, 16.2**

### Property 18: Corrupt config recovery round-trip

For any byte sequence written to the settings file that fails to deserialize, the next `load_settings()` returns the default `Settings` and rewrites the file such that a subsequent `load_settings()` again returns those same defaults.

**Validates: Requirements 16.5**

### Property 19: i18n bundle completeness and switch

For every key `k` present in the `vi` resource bundle, `k` is also present in every other supported language bundle, and after `setLanguage(lang)`, `t(k)` equals `bundle[lang][k]` for all `k`.

**Validates: Requirements 14.5**

### Property 20: Notifications gated by settings

For any sequence of terminal-state transitions while `Settings.notifications == false`, zero notifications are dispatched; while `true`, exactly one notification is dispatched per terminal transition.

**Validates: Requirements 15.4**

## Testing Strategy

### Unit Tests (Rust, `cargo test`)

- `domain::url_validator` — example tests for malformed strings.
- `domain::short_id::generate_short_id` — collision regression.
- `domain::conflict::auto_rename` — explicit edge cases (empty existing set, deeply nested suffixes).
- `domain::queue_manager::transition` — transition table coverage.
- `domain::progress::parse_progress` — yt-dlp stdout fixtures.
- `sidecar::*` — args_builder snapshot tests.

### Property Tests (Rust, `proptest`)

Each property test runs **≥100 iterations** and is tagged in its docstring as:
`/// Feature: multi-platform-video-downloader, Property <N>: <property text>`

Targets: Properties 1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20.

Generators:
- `arb_url()` — structured http/https with valid host, occasionally invalid.
- `arb_quality_format()` / `arb_metadata()` — structured-aware random formats.
- `arb_download_request()` — combinator over modes, format ids, settings.
- `arb_queue_op_seq()` — sequences of queue operations for state-machine fuzzing.

### Frontend Tests

- **Vitest** unit tests for components (`UrlInput` debounce, `QualitySelector` mode toggle, `FolderPicker` value bind, `DownloadItemCard` Short_ID badge).
- **fast-check** property tests for `parse_batch`, i18n bundle completeness (Property 19), metadata rendering (Property 3).
- **Playwright** e2e: paste-URL → fetch metadata (mocked) → start download (mocked sidecar) → progress events → completed; conflict dialog flow; history re-download; theme/language switch.

### Integration / Smoke

- Smoke: app starts, settings load, default folder = OS Downloads on first run, sidecars resolved.
- Integration: live yt-dlp call against a stable public test URL (gated behind `--features integration`); ffmpeg MP3 conversion produces a valid `.mp3`.

### Out-of-scope for PBT

- Tauri dialog API, OS notifications, SQLite persistence timing, theme application timing, network retry timing — covered by integration tests with 1–3 examples each.


## Backend Modules (`src-tauri/src/`)

### `queue.rs` — Queue_Manager

Trách nhiệm: lưu danh sách Download_Item, lập lịch concurrency, gọi `ytdlp_runner` cho từng slot, áp dụng FSM, retry/backoff, phát events.

```rust
pub struct QueueManager {
    items:    RwLock<IndexMap<String /*short_id*/, DownloadItem>>,
    children: Mutex<HashMap<String, ChildHandle>>,        // running yt-dlp procs
    semaphore: Arc<Semaphore>,                            // size = max_concurrency
    settings:  Arc<SettingsStore>,
    history:   Arc<HistoryStore>,
    runner:    Arc<YtDlpRunner>,
    app:       AppHandle,
    cancel:    Mutex<HashMap<String, CancellationToken>>,
}

impl QueueManager {
    pub async fn enqueue(&self, item: DownloadItem) -> Result<()>;
    pub async fn pause(&self, id: &str) -> Result<()>;
    pub async fn resume(&self, id: &str) -> Result<()>;
    pub async fn cancel(&self, id: &str) -> Result<()>;
    pub async fn retry(&self, id: &str) -> Result<()>;
    pub async fn list(&self) -> Vec<DownloadItem>;
    pub async fn set_concurrency(&self, n: u8);           // resize semaphore (Req 8.6)
    async fn worker_loop(self: Arc<Self>);                // background scheduler
}
```

Scheduler: vòng lặp `worker_loop` chờ permit từ `Semaphore`, lấy item `Queued` đầu tiên, spawn task chạy yt-dlp; trên failure áp retry policy (xem mục Concurrency).

### `ytdlp_runner.rs` — YtDlp_Sidecar orchestrator

Trách nhiệm: build argv, spawn child via `tauri_plugin_shell::process::Command::sidecar("yt-dlp")`, parse stdout, emit progress events, đồng bộ `tokio::process::Child` để pause/cancel.

```rust
pub struct YtDlpRunner { app: AppHandle }

impl YtDlpRunner {
    pub async fn fetch_metadata(&self, url: &str) -> Result<VideoMetadata>;
    pub async fn run_download(
        &self,
        item: &DownloadItem,
        cancel: CancellationToken,
        progress: mpsc::Sender<ProgressEvent>,
    ) -> Result<RunOutcome>;
    pub fn build_argv(&self, item: &DownloadItem, resume: bool) -> Vec<String>;
}

pub enum RunOutcome { Completed { output: String }, Cancelled, Failed { reason: String } }
```

`build_argv` là pure function (sẽ được property-tested) — xem matrix mục yt-dlp Argument Matrix.

### `settings_store.rs` — Settings_Store

Trách nhiệm: load/save JSON tại `tauri::path::app_config_dir()/settings.json`, validate (clamp `max_concurrency` ∈ [1,10]), khôi phục default khi file hỏng.

```rust
pub struct SettingsStore { inner: RwLock<Settings>, path: PathBuf }

impl SettingsStore {
    pub fn load(path: PathBuf) -> (Self, Option<AppError>); // err nếu phải khởi tạo lại
    pub fn get(&self) -> Settings;
    pub fn update<F>(&self, f: F) -> Result<Settings> where F: FnOnce(&mut Settings);
    fn persist(&self) -> Result<()>;                       // debounce 500ms
}
```

### `history_store.rs` — History_Store (SQLite)

Schema:

```sql
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
  finished_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_history_finished_at ON history(finished_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_title       ON history(title);
```

```rust
pub struct HistoryStore { conn: Mutex<Connection> }

impl HistoryStore {
    pub fn open(path: PathBuf) -> Result<Self>;
    pub fn insert(&self, e: &HistoryEntry) -> Result<()>;
    pub fn list(&self, query: Option<&str>, limit: u32, offset: u32) -> Result<Vec<HistoryEntry>>;
    pub fn delete(&self, short_id: &str) -> Result<()>;
    pub fn known_short_ids(&self) -> Result<HashSet<String>>; // for collision check
}
```

### `clipboard.rs` — Clipboard_Watcher

Trách nhiệm: poll clipboard mỗi 1000ms khi enabled, dedupe URL trong 60s, emit event `clipboard-detected`.

```rust
pub struct ClipboardWatcher { app: AppHandle, settings: Arc<SettingsStore> }

impl ClipboardWatcher {
    pub fn start(self) -> JoinHandle<()>;     // background tokio task
    fn last_seen: Mutex<HashMap<String /*url*/, Instant>>; // 60s dedupe window
}
```

### `notification.rs` — System notifications

Bọc `tauri-plugin-notification`, kiểm `settings.notifications_enabled` trước khi gửi.

```rust
pub fn notify_completed(app: &AppHandle, settings: &Settings, item: &DownloadItem);
pub fn notify_failed   (app: &AppHandle, settings: &Settings, item: &DownloadItem, reason: &str);
```

Click handler: emit event `notification-clicked` với `short_id` để frontend focus item.

### `filename_resolver.rs` — Tên file & xung đột

Pure function tính path đầu ra dựa trên title/ext + nội dung folder. Strategy:

```rust
pub enum ConflictStrategy { Overwrite, Skip, AutoRename, Ask }

pub struct FilenameResolver;
impl FilenameResolver {
    /// Trả về (final_path, action) dựa trên strategy.
    pub fn resolve(
        save_folder: &Path,
        title: &str,
        ext: &str,
        existing: &dyn Fn(&Path) -> bool,
        strategy: ConflictStrategy,
    ) -> ResolveOutcome;
    pub fn sanitize(title: &str) -> String;     // strip path separators, control chars
    fn auto_rename_suffix(base: &Path, ext: &str, existing: &dyn Fn(&Path)->bool) -> PathBuf;
}

pub enum ResolveOutcome {
    Path(PathBuf),
    AskUser { suggested: PathBuf, conflicting: PathBuf },
    SkipItem,
}
```

### `short_id.rs`

```rust
pub fn generate(url: &str, created_at_ms: i64, taken: &HashSet<String>) -> String;
```

### `commands.rs`, `events.rs`

Lớp mỏng map từ Tauri commands vào module trên, định danh constants cho event names.

## Frontend Components & Pages (`src/`)

### Pages

| Page | Path | Mô tả |
|---|---|---|
| `PasteUrlPage` | `/` | Ô input URL + clipboard banner, hiển thị metadata + chọn quality + chọn folder + nút "Tải". |
| `QueuePage` | `/queue` | Danh sách Download_Item, progress bar, nút pause/resume/cancel/retry, conflict dialog. |
| `HistoryPage` | `/history` | Bảng history với search, action "Tải lại", "Xoá", mở folder. |
| `SettingsPage` | `/settings` | Form các trường settings (Req 16). |

### Components (`src/components/`)

- `UrlInput.tsx` — input + validate cú pháp realtime (debounced 100ms).
- `ClipboardBanner.tsx` — banner gợi ý URL từ clipboard.
- `PlatformBadge.tsx` — icon + tên 6 Featured_Platform.
- `MetadataCard.tsx` — title/thumb/duration/channel.
- `QualityPicker.tsx` — radio danh sách Quality_Format + tuỳ chọn "Best".
- `ModeToggle.tsx` — Video / Audio MP3.
- `SubtitlePicker.tsx` — multi-select ngôn ngữ + auto-translate.
- `FolderPicker.tsx` — input + nút mở Tauri dialog.
- `PlaylistEntryList.tsx` — checkbox cho từng entry.
- `BatchInput.tsx` — textarea nhiều URL.
- `QueueRow.tsx` — hàng item: short_id, title, progress, speed, ETA, actions.
- `ConflictDialog.tsx` — "Ghi đè / Bỏ qua / Tự động đổi tên".
- `HistoryRow.tsx` — hàng history với action.
- `ThemeProvider.tsx`, `I18nProvider.tsx`.

### Stores (Zustand) (`src/stores/`)

- `useUrlStore` — URL hiện tại + trạng thái validate + metadata.
- `useQueueStore` — list `DownloadItem`, action `pause/resume/cancel/retry`, sync với events.
- `useSettingsStore` — Settings, hành động `update()` đồng bộ với backend.
- `useHistoryStore` — paged history + query.
- `useClipboardStore` — last detected URL + dismissed set.

### i18n

- `src/i18n/vi.json` (default), `src/i18n/en.json`.
- Hook `useT()` trả về function `t(key, params)`. Bundle nạp ngay khi app khởi động sau khi đọc settings (Req 16.4).

## Tauri Commands

Tất cả command trả về `Result<T, AppError>` (serde-serialized).

| Command | Args | Return | Mục đích / Req |
|---|---|---|---|
| `validate_url` | `url: String` | `{ valid: bool, extractor: Option<String> }` | Req 1.2/1.3 |
| `fetch_metadata` | `url: String` | `VideoMetadata` | Req 2.1/2.5 |
| `enqueue_download` | `url, options: DownloadOptions, title, thumbnail, extractor` | `DownloadItem` | Req 3, 4, 5, 9, 10, 11 |
| `enqueue_batch` | `urls: Vec<String>, options` | `Vec<DownloadItem>` | Req 9.1/9.2 |
| `enqueue_playlist` | `playlist_url, selected: Vec<String>, options, all_with_yes_playlist: bool` | `Vec<DownloadItem>` | Req 9.3/9.4/9.5 |
| `pause_download` | `short_id` | `()` | Req 8.2 |
| `resume_download` | `short_id` | `()` | Req 8.3 |
| `cancel_download` | `short_id` | `()` | Req 8.4 |
| `retry_download` | `short_id` | `DownloadItem` | Req 8.5 |
| `resolve_conflict` | `short_id, action: "overwrite"\|"skip"\|"auto_rename"` | `()` | Req 7.3-7.6 |
| `list_queue` | `()` | `Vec<DownloadItem>` | Req 8 |
| `get_settings` | `()` | `Settings` | Req 16 |
| `update_settings` | `patch: Partial<Settings>` | `Settings` | Req 16.3 |
| `pick_folder` | `()` | `Option<String>` | Req 4.2 |
| `check_folder_writable` | `path: String` | `bool` | Req 4.5 |
| `list_history` | `query: Option<String>, limit: u32, offset: u32` | `Vec<HistoryEntry>` | Req 13.2/13.4 |
| `delete_history_entry` | `short_id` | `()` | Req 13.5 |
| `redownload_from_history` | `short_id, options_override: Option<...>` | `DownloadItem` | Req 13.3 |
| `set_clipboard_watcher` | `enabled: bool` | `()` | Req 12.4 |
| `open_in_folder` | `path: String` | `()` | reveal output |
| `open_url` | `url: String` | `()` | open in browser |
| `app_bootstrap` | `()` | `BootstrapPayload` | Req 16.4 (settings + queue snapshot) |

## Tauri Events (Backend → Frontend)

| Event name | Payload | Khi nào |
|---|---|---|
| `download://progress` | `{ short_id, progress: ProgressSnapshot }` | yt-dlp emit progress (≥ mỗi 500ms) — Req 5.3 |
| `download://state` | `{ short_id, status, error?, output_path? }` | FSM transition |
| `download://conflict` | `{ short_id, suggested_path, conflicting_path }` | trùng tên cần hỏi — Req 7.3 |
| `download://completed` | `{ short_id, output_path, title }` | sau khi commit thành công — Req 15.1 |
| `download://failed` | `{ short_id, reason }` | sau khi hết retry — Req 15.3 |
| `clipboard://detected` | `{ url, extractor }` | Req 12.2 |
| `notification://clicked` | `{ short_id }` | user click system notification — Req 15.2 |
| `settings://changed` | `Settings` | sau persist |
| `queue://updated` | `Vec<DownloadItem>` | snapshot khi có thay đổi lớn |

## yt-dlp Argument Matrix

`build_argv(item, resume)` ghép theo mode. Các tham số chung luôn có:

```
--dump-single-json          # chỉ trong fetch_metadata
-o "{save_folder}/%(title)s.%(ext)s"
-N 16                       # multi-connection mặc định (Req 5.1)
--newline                   # mỗi progress 1 dòng để parse
--no-warnings
--encoding utf-8
--no-mtime
--restrict-filenames        # KHÔNG bật — giữ tên gốc (Req 7.1/7.2)
```

(Lưu ý: `--restrict-filenames` được liệt kê chỉ để nhấn mạnh **không** bật.)

### Theo chế độ

| Mode / use case | Tham số bổ sung |
|---|---|
| **Fetch metadata** | `--dump-single-json --no-warnings <URL>` (không có `-o`) |
| **Video — Best** (format_id = None) | `-f "bv*+ba/b" --merge-output-format mp4` |
| **Video — chọn format_id** | `-f <format_id>` (nếu format không có audio: `-f "<id>+ba/b"`) |
| **Audio MP3** | `-x --audio-format mp3 --audio-quality 0` (Req 10.1) |
| **Subtitles (manual)** | `--write-subs --sub-langs "<csv>" --convert-subs srt` (Req 11.2) |
| **Subtitles (auto-translate)** | `--write-auto-subs --sub-langs "<target>" --convert-subs srt` (Req 11.3) |
| **Playlist (tải tất cả)** | `--yes-playlist` (Req 9.5) |
| **Playlist (single entry)** | `--no-playlist` |
| **Aria2c enabled** | `--downloader aria2c --downloader-args "aria2c:-x 16 -s 16 -k 1M"` (Req 5.2) |
| **Resume sau pause** | `--continue --no-overwrites` (Req 8.3) |
| **Conflict = Overwrite** | `--force-overwrites` (Req 7.6) |
| **Conflict = Auto-rename** | resolver đổi `-o` thành path đã có hậu tố ` (n)` |
| **Retry network** | `--retries 0` (retry do queue manager kiểm soát) — tránh đếm 2 lần |

Cuối cùng URL được append làm positional argument.

### Progress parser

yt-dlp với `--newline` xuất các dòng dạng:

```
[download]   1.2% of  120.50MiB at  3.10MiB/s ETA 00:39
[download] 100% of 120.50MiB in 00:42
```

Parser regex:

```
^\[download\]\s+(\d+(?:\.\d+)?)% of\s+(?:~\s*)?([\d.]+)([KMG]i?B)\s+at\s+([\d.]+)([KMG]i?B)/s\s+ETA\s+(\d+:\d+(?::\d+)?)
```

Convert sang `ProgressSnapshot`. Khi gặp `[download] Destination:` hoặc `[Merger]` chuyển progress → giai đoạn mux. Khi `[download] 100%` lần cuối + exit code 0 → `Completed`.

## File Naming & Conflict Resolution

### Sanitization

`sanitize(title)`:

1. Loại các ký tự bị OS cấm: `< > : " / \ | ? *` và control chars `0x00-0x1F`.
2. Trim whitespace cuối/đầu, ép max length 200 chars để tránh PATH_MAX.
3. Nếu rỗng sau sanitize → fallback `"video"`.

### Algorithm

```
input: save_folder, title, ext, strategy
sanitized = sanitize(title)
candidate = save_folder / "{sanitized}.{ext}"

if not exists(candidate):
    return Path(candidate)

switch strategy:
    Overwrite   → return Path(candidate)               # truyền --force-overwrites
    Skip        → return SkipItem
    AutoRename  → return Path(auto_rename_suffix(...))
    Ask         → return AskUser { suggested = auto_rename_suffix(...), conflicting = candidate }

auto_rename_suffix(save_folder, sanitized, ext):
    n = 1
    while exists(save_folder / "{sanitized} ({n}).{ext}"):
        n += 1
    return save_folder / "{sanitized} ({n}).{ext}"
```

`Ask` là default → emit `download://conflict`, frontend mở `ConflictDialog`, người dùng chọn → frontend gọi `resolve_conflict` → backend re-run với strategy đã chọn (Req 7.3-7.6).

## Short_ID Generation

```
alphabet = base32 lowercase ("abcdefghijklmnopqrstuvwxyz234567"), 32 chars
hash    = blake3(url || "|" || created_at_ms.to_string())   // 32 bytes
length  = 6  (mặc định, có thể nâng tới 8 khi va chạm)

while attempts < 10:
    take first {length} bytes of hash as base32 → id
    if id not in taken:
        return id
    length = min(length + 1, 8)
    re-hash with extra nonce (counter) and retry

if vẫn va chạm sau 10 lần ở length=8:
    sample 8 chars random từ alphabet với CSRNG, retry tới khi unique
```

`taken` = `queue.short_ids() ∪ history.known_short_ids()` (Req 6.1, 6.3, 6.4). Gọi dưới một mutex để tránh race.

## Concurrency Control & Retry Policy

### Concurrency

- `QueueManager` giữ một `Arc<Semaphore>` với `max_permits = settings.max_concurrency`.
- Worker loop: `permit = sem.acquire().await; spawn(run_item(item, permit))`.
- Khi user đổi `max_concurrency`:
  - Nếu giá trị mới `> hiện tại` → `sem.add_permits(delta)`.
  - Nếu `< hiện tại` → mark "shrinking", các permit đang giữ không lấy thêm; khi worker hoàn tất nó **không** release lại slot dư cho tới khi tổng permit ≤ giá trị mới. Đảm bảo cap mới đạt trong ≤ 2s sau khi tất cả running task kế tiếp finalize, mọi item mới chỉ chạy khi free slot — Req 8.6.

### Retry policy (Req 5.5)

```
delays_ms = [2000, 5000, 10000]
on transport error (exit code != 0 với stderr match network patterns, hoặc IO error):
    if retry_count < 3:
        sleep(delays_ms[retry_count])
        retry_count += 1
        re-run với --continue
    else:
        status = Failed
        emit download://failed
```

`yt-dlp --retries 0` để chỉ Queue Manager kiểm soát retry (tránh đếm trùng).

### Cancellation

- Mỗi run gắn một `CancellationToken` (tokio-util). `cancel()` → `child.kill().await` + `token.cancel()`.
- Pause: gửi `child.kill()` (yt-dlp tự ghi `.part`), đặt `Paused`. Resume gọi lại với `--continue`.

### Timeouts

- Fetch metadata timeout 30s (Req 2.4) qua `tokio::time::timeout`.
- Spawn yt-dlp với env `LANG=C.UTF-8`, redirect stdout/stderr line-buffered.

## Project Structure

```
prodowwn/
├─ .kiro/specs/multi-platform-video-downloader/
├─ src/                              # React frontend
│  ├─ main.tsx
│  ├─ App.tsx
│  ├─ pages/
│  │  ├─ PasteUrlPage.tsx
│  │  ├─ QueuePage.tsx
│  │  ├─ HistoryPage.tsx
│  │  └─ SettingsPage.tsx
│  ├─ components/...
│  ├─ stores/
│  │  ├─ useQueueStore.ts
│  │  ├─ useSettingsStore.ts
│  │  ├─ useHistoryStore.ts
│  │  ├─ useUrlStore.ts
│  │  └─ useClipboardStore.ts
│  ├─ ipc/
│  │  ├─ commands.ts                 # typed wrappers around invoke()
│  │  └─ events.ts                   # typed listen() helpers
│  ├─ types/models.ts
│  ├─ i18n/
│  │  ├─ vi.json
│  │  └─ en.json
│  ├─ lib/
│  │  ├─ url.ts                      # isValidUrl, host extraction
│  │  ├─ format.ts                   # formatBytes, formatEta
│  │  └─ best-format.ts              # selectBest(formats)
│  └─ styles/
│     └─ index.css                   # tailwind base
├─ src-tauri/
│  ├─ Cargo.toml
│  ├─ tauri.conf.json
│  ├─ build.rs
│  ├─ binaries/                      # sidecars (per target triple)
│  │  ├─ yt-dlp-x86_64-pc-windows-msvc.exe
│  │  ├─ ffmpeg-x86_64-pc-windows-msvc.exe
│  │  ├─ yt-dlp-x86_64-apple-darwin
│  │  └─ ...
│  ├─ icons/
│  └─ src/
│     ├─ main.rs
│     ├─ lib.rs
│     ├─ commands.rs
│     ├─ events.rs
│     ├─ models.rs
│     ├─ queue.rs
│     ├─ ytdlp_runner.rs
│     ├─ progress_parser.rs
│     ├─ settings_store.rs
│     ├─ history_store.rs
│     ├─ clipboard.rs
│     ├─ notification.rs
│     ├─ filename_resolver.rs
│     ├─ short_id.rs
│     └─ extractors.rs               # Featured_Platform mapping + host→extractor
├─ scripts/
│  ├─ fetch-sidecars.mjs             # tải/đặt tên yt-dlp + ffmpeg theo target triple
│  └─ verify-sidecars.mjs
├─ tests/
│  ├─ rust/                          # cargo tests + proptest
│  └─ web/                           # vitest + fast-check
├─ package.json
├─ vite.config.ts
├─ tailwind.config.js
└─ tsconfig.json
```

## Sidecar Binary Setup

### Target triples

Tauri yêu cầu sidecar có hậu tố target triple: `<name>-<triple>[.exe]`.

Hỗ trợ chính:

| Platform | Triple |
|---|---|
| Windows x64 | `x86_64-pc-windows-msvc` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| Linux x64 | `x86_64-unknown-linux-gnu` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` |

### `tauri.conf.json` (trích)

```json
{
  "bundle": {
    "externalBin": [
      "binaries/yt-dlp",
      "binaries/ffmpeg"
    ]
  },
  "plugins": {
    "shell": {
      "scope": [
        { "name": "yt-dlp", "sidecar": true, "args": true },
        { "name": "ffmpeg", "sidecar": true, "args": true }
      ]
    },
    "dialog": {},
    "notification": {}
  }
}
```

### `scripts/fetch-sidecars.mjs` (logic)

```
const triple = detectTriple();        // từ process.platform + arch
const dir    = "src-tauri/binaries";

const ytdlpUrl = {
  "x86_64-pc-windows-msvc":   "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe",
  "x86_64-apple-darwin":      ".../yt-dlp_macos",
  "aarch64-apple-darwin":     ".../yt-dlp_macos",
  "x86_64-unknown-linux-gnu": ".../yt-dlp_linux",
  "aarch64-unknown-linux-gnu":".../yt-dlp_linux_aarch64",
}[triple];

download(ytdlpUrl)  → dir/`yt-dlp-${triple}${ext}`
download(ffmpegUrl) → dir/`ffmpeg-${triple}${ext}`
chmod +x (Unix)
verify sha256 từ manifest đi kèm
```

Script chạy postinstall (`"postinstall": "node scripts/fetch-sidecars.mjs"`).

### aria2c (optional)

aria2c **không** phải sidecar bắt buộc; người dùng tự cài. Backend phát hiện qua `which aria2c`/`where aria2c` khi load Settings; nếu không tìm thấy mà `aria2c_enabled = true`, frontend hiển thị cảnh báo.

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system — essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: URL syntactic validation

For any string `s`, `isValidUrl(s)` returns true if and only if `s` parses as an absolute URL with scheme `http` or `https` and a non-empty host.

**Validates: Requirements 1.2**

### Property 2: Host → extractor resolution is total and consistent

For any URL whose host matches an entry in the extractor table, `resolveExtractor(url)` returns the matching extractor; for any URL whose host does not match, it returns `None`.

**Validates: Requirements 1.3, 1.4**

### Property 3: yt-dlp argv builder mode invariants

For any `DownloadItem` and `resume` flag, `build_argv(item, resume)` satisfies all of the following simultaneously:

- always contains `-o "<save_folder>/%(title)s.%(ext)s"` and never contains a prefix/suffix/timestamp token;
- always contains `-N 16` unless overridden by `options.max_connections`;
- contains `-x --audio-format mp3 --audio-quality 0` if and only if `mode == AudioMp3`;
- contains `-f "bv*+ba/b"` if and only if `mode == Video` and `format_id is None`;
- contains `--write-subs --sub-langs "<csv>" --convert-subs srt` if and only if `subtitle_langs` is non-empty;
- contains `--write-auto-subs --sub-langs "<lang>"` if and only if `auto_translate_to.is_some()`;
- contains `--downloader aria2c --downloader-args "aria2c:-x 16 -s 16 -k 1M"` if and only if `use_aria2c == true`;
- contains `--continue` if and only if `resume == true`;
- ends with the URL as positional argument.

**Validates: Requirements 5.1, 5.2, 7.1, 7.2, 8.3, 9.5, 10.1, 10.3, 11.2, 11.3, 11.4**

### Property 4: Playlist metadata count

For any playlist URL whose metadata yields `n` entries, `fetch_metadata(url).playlist.entries.len() == n` and `playlist.total == n`.

**Validates: Requirements 2.5, 9.3**

### Property 5: Best-format selector dominates

For any non-empty list of `QualityFormat` values `fs`, `selectBest(fs)` returns a format `f*` such that no other element of `fs` strictly dominates `f*` on the lexicographic ordering `(height, fps, tbr)`.

**Validates: Requirements 3.2**

### Property 6: Audio-mode selects max bitrate

For any non-empty list of audio-only formats `as`, the default audio selection has `abr == max(a.abr for a in as)`.

**Validates: Requirements 3.3**

### Property 7: Filename auto-rename uses smallest free suffix

For any `(save_folder, title, ext)` and any existing-set `E` of files in that folder, `auto_rename_suffix` returns `"<title> (n).<ext>"` where `n` is the smallest positive integer such that the resulting path is not in `E`. When `E` does not contain `"<title>.<ext>"`, the resolver returns `"<title>.<ext>"` unchanged.

**Validates: Requirements 7.1, 7.2, 7.4**

### Property 8: Filename sanitizer preserves valid titles

For any title `t` containing none of `< > : " / \ | ? *` nor control characters, `sanitize(t).trim() == t.trim()`. For any title containing such characters, `sanitize(t)` contains none of them.

**Validates: Requirements 7.1, 7.2**

### Property 9: Short_ID format and uniqueness

For any URL, timestamp, and `taken` set, `generate(url, ts, taken)` returns an id whose length is between 6 and 8 inclusive, whose characters are all from the base32 alphabet, and which is not in `taken`. For any sequence of generations against the same store, no two returned ids are equal.

**Validates: Requirements 6.1, 6.3, 6.4**

### Property 10: Queue concurrency invariant

For any sequence of enqueue/pause/resume/cancel operations and any `max_concurrency = N`, the number of `DownloadItem` in state `Downloading` at any instant is at most `N`. After updating `max_concurrency` to `N'`, this bound becomes `N'` within at most 2 seconds of stable scheduling time.

**Validates: Requirements 5.4, 8.6**

### Property 11: Retry schedule

For any flaky download that fails with a transport error every attempt, the queue manager performs at most 3 retries with inter-attempt delays equal to the prefix of `[2s, 5s, 10s]`, after which the item is marked `Failed`.

**Validates: Requirements 5.5**

### Property 12: FSM legal transitions

For any `DownloadItem`, every status transition observed at runtime belongs to the set `{Queued→Downloading, Downloading→Paused, Paused→Downloading, Downloading→Completed, Downloading→Failed, *→Cancelled, *→Skipped, Failed→Queued (retry), Cancelled→Queued (retry)}`. No other transition occurs.

**Validates: Requirements 8.1, 8.2, 8.3, 8.4, 8.5**

### Property 13: Batch parser yields exactly the valid URLs

For any newline-separated input string, the batch parser returns the list of strings whose `isValidUrl` is true, in original order, with whitespace trimmed and duplicates preserved.

**Validates: Requirements 9.1, 9.2**

### Property 14: Playlist selection round-trip

For any playlist with entries `E` and any subset `S ⊆ E` selected by the user, `enqueue_playlist` creates exactly `|S|` `DownloadItem`s whose URLs are exactly the URLs in `S`.

**Validates: Requirements 9.4**

### Property 15: Progress line parser is total and structured

For any line emitted by yt-dlp matching the documented progress format, `parse_progress_line` returns a `ProgressSnapshot` with consistent fields (`percent ∈ [0,100]`, `downloaded_bytes ≤ total_bytes` when both present, `eta_secs ≥ 0`). For any non-matching line it returns `None` without panicking.

**Validates: Requirements 5.3**

### Property 16: Clipboard dedup window

For any URL `u` detected at time `t0`, no `clipboard://detected` event is emitted for `u` again within the time interval `[t0, t0 + 60s)`.

**Validates: Requirements 12.1, 12.5**

### Property 17: History persistence round-trip and ordering

For any sequence of `HistoryEntry` insertions, the result of `history.list(query=None, limit=∞, offset=0)` contains exactly those entries (matched on `short_id`) and is sorted by `finished_at` strictly descending. Deleting an entry removes it from subsequent listings.

**Validates: Requirements 13.1, 13.2, 13.5**

### Property 18: History search semantics

For any query string `q` and any set of `HistoryEntry`s, `history.list(query=q, ...)` returns exactly the subset whose `title` or `url` contains `q` (case-insensitive).

**Validates: Requirements 13.4**

### Property 19: Re-download preserves URL and format

For any `HistoryEntry`, `redownload_from_history(short_id)` produces a new `DownloadItem` in state `Queued` whose `url` and `options.format_id` match the entry, and whose `short_id` is fresh and unique.

**Validates: Requirements 13.3, 6.3**

### Property 20: Settings persistence round-trip and clamping

For any `Settings` patch `p`, after `update_settings(p)` followed by `get_settings()` (or app restart), the returned settings equal the merge of prior settings and `p`, with `max_concurrency` clamped into `[1, 10]` and any unrecognized fields rejected. If the on-disk file is corrupt at load time, the store re-initializes defaults (`max_concurrency = 3`) and overwrites the file.

**Validates: Requirements 16.2, 16.3, 16.5**

### Property 21: i18n bundle completeness

For any supported locale `L ∈ {"vi", "en"}` and any UI key `k` referenced by a component, the locale bundle `L` defines a non-empty string for `k`.

**Validates: Requirements 14.1, 14.5**

### Property 22: Notification gating

For any `DownloadItem` reaching a terminal state, `notify(...)` is called with payload containing the title and `short_id` if and only if `settings.notifications_enabled == true`.

**Validates: Requirements 15.1, 15.3, 15.4**

## Error Handling

- Mọi command trả `Result<T, AppError>`; frontend dùng discriminated union để hiển thị thông báo tiếng Việt.
- Timeouts (Req 2.4): `tokio::time::timeout(30s, fetch_metadata)` → `AppError::Timeout`.
- Folder không ghi được (Req 4.5): kiểm `check_folder_writable` ngay trước khi enqueue; nếu fail trong lúc tải, parse stderr → mark `Failed` với message dịch sẵn.
- Ffmpeg thiếu (Req 10.4): khi sidecar không tồn tại lúc bootstrap → set flag `ffmpegMissing = true`, command audio trả `AppError::FfmpegMissing`.
- Settings hỏng (Req 16.5): `serde_json::from_str` lỗi → log + ghi đè defaults, return `(Settings::default(), Some(AppError::SettingsCorrupt))` để frontend toast cảnh báo.
- yt-dlp stderr được capture; mapping pattern phổ biến (`HTTP Error 403`, `Sign in to confirm`, `Video unavailable`) → message tiếng Việt thân thiện.

## Testing Strategy

- **Unit tests (Rust + Vitest):** ví dụ cụ thể cho UI, FSM, parser.
- **Property tests:** dùng `proptest` (Rust) cho `build_argv`, `auto_rename_suffix`, `parse_progress_line`, `short_id::generate`, queue scheduler (với virtual time qua `tokio::time::pause`); dùng `fast-check` (TS) cho `isValidUrl`, `selectBest`, batch parser, history filter, i18n completeness.
- **Integration tests:** mock yt-dlp child process bằng script in dữ liệu cố định để kiểm progress event flow; test SQLite migrations với temp dir.
- **Smoke tests:** bootstrap App với settings rỗng → defaults được áp; sidecar binaries phát hiện đúng theo triple.
- Tối thiểu 100 iterations cho mỗi property test.
