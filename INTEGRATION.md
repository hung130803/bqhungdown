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

## 5. Nhịp chạy (tool cắt) — SỬA 04/11/2026

- Có 2 cách chạy: bấm "▶ Chạy dây chuyền" (như cũ) và **Tự chạy** (mới).
  Tự chạy có công tắc, **mặc định TẮT**. Tắt = hành vi y như trước 04/11/2026.
- Tự chạy: trong khung giờ (mặc định 06:00–22:00), cứ N phút (mặc định 10)
  gọi ĐÚNG hàm mà nút ▶ / bot đang gọi (`_bot_chay_day_chuyen(nhóm đã lưu)`),
  không viết lại nghiệp vụ. Thêm 1 lượt 45 giây sau khi phiếu đợt tải (mục 11)
  ghi một video `xong`. Lượt trước còn đang quét/nhận file thì bỏ nhịp.
- Lượt tự chạy KHÔNG được nhảy màn hình (không gọi `_pipe_focus` /
  `_select_project`), KHÔNG mở hộp thoại, KHÔNG gọi mạng trên luồng giao diện.
- Thứ tự trong kênh: file có mtime CŨ NHẤT trước (giữ nguyên).

## 6. Cấu hình theo kênh (tool cắt)

- `pipe_on` (bật dây chuyền cho kênh), `pipe_src` (thư mục trung chuyển
  của kênh; mặc định `<gốc>\<Tên kênh>`), `pipe_mode` ("auto" = Tạo clip
  thường | "recap" = Reup thuyết minh), `pipe_daily` (1-2), `grp` (nhóm).
- Thư mục xuất Part dùng cấu hình sẵn có (export_dir của kênh).
- **Chạy THEO NHÓM:** hộp 🤖 Dây chuyền có bộ lọc nhóm; "▶ Chạy dây chuyền"
  chỉ xử lý kênh của NHÓM đang chọn (chọn "Tất cả nhóm" = mọi kênh bật).
  Nhóm ở tool cắt (`grp`) là để GOM/CHẠY THEO ĐỢT — **không ảnh hưởng
  thư mục handoff** (thư mục vẫn là `<gốc>\<Tên kênh>` theo mục 1). Nhóm
  của tool cắt độc lập với nhóm bên tool tải; user tự gán trong bảng.
- **1 THƯ MỤC/KÊNH (mô hình hiện tại — KHÔNG còn "thư mục trung chuyển"
  chung):** MỖI kênh tự đặt THƯ MỤC LẤY VIDEO riêng (`export_dir`) qua nút
  📂 ở cột phải bảng 🤖 Dây chuyền. Thư mục đó VỪA là nơi tool tải thả video
  vào VỪA là nơi xuất: Part cắt xong xuất THẲNG vào thư mục đó (KHÔNG tạo
  thư mục con `\Clip`), file Part đặt tên "Part N …"; scan bỏ qua file khớp
  `^Part\s*\d` nên KHÔNG cắt lại Part. Video gốc XÓA sau khi đủ Part; file
  hỏng chuyển `_Loi`. `pipe_src` (chọn tay riêng) vẫn ưu tiên cao nhất, rồi
  tới `export_dir`. Kênh CHƯA đặt thư mục → báo "CHƯA đặt thư mục lấy video"
  (không xử lý oan). KHÔNG giới hạn video/ngày — có video là xử lý hết.
  "Làm lại": `reset_channel` xóa sổ `pipeline_files` của kênh → cho cắt lại;
  nút 🔄 làm lại 1 kênh hoặc cả nhóm. Bảng có Bật/Tắt tất cả + combo/ô số
  chống cuộn nhầm (NoWheel).

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

## 10. Kênh đích nhập tên + chống lưu nhầm khi tải song song (tool tải)

- `Settings.watch_root` = thư mục trung chuyển gốc (chọn 1 lần). Mỗi kênh
  theo dõi có `target_name` = TÊN KÊNH ĐÍCH user gõ (kênh TikTok của họ)
  → video tự về `<watch_root>\<target_name đã làm sạch ký tự cấm Windows>`.
