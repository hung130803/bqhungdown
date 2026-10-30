# Bot Telegram điều khiển BQHungDown

Điều khiển và theo dõi app tải từ Telegram: thêm link, xem tiến độ, dừng/huỷ/thử lại,
quét kênh, thay cookie, đổi cài đặt nhanh, nhận thông báo. Bot chạy **cùng máy** với
app và nói chuyện với app qua cổng điều khiển cục bộ (`127.0.0.1`, có khoá bí mật).

```
Telegram ⇄ bot.py (Python, máy anh) ⇄ http://127.0.0.1:47321 (app BQHungDown)
```

## Cài đặt (1 lần)

1. Cài Python 3.10+ rồi: `pip install -r requirements.txt`
2. Nhắn **@BotFather** → `/newbot` → lấy token.
3. Chạy `run-bot.bat` lần đầu: bot tạo `config.json` → điền `telegram_token`.
4. Mở app BQHungDown (bản có cổng điều khiển). App tự tạo
   `%APPDATA%\com.prodown.app\control_api.json` (cổng + khoá) — bot tự đọc file này.
5. Chạy lại `run-bot.bat`, nhắn bất kỳ cho bot → bot trả **mã chat** →
   điền vào `allowed_chat_ids` trong `config.json` → chạy lại. Chỉ mã chat trong
   danh sách mới điều khiển được bot.

`python bot.py --check` kiểm tra kết nối Telegram + app rồi thoát.

**App và bot tự bật lẫn nhau:**
- Bot chạy lần đầu tự ghi `%APPDATA%\com.prodown.app\telegram_bot.json`. Từ đó **mở
  app là app tự bật bot chạy ngầm** (không cửa sổ), bot văng thì app bật lại.
  Tắt tự bật: sửa `"enabled": false` trong file đó.
- **App đang tắt → bấm "▶ Mở app trên máy tính" (hoặc `/openapp`) trên điện thoại**
  để bật app từ xa. Bot tìm app ở bản cài (`%LOCALAPPDATA%\BQHungDown`), hoặc điền
  `app_exe` trong `config.json`.
- Mỗi lúc chỉ 1 bot chạy (khoá `logs\bot.lock`); bản thứ hai thoát ngay (mã 4).

**Muốn điều khiển được cả khi app tắt** thì bot phải luôn chạy: nhấn `Win + R` →
`shell:startup` → kéo (giữ chuột phải → *Create shortcuts here*) `start-all.bat`
vào thư mục vừa mở. Máy tính bật là bot + app chạy.

## Dùng

| Việc | Cách |
|---|---|
| Tải video | Gửi 1 hay nhiều link vào chat |
| Tải âm thanh | `/audio link…` |
| Bảng điều khiển | `/menu` (nút bấm) |
| Tình trạng | `/status` |
| Hàng đợi / lỗi | `/queue`, `/failed` — nút ⏸ ▶ ✖ 🔁 🗑 từng mục, "Thử lại tất cả" |
| Kênh theo dõi | `/channels` — lọc nhóm / kênh lỗi, 🔄 quét, +1 thêm lượt, ⏸ tắt/bật |
| Quét kênh | `/check` (tất cả) hoặc `/check Tên-nhóm` |
| Thêm kênh | `/addchannel link [videos\|shorts\|all]` |
| Thay cookie | Gửi file `cookies.txt` (chú thích: `youtube`, `tiktok`, `douyin`, `facebook`, `instagram`, `bilibili`, `twitter` — bỏ trống thì bot tự nhận) |
| Gửi video về điện thoại | `/history` → 📤, hoặc nút **📤 Gửi về điện thoại** khi tải xong. File > 49 MB (giới hạn Telegram) được nén thành bản xem thử dưới 45 MB bằng ffmpeg của app; file gốc trên máy tính giữ nguyên |
| Bảng tự cập nhật | `/board` (1 tin tự sửa, không spam), `/stopboard` |
| Cài đặt nhanh | `/settings` — bật/tắt tự quét, số luồng, nhịp quét |
| Sửa lỗi tải | `/fix` (cập nhật yt-dlp…) |
| Bật app từ xa | `/openapp` hoặc nút **▶ Mở app trên máy tính** (máy tính phải đang mở) |

**Bot tự báo:** tải xong / lỗi (kèm nút thử lại) / video mới từ kênh (gom 1 tin mỗi
20 giây) · cookie hỏng hoặc sắp hết hạn · kênh quét lỗi · app tắt/chạy lại.

## Thiết kế tốc độ

- Ép IPv4; mỗi luồng 1 `requests.Session`; timeout `(5, đọc)`.
- 1 luồng riêng long-polling `getUpdates`; vòng chính thức bằng `threading.Event`.
- Lệnh nhẹ trả lời ngay; lệnh nặng → ThreadPool 3 luồng; cùng 1 mục/kênh làm lần lượt.
- Bấm nút: `answerCallbackQuery("⏳ Đang làm...")` ngay ở luồng nền riêng (test đo < 1 giây).
- 429: edit/answer/delete bỏ qua; send* chờ `retry_after` rồi gửi lại; giãn 0,35 s giữa các send*.
- Cache đọc 15–60 giây (kênh, cookie, lịch sử); thao tác ghi xoá cache.
- Tiến độ: sửa 1 tin. Việc dài (sửa bộ tải, thêm kênh, gửi file) gửi đi không chờ.
- Lệnh > 3 giây ghi log `logs/bot.log`.

## Bảo trì / nâng cấp

```
bqbot/net.py         Telegram API (IPv4, session, 429, giãn send)
bqbot/app_client.py  Gọi cổng điều khiển của app (+ cache)
bqbot/dispatcher.py  Luồng nghe, ThreadPool, khoá theo đối tượng, việc dài
bqbot/handlers.py    Lệnh + nút bấm
bqbot/monitor.py     Thông báo + bảng theo dõi
bqbot/fmt.py         Định dạng tin (hàm thuần)
bqbot/launcher.py    Đăng ký tự bật cùng app, chỉ 1 bản chạy, mở app từ xa
bqbot/compress.py    Nén video > 49 MB để gửi về điện thoại
tests/test_bot.py    Test — KHÔNG gửi tin thật
```

**Thêm lệnh mới (3 bước):**
1. Rust: thêm route trong `src-tauri/src/control_api.rs` (gọi lại `commands::*`, KHÔNG viết lại nghiệp vụ).
2. `app_client.py`: thêm 1 hàm mỏng gọi route đó.
3. `handlers.py`: thêm vào `self.commands` / bảng callback + `BOT_COMMANDS`.

Thêm ô cookie trang mới: sửa `COOKIE_SITES` ở **cả** `control_api.rs` và `handlers.py`
(test `test_cookie_sites_match_rust` báo nếu lệch) và `SITE_COOKIE_SLOTS` bên giao diện.

Chạy test: `python -m unittest discover -s tests -v`

## Bảo mật

- Cổng chỉ nghe `127.0.0.1`; mọi lệnh cần khoá trong `control_api.json` (không gửi đi đâu).
- Đổi khoá: xoá `control_api.json` rồi mở lại app (bot đọc khoá mới tự động).
- Tắt hẳn cổng: sửa `"enabled": false` trong `control_api.json` rồi mở lại app.
- `config.json` (chứa token bot) đã nằm trong `.gitignore` — đừng gửi file này cho ai.
