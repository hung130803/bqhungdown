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
- **BỎ VIDEO LIVE / CHỜ LIVE KHI QUÉT KÊNH** (anh Hùng 07/08/2026: "video
  phát live hoặc đang chờ phát live sẽ k lấy mấy video đó bỏ đi, k tự tải và
  k cho rằng video nó là video mới nhất"). Trước đây app chỉ biết SAU KHI đã
  tải (error.rs có sẵn "live event will begin") -> video live chiếm chỗ "mới
  nhất" -> tự tải -> vấp lỗi -> kênh đứng im.
  ĐO THẬT trên @SkyNews/streams (đừng đoán — bài học phân loại Shorts):
  flat-playlist CÓ trả `live_status` = `is_upcoming` (duration=null) ·
  `is_live` (duration=null) · `was_live` (duration THẬT 949s/25568s = đã
  thành video thường). Có 2 CỬA lấy video, phải vá CẢ HAI:
  `channel_fetcher::parse_entry` (bỏ is_live/is_upcoming/post_live) và
  `youtube_api::parse_video_item` (`snippet.liveBroadcastContent` =
  live/upcoming; snippet đã có trong `part=` nên KHÔNG tốn thêm quota).
  BẤT BIẾN: `was_live` + video thường + extractor không trả live_status
  (TikTok/Douyin) đều phải GIỮ — vá quá tay là mất video thật.
  CHƯA LÀM: đường RSS (`watcher::fetch_rss_videos`) không có thông tin live.
  Nó KHÔNG gây tải live (candidate lấy từ `vet_pool` đã lọc) nhưng có thể
  đánh dấu "đã làm" cho id chờ-live. Muốn sửa thì phải KIỂM CHỨNG trước xem
  premiere có nằm trong feed hay không (lần thử 07/08 feed trả rỗng).
- Chủ app: BQ Hung — trao đổi tiếng Việt.
