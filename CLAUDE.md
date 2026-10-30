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
  Đường RSS (`watcher::fetch_rss_videos`) không có thông tin live; không được
  coi lọc danh sách là đủ. Cổng tải cuối bổ sung ngày 25/10 bên dưới bảo vệ
  cả nguồn này. Mục vẫn có thể xuất hiện trong queue rồi bị Bỏ qua khi kiểm
  tra metadata thật; không có nghĩa đã tải nội dung live.
- Chủ app: BQ Hung — trao đổi tiếng Việt.
- **MẶC ĐỊNH KHÔNG TẢI LIVE (25/10/2026, yêu cầu trực tiếp của chủ app):**
  chặn `is_live`, `is_upcoming`, `post_live` ở cả quét kênh và lệnh tải cuối
  (`args_builder::LIVE_FILTER`). Áp dụng URL tay, RSS, cache, queue khôi phục,
  retry và force-redownload. Không chờ đến giờ phát; báo Bỏ qua có lý do,
  không ghi Hoàn tất hoặc tự retry. Giữ `was_live` đã thành video hoàn chỉnh
  và video thiếu metadata live. Không suy đoán theo chữ "live" trong tiêu đề.
  RSS không có trạng thái live nên bộ lọc lúc quét không đủ bảo vệ.
  Video bị cổng tải chặn vì live (qua RSS) được GIỮ TẠM 6 giờ
  (`WatchedChannel.live_hold`, `watcher::LIVE_HOLD_SECS`): nằm trong seen nên
  không chiếm suất mỗi lượt quét, hết hạn thì gỡ khỏi seen để xét lại — KHÔNG
  được cho vào skipped_ids (mất video khi nó thành was_live).
- **BOT TELEGRAM + CỔNG ĐIỀU KHIỂN (30/10/2026):** `src-tauri/src/control_api.rs`
  mở HTTP `127.0.0.1:<port>` (khoá trong `<config>/control_api.json`) cho bot
  `telegram-bot/` (Python). Route CHỈ gọi lại `commands::*` — không viết lại
  nghiệp vụ, để bấm trên bot = bấm trên máy (cùng luật live/chống trùng/thư
  mục). Thêm tính năng bot: route Rust → hàm trong `app_client.py` → lệnh trong
  `handlers.py` (xem `telegram-bot/README.md`). `COOKIE_SITES` phải khớp ở
  `control_api.rs`, `handlers.py` và `SITE_COOKIE_SLOTS` (SettingsPage.tsx).
  Test bot: `cd telegram-bot && python -m unittest discover -s tests`.
