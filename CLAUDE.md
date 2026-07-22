# bqhungdown (prodown) — tool TẢI video đa nền tảng (Tauri: Rust + React/TS)

- **ĐỌC `INTEGRATION.md` TRƯỚC KHI SỬA GÌ** — repo này là 1 nửa của dây chuyền
  với tool cắt "BQ Hung Video" (`D:\claude\ai-content-studio`). Giao kèo thư mục
  trung chuyển theo kênh nằm ở đó, sửa lệch là gãy dây chuyền.
- Tính năng lõi: tải yt-dlp (sidecar) + theo dõi kênh tự tải (`watcher.rs`,
  `watchlist_store.rs`, sổ `seen_ids` chống trùng), hàng đợi (`queue.rs`),
  potoken/cookie chống chặn.
- Chạy dev: `npm run tauri:dev` · Build exe: `npm run tauri:build`
  (exe: `src-tauri/target/release/`, installer trong `bundle/`).
- Quy trình an toàn: làm trên nhánh riêng → test LOCAL → user nghiệm thu →
  mới push/phát hành. KHÔNG push khi user chưa duyệt.
- Chủ app: BQ Hung — trao đổi tiếng Việt.
