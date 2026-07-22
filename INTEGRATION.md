# GIAO KÈO DÂY CHUYỀN 2 TOOL (INTEGRATION CONTRACT)

> File này đặt Ở CẢ 2 REPO (bqhungdown + BQ Hung Video) và là NGUỒN SỰ THẬT
> DUY NHẤT về cách 2 app nói chuyện với nhau. Mọi phiên AI/dev sửa 1 trong 2
> app PHẢI đọc và tuân thủ file này; muốn đổi giao kèo thì đổi Ở CẢ 2 REPO
> trong cùng 1 đợt.

## Bức tranh

```
[bqhungdown - tool TẢI]     [BQ Hung Video - tool CẮT]      [tool ĐĂNG (bên thứ 3)]
Theo dõi kênh nguồn          Quét thư mục trung chuyển        Quét thư mục xuất
→ video mới TỰ TẢI vào       → mỗi kênh 1 video/ngày          → đăng Part
  <TRUNG_CHUYỂN>\<Kênh>\     → cắt/reup theo cấu hình kênh    → tự xóa Part sau đăng
                             → xuất Part vào thư mục xuất kênh
                             → XÓA video gốc
```

## 1. Thư mục trung chuyển (handoff)

- Gốc trung chuyển do user chọn (ví dụ `D:\daychuyen`). KHÔNG hardcode.
- Mỗi kênh 1 thư mục con: `<TRUNG_CHUYỂN>\<Tên kênh>\`.
- **`<Tên kênh>` phải TRÙNG TỪNG KÝ TỰ với tên kênh trong BQ Hung Video**
  (kể cả dấu, khoảng trắng). Tool tải để user gán "thư mục lưu" cho từng
  kênh theo dõi; user trỏ vào đúng thư mục con này.
- Tool cắt: thư mục con không khớp tên kênh nào → KHÔNG xử lý, ghi vào
  báo cáo "thư mục không khớp kênh".

## 2. Luật file (tool tải phải giữ)

- File đang tải mang đuôi tạm (`.part`/`.ytdl`/`.tmp`) hoặc tên tạm; CHỈ
  khi tải xong mới mang tên/đuôi video hoàn chỉnh (mp4/mkv/webm/mov/m4v).
  (yt-dlp mặc định `.part` rồi rename — đạt chuẩn.)
- Tool cắt chỉ ăn file "ĐỨNG YÊN": không đuôi tạm + size không đổi giữa 2
  lần quét cách nhau ≥ 10 giây + mtime cách hiện tại ≥ 30 giây.

## 3. Chống trùng (2 phía độc lập, không phụ thuộc file còn hay mất)

- Tool tải: sổ `seen_ids` theo kênh (ID video nền tảng) — video đã tải/đã
  thấy thì KHÔNG BAO GIỜ tải lại, kể cả khi file đã bị tool cắt xóa.
- Tool cắt: sổ đã-xử-lý theo kênh (hash nội dung + tên file) — file trùng
  → bỏ qua + báo cáo "trùng với video đã làm ngày X".

## 4. Vòng đời video gốc (tool cắt chịu trách nhiệm)

- Xử lý THÀNH CÔNG (đủ Part xuất vào thư mục xuất của kênh) → **XÓA video
  gốc** khỏi thư mục trung chuyển. Video gốc KHÔNG BAO GIỜ được chép vào
  thư mục xuất (thư mục xuất chỉ chứa Part — tránh tool đăng đăng nhầm gốc).
- Xử lý LỖI (phân tích/cắt/xuất hỏng sau retry) → chuyển video gốc sang
  `<TRUNG_CHUYỂN>\_Loi\<Kênh>\` + ghi lý do vào báo cáo. Không xóa oan.
- File hỏng/không đọc được (ffprobe fail) → cũng chuyển `_Loi` + báo.

## 5. Nhịp chạy & hạn mức (tool cắt)

- Mặc định chạy KHI USER BẤM "▶ Chạy dây chuyền" (không tự chạy nền);
  có tùy chọn bật "tự canh thư mục" cho ai muốn full auto.
- Mỗi kênh tối đa `daily_limit` video/ngày (mặc định 1). File dư nằm chờ
  các ngày sau (backlog là nguyên liệu ngày mai — không xóa).
- Thứ tự trong kênh: file có mtime CŨ NHẤT trước (đến trước làm trước).

## 6. Cấu hình theo kênh (tool cắt)

- `pipe_on` (bật dây chuyền cho kênh), `pipe_src` (thư mục trung chuyển
  của kênh; mặc định `<gốc>\<Tên kênh>`), `pipe_mode` ("auto" = Tạo clip
  thường | "recap" = Reup thuyết minh), `pipe_daily` (1-2).
- Thư mục xuất Part dùng cấu hình sẵn có (export_dir của kênh).

## 7. Báo cáo & cảnh báo (tool cắt)

Mỗi lần chạy ra báo cáo từng kênh: nhận video nào / trạng thái
(✅ xong N Part | ⏳ đang | 🔴 lỗi + LÝ DO RÕ: hết key Groq, ElevenLabs hết
credit (pre-check trước khi chạy kênh recap), file hỏng, trùng, thư mục
không khớp) / cảnh báo nguồn cạn (N ngày thư mục không có file mới).
Nhật ký ghi file `logs/pipeline_YYYYMMDD.log`.

## 8. Điều tool tải KHÔNG cần biết

Tool tải không cần biết gì về cắt/xuất/đăng — chỉ cần: tải video mới của
kênh theo dõi vào đúng thư mục đã gán, file hoàn chỉnh mới mang tên thật.
Hết trách nhiệm. (Việc file "biến mất" sau đó là bình thường — tool cắt xóa.)

## 9. Nguồn video khi kênh không đăng mới (tool tải)

- Mỗi kênh theo dõi có `source_mode`: "new" (chỉ video mới — mặc định,
  giữ nguyên hành vi cũ) | "picked" (hàng chờ user tích 🎯) | "auto"
  (🤖 tự vét kho: app tự chọn video VIEW CAO NHẤT chưa làm, quét kho tối
  đa 1 lần/ngày — `auto_fetch_date`); và `daily_limit` (1-3, mặc định 1);
  và `group` (nhóm/quốc gia để lọc UI, không ảnh hưởng dây chuyền).
- Mỗi ngày (theo ngày local, đếm `drip_date`/`drip_count` bền qua restart)
  watcher tự tải tối đa `daily_limit` video: video MỚI đăng chiếm suất
  trước, còn suất thì lấy theo `source_mode` (đầu hàng `picked` theo thứ
  tự user tích, hoặc video view cao nhất chưa làm với "auto").
- Video đã rót: rút khỏi `picked`, ghi vào `seen_ids` + `done_ids`
  (dialog kho đánh dấu "✅ đã làm" — không bao giờ tải trùng).
- Id video 2 bên UI/backend rút CÙNG MỘT CÁCH (`extract_video_id`:
  ?v= → /shorts/ /embed/ /video/ /v/ → nguyên URL) — sửa 1 nơi phải sửa nơi kia.
- Tool cắt KHÔNG cần biết video đến từ hàng chờ hay video mới — với nó
  mọi file trong thư mục kênh như nhau (mục 2-5 giữ nguyên).