- MỘT hàm duy nhất `watcher::resolve_watch_folder` quyết định thư mục cho
  MỌI đường tải (video mới / hàng chờ / tự vét / tải tay pending), ưu tiên:
  `dest_dir` chọn tay 📁 > `watch_root + target_name` > thư mục mặc định.
  Thư mục CHỐT NGAY LÚC ENQUEUE (mỗi job giữ save_folder riêng) → nhiều
  kênh tải song song không thể lẫn thư mục của nhau.
- Tên thư mục sinh từ `target_name` phải TRÙNG tên kênh trong tool cắt
  (mục 1). Sanitize: thay `<>:"/\|?*` bằng khoảng trắng, bỏ chấm/trắng cuối.

## 11. Phiếu đợt (thêm 04/11/2026)

Phiếu chỉ để BÁO CÁO và KÍCH HOẠT SỚM, **không bao giờ là điều kiện bắt
buộc**. Nguồn sự thật vẫn là file trong thư mục kênh theo mục 2. Phiếu mất,
hỏng hay đến trễ thì video vẫn được cắt nhờ quét định kỳ.

**Vị trí:** cài đặt chọn được ở cả 2 app, mặc định
`C:\ProgramData\BQDayChuyen\`. Phiếu tải ở `dot\<dot_id>.json` (chỉ
bqhungdown ghi). Phiếu cắt ở `cat\<dot_id>.json` (chỉ BQ Hung Video ghi).
KHÔNG BAO GIỜ đặt phiếu trong thư mục kênh (tool cắt `collect_wipe` xoá
mọi file cấp 1 ở đó; bộ dọn rác tool tải quét thư mục kênh).

**Phiếu đợt tải** — mỗi nhóm mỗi lần chạy Lịch tải 1 phiếu, `dot_id` = `YYYYMMDD-NN`:

```json
{"schema":1, "dot_id":"20261104-01", "nhom":"Mỹ mới", "seq":17,
 "mo_luc":"2026-11-04T06:00:12+07:00", "han_chot":"2026-11-04T09:00:12+07:00",
 "trang_thai":"dang_tai", "dong_luc":null, "ly_do_dong":null,
 "kenh":[{"thu_muc":"D:\\video ssmatool\\video mỹ\\kênh 3", "ten_dich":"...",
          "video":[{"id":"youtube:abc123", "trang_thai":"xong", "file":"Tên video.mp4",
                    "size":123456789, "xong_luc":"2026-11-04T06:07:40+07:00", "loi":null}]}],
 "kenh_khong_co_video":["D:\\...\\kênh 9"]}
```

- `trang_thai` phiếu: `dang_tai` → `da_dong`. `ly_do_dong`: `xong` | `het_han`
  (qua `han_chot`, mặc định 3 giờ) | `huy`. Phiếu đã đóng không bao giờ mở lại.
- `trang_thai` video: `cho` | `dang_tai` | `xong` | `loi` | `bo_qua` (live,
  18+, không phải video — kèm lý do) | `tre` (chưa xong khi đóng phiếu; vẫn
  tải tiếp, xong thì vẫn được cắt).
- Chỉ ghi `xong` khi file đã có TÊN CUỐI CÙNG trong thư mục kênh (sau cả bước
  đổi tên Douyin), lấy từ mục queue đã Completed.
- Id job của đợt lấy trực tiếp từ đường enqueue của chính đợt đó, KHÔNG đoán
  bằng cách so danh sách hàng trước/sau. Phiếu tự lưu danh sách id của mình.

**Phiếu đợt cắt** — cùng `dot_id`, cùng danh sách video. Trạng thái mỗi video:
`chua_thay` | `dang_lam` | `xong` (kèm số Part) | `loi` (kèm lý do) | `trung` |
`mat_file` | `kenh_tat` (kênh có `pipe_on=0`) | `khong_co_kenh` (thư mục
không thuộc kênh cắt nào). `mat_file` CHỈ khi phiếu tải ghi `xong` mà sau 10
phút file không có trong thư mục VÀ không có trong sổ `pipeline_files`.
Video `tre` xong sau khi phiếu đóng vẫn cắt và ghi vào phiếu cắt đợt cũ.

**Báo cáo nhóm:** khi cả 2 phiếu của một nhóm đã đóng, bot của BQ Hung Video
gửi ĐÚNG 1 tin (khởi động lại app không gửi lại).

## 12. Trạng thái bận (thêm 04/11/2026 — áp dụng cả PCBot)

- Mỗi app ghi 1 file của mình: `C:\ProgramData\PCBot\exchange\ban_bqhungdown.json`,
  `C:\ProgramData\PCBot\exchange\ban_bqhungvideo.json`.
  Nội dung: `{"app":"bqhungvideo","ban":true,"cho":12,"dang_lam":3,
  "cap_nhat":"2026-11-04T10:00:00+07:00","pid":1234,"phien_ban":"2.x","ly_do":null}`.
- Ghi mỗi 60 giây. BQ Hung Video ghi từ luồng giao diện (app treo thì file cũ đi).
  Thoát êm thì ghi lần cuối `ban=false`. Thư mục không tồn tại hoặc lỗi quyền
  ghi thì bỏ qua im lặng (ghi log 1 lần), không ảnh hưởng việc chính.
- bqhungdown bận khi: còn mục queued/downloading/paused, hoặc còn đợt đang mở.
- BQ Hung Video bận khi: còn job pending/running, hoặc còn file trong hàng nhận,
  hoặc đang quét nền.
- PCBot: agent đọc 2 file CHỈ qua `safe_exchange_path` (+ giới hạn kích thước,
  try/except), gửi cờ lên ESP32 trong stats. **Firmware quyết định** việc hoãn
  lịch ngủ đông/sleep/tắt máy theo lịch (lệnh bấm tay KHÔNG bị chặn). File cũ
  hơn 3 phút = app không chạy/treo → không coi là bận, báo Telegram. App bận
  → hoãn 15 phút, mỗi lần hoãn phải cảnh báo lại + nút bỏ qua, tối đa 2 giờ,
  rồi hỏi qua Telegram, KHÔNG tự hành động.
- Dây chuyền chỉ dùng NGỦ ĐÔNG. Không dùng lệnh tắt máy cho dây chuyền
  (`shutdown /t 10` ép đóng app).

## Luật đọc/ghi chung cho mọi file ở mục 11–12

1. Ghi nguyên tử: ghi `<tên>.json.tmp` cùng thư mục → flush → đổi tên đè.
   Đổi tên vướng khoá (bên kia đang đọc) thì thử lại 5 lần × 200 ms. File tạm
   KHÔNG mang đuôi video hay `.part`/`.ytdl`.
2. Bên đọc: đọc hết rồi đóng file ngay. JSON hỏng → giữ bản đọc được lần
   trước, KHÔNG coi là rỗng; hỏng 3 lần liên tiếp thì báo.
3. `schema` lớn hơn số mình hiểu → bỏ qua phiếu + báo 1 lần, vẫn cắt theo quét.
4. Khớp kênh THEO THƯ MỤC, không theo tên nhóm (tên nhóm 2 bên khác chữ:
   MỸ/Mỹ, Mỹ mới/Mỹ mới 1, HÀN/Hàn). Chuẩn hoá trước khi so: Unicode NFC,
   `/` → `\`, chữ thường, bỏ `\` cuối. (DB tool cắt lưu `D:/video ssmatool/...`,
   tool tải lưu `D:\video ssmatool\...` — so nguyên chuỗi thì lệch 154/154.)
   Ca mẫu dùng chung cho test D18 (tool tải) và V18 (tool cắt):
   `D:/video ssmatool/video mỹ/kênh 3`, `D:\video ssmatool\video mỹ\kênh 3\`,
   `d:\VIDEO SSMATOOL\video mỹ\kênh 3` (dạng NFD của "mỹ") → cùng một khoá.
5. Thời gian ISO 8601 có múi giờ (`+07:00`). Ngày của đợt theo giờ máy.
6. Mỗi bên tự xoá phiếu của mình cũ hơn 14 ngày, chỉ xoá file đúng mẫu tên.
7. Không bao giờ ghi key, token, mật khẩu vào bất kỳ file nào ở mục 11–12.
