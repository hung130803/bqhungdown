# Quy trình dây chuyền tự động: bật máy → tải → cắt

Nov 4, 2026 · @Hwhw Hej

## Tóm tắt

Làm được, với mô hình **"chảy liên tục + chốt theo nhóm"**. Tool tải tải lần lượt từng nhóm theo thứ tự anh chọn. Tool cắt nhận từng video ngay khi file tải xong, ưu tiên nhóm đến trước. Mỗi nhóm có một **phiếu đợt** để hai bên báo "nhóm X đã tải xong / đã cắt xong".

Không dùng kiểu "chờ cả nhóm tải xong mới cắt". Lý do: một video bị YouTube chặn bot sẽ nằm chờ thử lại tới 30 lần (khoảng 1,5 giờ với cài đặt hiện tại, 5 giờ với mặc định) và giữ cả nhóm đứng theo. Trong lúc đó tool cắt, khâu chậm nhất, lại ngồi không.

Đo trên máy thật: mỗi ngày tool cắt làm xong 120–131 video, chạy 6–10 giờ. Phân tích một video mất trung vị 8,6 phút (10% chậm nhất trên 25,8 phút), xuất Part mất trung vị 4,1 phút. Có 137 kênh bật dây chuyền, nên khung giờ tự chạy phải dài ít nhất khoảng 10 giờ.

**Anh Hùng cần duyệt 6 việc trước khi giao sửa:**

1. Mô hình chảy liên tục + phiếu đợt theo nhóm (mục So sánh mô hình).
2. Giờ bật máy, thứ tự nhóm tải, khung giờ tự cắt (mục Luồng đề xuất).
3. Bật tự đăng nhập Windows. Không có bước này thì cả dây chuyền không chạy.
4. Cuối ngày ngủ đông hay để máy chạy. Khuyên ngủ đông, và chỉ ngủ khi cả 2 app báo rảnh.
5. Xử lý 18 kênh Nhật: bên tải vẫn tải nhưng bên cắt đang tắt dây chuyền. Mỗi kênh có 1 video nằm chờ 42 ngày.
6. Kênh 39 (TicsTripsTruths) có tải nhưng không có kênh cắt. Kênh 47 (Jake Martin) có kênh cắt nhưng không có nguồn tải.

## Hiện trạng 3 tool (đọc từ code, đo ngày 04/11/2026)

Mỗi tool đã làm tốt phần của mình. Chỗ hở nằm ở **khâu nối**: không tool nào biết tool kia đang làm gì, chỉ nhìn thấy file trong thư mục.

| Tool | Đã có sẵn | Chưa có (cần cho tự động) |
| --- | --- | --- |
| PCBot (ESP32 + agent) | Hẹn giờ bật/tắt/ngủ đông, tối đa 8 lịch, lưu trong ESP32. Biết máy bật hay tắt, ai đang đăng nhập. Cảnh báo 5 phút trước khi tắt, có nút bỏ qua. | Không kiểm tra app còn bận trước khi tắt. Không gửi lại gói Wake-on-LAN nếu máy không lên. Lịch trễ quá 2 phút thì bỏ, không chạy bù. |
| Windows | BIOS có điện là tự bật, có Wake-on-LAN, Fast Startup đã tắt. BQ Hung Video và bot của bqhungdown nằm trong Startup. | **Không tự đăng nhập** (`AutoAdminLogon=0`). App bqhungdown không nằm trong Startup. Windows Update có thể tự khởi động lại giữa chừng. |
| bqhungdown (tải) | 155 kênh theo dõi, kênh nào cũng có thư mục đích, giới hạn 1 video/ngày, chế độ tự vét kho. File tải nằm trong thư mục ẩn `.bqd-temp`, xong mới chuyển ra (đúng giao kèo). Cổng điều khiển `127.0.0.1:47321`. | **Tự quét đang tắt** (`watchAutoEnabled=false`, nhịp 1 phút). Không có khung giờ, không chạy lần lượt theo nhóm. **Không có tín hiệu "nhóm tải xong"**: "đợt" hiện chỉ là bot đoán, trộn nhiều nhóm. |
| BQ Hung Video (cắt) | 137 kênh bật dây chuyền, chạy theo nhóm, chống chạy trùng lượt, tự hồi phục sau khi tắt app, chờ lượt Groq. Bot Telegram chạy được 1 nhóm từ xa (`/chay` → `/chayok`). | **Không tự chạy** (`studio_page.py:6669` ghi rõ "không tự chạy nền"). Chế độ "tự canh thư mục" ghi trong INTEGRATION.md nhưng **chưa làm**. Mỗi lượt chỉ quét thư mục 1 lần. |

**Đối chiếu thư mục 2 bên** (watchlist của tool tải so với `studio.db` của tool cắt):

| Kết quả | Số kênh | Chi tiết |
| --- | --- | --- |
| Khớp, bật cả 2 bên | 135 | Chạy được ngay |
| Tải về nhưng bên cắt tắt dây chuyền | 18 | Toàn bộ nhóm Nhật (`video nhật dài\kênh 1`–18). Mỗi kênh 1 video nằm chờ 42,5 ngày. |
| Tải về nhưng không có kênh cắt | 1 | `video mỹ\kênh 39` (TicsTripsTruths, nhóm MỸ), 3 video nằm chờ |
| Có kênh cắt nhưng không có nguồn tải | 1 | `video mỹ\kênh 47` (Jake Martin, đang bật) |

Tên nhóm 2 bên **không trùng chữ**: bên tải là `MỸ`, `Mỹ mới`, `HÀN`, bên cắt là `Mỹ`, `Mỹ mới 1`, `Hàn`. Vì vậy hai bên phải khớp với nhau **theo thư mục kênh**, không theo tên nhóm.

Tồn đọng hiện tại: 112 video gốc chờ cắt trên 111 kênh, và 85 file Part chờ tool đăng.

## So sánh mô hình: chọn C (lai)

Khâu chậm là tool cắt (8,6 phút/video, phụ thuộc lượt Groq), còn tool tải thường chỉ mất vài phút/video. Vì vậy mô hình tốt nhất là mô hình **không bao giờ để tool cắt phải ngồi chờ**.

|  | A. Cả nhóm tải xong mới cắt | B. Tải xong file nào cắt file đó, không có nhóm | **C. Lai: cắt ngay từng file + phiếu đợt theo nhóm** |
| --- | --- | --- | --- |
| Tool cắt bắt đầu | Sau khi video cuối cùng của nhóm tải xong | Khi file đầu tiên đứng yên (≥ 40 giây) | Khi file đầu tiên đứng yên |
| 1 video bị chặn bot (thử lại tới 30 lần) | Cả nhóm đứng 1,5–5 giờ | Không ảnh hưởng | Không ảnh hưởng. Phiếu ghi video đó là "trễ", tới hạn thì đóng đợt |
| Báo "nhóm X xong" | Có | Không, chỉ báo từng video | Có: tải xong N/N, cắt xong M Part, lỗi K |
| Phát hiện video tải về mà bị thiếu | Có | Không | Có: phiếu ghi "xong" mà không thấy file thì báo |
| Hỏng phiếu/tín hiệu thì sao | Dây chuyền đứng | Không liên quan | Vẫn cắt bình thường nhờ quét định kỳ, chỉ mất phần báo cáo |
| Lượng sửa code | Trung bình | Ít nhất | Trung bình |

**Nguyên tắc sống còn của mô hình C: phiếu đợt chỉ dùng để báo cáo và để kích hoạt cắt sớm. Nó không bao giờ là điều kiện bắt buộc.** Nguồn sự thật vẫn là file trong thư mục, theo luật "file đứng yên" đã có (mục 2 INTEGRATION.md). Phiếu mất, hỏng hay đến trễ thì video vẫn được cắt. Nhờ vậy một lỗi ở khâu tín hiệu không thể làm đứng cả dây chuyền.

Thứ tự nhóm vẫn có ích. Tool tải làm nhóm A trước nên video nhóm A về trước. Tool cắt lấy file cũ nhất trước nên cũng làm nhóm A trước. Nhóm A vì thế có Part sớm nhất mà không phải xếp hàng cứng.

## Luồng đề xuất trong một ngày

Mỗi ngày có 7 bước. Các giờ dưới đây là mặc định đề xuất, anh đổi trong cài đặt. Mọi mốc giờ đều là giờ máy (Việt Nam).

1. **05:50, PCBot bật máy.** Lịch `/hen` hằng ngày gửi Wake-on-LAN. Cần sửa: 3 phút chưa thấy máy lên thì tự gửi lại, tối đa 2 lần, rồi mới báo lỗi.
2. **\~05:52, Windows tự đăng nhập.** PCBot kiểm tra: 10 phút sau khi máy lên mà chưa có ai đăng nhập thì báo đỏ lên Telegram.
3. **\~05:53, 2 app tự mở.** bqhungdown mở thu nhỏ xuống khay, BQ Hung Video mở từ Startup. Khoảng 5 phút sau, mỗi app ghi file "trạng thái bận" lần đầu. Nếu đến 06:10 mà thiếu file của app nào, PCBot báo "app X chưa chạy".
4. **06:00, tool tải chạy Lịch tải.** Tải lần lượt từng nhóm theo thứ tự anh xếp, mỗi nhóm một phiếu đợt. Nhóm trước đã quét xong và xếp hết vào hàng thì chuyển sang nhóm sau, không chờ từng video tải xong. Hàng tải vẫn chạy 5 luồng như hiện tại, làm theo thứ tự vào trước ra trước, nên video nhóm A vẫn về trước. Mỗi đợt có hạn chót (mặc định 3 giờ). Hết hạn thì đóng phiếu, video chưa xong ghi là "trễ". Video trễ vẫn tải tiếp, xong thì vẫn được cắt.
5. **06:00–22:00, tool cắt Tự chạy.** Cứ 10 phút gọi đúng hàm mà nút ▶ Chạy dây chuyền đang dùng, cho tất cả kênh đang bật. Ngoài ra, khi phiếu đợt ghi một video "xong" thì chạy thêm 1 lượt sau 45 giây, để đủ luật đứng yên 30 giây + 10 giây. Hàm này vốn đã an toàn khi bị gọi lặp: file đang làm sẽ được bỏ qua.
6. **Khi một nhóm đóng ở cả 2 bên, báo Telegram 1 tin.** Ví dụ: "Nhóm Mỹ mới: tải 56/58 (2 trễ) · cắt xong 55 · lỗi 1 (lý do) · Part mới 165".
7. **23:00, PCBot ngủ đông nếu cả 2 app báo rảnh.** Còn app bận thì hoãn từng 15 phút, tối đa 2 giờ, rồi hỏi lại qua Telegram chứ **không tự ngủ**. Ngủ đông chỉ đóng băng tiến trình, không giết. Vì vậy không dùng lệnh tắt máy cho dây chuyền: lệnh `shutdown /t 10` ép đóng app sau 10 giây.

Mất điện giữa ngày vẫn được lo: có điện lại thì BIOS tự bật máy, 2 app tự mở và tự hồi phục. Tool cắt nối lại job sau 4 giây, tool tải nạp lại hàng. Với tool tải, đợt đang mở trong ngày được nối tiếp chứ không mở đợt mới.

## Giao kèo tín hiệu mới (bổ sung vào INTEGRATION.md mục 11, 12)

Hai tool chỉ nói chuyện với nhau qua file, không qua HTTP. Vì vậy app này tắt thì app kia vẫn chạy, và bật lại đọc tiếp được. Có 3 loại file, **mỗi loại chỉ một bên được ghi**:

| File | Bên ghi | Bên đọc | Vị trí mặc định |
| --- | --- | --- | --- |
| Phiếu đợt tải | bqhungdown | BQ Hung Video, bot | `C:\ProgramData\BQDayChuyen\dot\<dot_id>.json` |
| Phiếu đợt cắt | BQ Hung Video | bot, anh Hùng | `C:\ProgramData\BQDayChuyen\cat\<dot_id>.json` |
| Trạng thái bận | mỗi app 1 file của mình | PCBot agent | `C:\ProgramData\PCBot\exchange\ban_bqhungdown.json`, `ban_bqhungvideo.json` |

Thư mục `BQDayChuyen` là cài đặt chọn được ở cả 2 app, không viết cứng. File trạng thái bận đặt trong thư mục `exchange` vì agent PCBot (chạy quyền SYSTEM) đã có sẵn cách đọc an toàn thư mục này (`safe_exchange_path`, chặn junction và hardlink). Người dùng thường ghi được file vào đó.

**Phiếu đợt tải**: mỗi nhóm mỗi lần chạy 1 file, `dot_id` = `YYYYMMDD-NN`.

```json
{"schema":1, "dot_id":"20261104-01", "nhom":"Mỹ mới", "seq":17,
 "mo_luc":"2026-11-04T06:00:12+07:00", "han_chot":"2026-11-04T09:00:12+07:00",
 "trang_thai":"dang_tai", "dong_luc":null, "ly_do_dong":null,
 "kenh":[{"thu_muc":"D:\\video ssmatool\\video mỹ\\kênh 3", "ten_dich":"...",
          "video":[{"id":"youtube:abc123", "trang_thai":"xong", "file":"Tên video.mp4",
                    "size":123456789, "xong_luc":"2026-11-04T06:07:40+07:00", "loi":null}]}],
 "kenh_khong_co_video":["D:\\...\\kênh 9"]}
```

- `trang_thai` của phiếu: `dang_tai` → `da_dong`. `ly_do_dong`: `xong` (mọi video đã có kết quả cuối), `het_han` (qua `han_chot`) hoặc `huy` (anh bấm huỷ). Phiếu đã đóng thì không bao giờ mở lại.
- `trang_thai` của video: `cho`, `dang_tai`, `xong`, `loi`, `bo_qua` (live, 18+, không phải video), `tre` (chưa xong khi đóng phiếu).
- Chỉ ghi `xong` khi file đã có **tên cuối cùng** trong thư mục kênh, kể cả bước đổi tên của Douyin.
- Bài ảnh TikTok ghi `bo_qua` kèm lý do "không phải video".

**Phiếu đợt cắt** dùng cùng `dot_id` và cùng danh sách video. Mỗi video có trạng thái `chua_thay`, `dang_lam`, `xong` (kèm số Part), `loi` (kèm lý do), `trung` hoặc `mat_file`. `mat_file` nghĩa là phiếu tải ghi `xong` mà sau 10 phút vẫn không thấy file. Video "trễ" xong sau khi phiếu đã đóng vẫn được cắt, và ghi vào phiếu cắt của đợt cũ.

**Trạng thái bận**: `{"app":"bqhungvideo","ban":true,"cho":12,"dang_lam":3,"cap_nhat":"…+07:00","pid":1234,"phien_ban":"2.x"}`. Ghi mỗi 60 giây từ **luồng giao diện**, để app treo thì file cũ đi và PCBot nhận ra.

- Tool tải bận khi còn mục đang chờ, đang tải hoặc tạm dừng, hoặc còn đợt đang mở.
- Tool cắt bận khi còn job đang chờ hoặc đang chạy, hoặc còn file trong hàng nhận.
- File cũ hơn 3 phút nghĩa là app không chạy hoặc bị treo. PCBot báo lên Telegram, và không coi là "bận".

**Luật đọc ghi bắt buộc (áp dụng cho cả 3 loại file):**

1. **Ghi nguyên tử.** Ghi ra `<tên>.tmp` cùng thư mục, flush xuống đĩa, rồi đổi tên đè lên file thật. Không bao giờ ghi thẳng vào file đang có người đọc. Nếu đổi tên vướng khoá (do đọc trùng lúc) thì thử lại 5 lần, mỗi lần cách 200 ms.
2. **Đọc ra JSON hỏng thì không coi là rỗng.** Giữ bản đọc được lần trước. Hỏng 3 lần liên tiếp thì báo.
3. **`schema` lớn hơn số mình hiểu thì bỏ qua phiếu và báo**, nhưng vẫn cắt theo quét thư mục.
4. **Khớp kênh theo thư mục, không theo tên nhóm.** Trước khi so, cả 2 bên chuẩn hoá đường dẫn: Unicode NFC, đổi `/` thành `\`, chữ thường, bỏ `\` cuối. Đo thật cho thấy việc này là bắt buộc: DB của tool cắt lưu `D:/video ssmatool/...`, còn tool tải lưu `D:\video ssmatool\...`. So nguyên chuỗi thì **cả 154 kênh đều lệch**.
5. **Thời gian ghi theo ISO 8601 có múi giờ** (`+07:00`). Ngày của đợt tính theo giờ máy.
6. **Dọn dẹp:** mỗi bên tự xoá phiếu của mình khi đã cũ hơn 14 ngày, và chỉ xoá file đúng mẫu tên.

## Bảng lỗ hổng: 30 chỗ, 9 chỗ chặn đứng cả ngày

Mỗi dòng đều có bằng chứng từ code hoặc từ số đo trên máy. Mức: **Đ** = đứng cả dây chuyền, **S** = sai kết quả hoặc mất video, **N** = phí tài nguyên hoặc báo cáo sai. Bên sửa: **D** = bqhungdown, **V** = BQ Hung Video, **P** = PCBot, **H** = anh Hùng tự cài.

| # | Mức | Chỗ hở | Bằng chứng | Hệ quả nếu không sửa | Cách xử lý bắt buộc | Bên |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Đ | Windows không tự đăng nhập | Registry `AutoAdminLogon=0` | Máy lên nhưng dừng ở màn hình đăng nhập, 2 app không mở | Cài Autologon (Sysinternals), anh tự nhập mật khẩu. PCBot báo nếu sau 10 phút vẫn chưa đăng nhập | H, P |
| 2 | Đ | Tool cắt không tự chạy | `studio_page.py:6669`; không có timer hoặc theo dõi thư mục | Video nằm chờ đến khi có người bấm | Thêm Tự chạy: khung giờ + nhịp 10 phút + kích theo phiếu. Gọi `_bot_chay_day_chuyen(nhóm đã lưu)` từ QTimer của MainWindow. Lượt tự chạy không được nhảy màn hình (`_pipe_focus`) | V |
| 3 | Đ | App bqhungdown không tự mở | Thư mục Startup chỉ có bot của nó | Không tải gì | Tùy chọn "mở cùng Windows, thu nhỏ xuống khay". Bot đang chạy thì tự `/openapp` nếu app chưa chạy | D |
| 4 | Đ | Tự quét của tool tải đang tắt, và không có khung giờ | `watchAutoEnabled=false`; `watcher.rs:2041` | Không tải gì | Thêm Lịch tải (giờ bắt đầu + danh sách nhóm theo thứ tự). Khi bật Lịch tải thì tắt nhịp quét cũ, không chạy cả hai | D |
| 5 | Đ | BQ Hung Video có thể kẹt ở màn đăng nhập Supabase | `main.py:223-234`; `/moapp` dựa vào mật khẩu đã lưu | Phiên đăng nhập hết hạn thì mất cả ngày, không ai biết | Bot gác cổng báo đỏ nếu app nằm ở màn đăng nhập quá 5 phút | V |
| 6 | Đ | WoL chỉ gửi 1 lượt, không gửi lại | `pcbot_esp32.ino:1077-1081` (chỉ báo) | Gói bị rơi thì cả ngày không bật máy | Tự gửi lại sau 3 và 6 phút, sau đó mới báo lỗi | P |
| 7 | Đ | Lịch trễ quá 2 phút thì bị bỏ | `extras.ino:494-518`, `FIRE_GRACE_S=120` | ESP32 mất điện hay mất Wi-Fi đúng 05:50 thì hôm đó không chạy | Riêng lịch BẬT máy: cho chạy bù trong 60 phút. Lịch tắt/ngủ giữ nguyên, không chạy bù | P |
| 8 | Đ | Windows Update tự khởi động lại | Không có xử lý nào (PCBot chỉ báo sự kiện 1074) | Restart giữa ngày, tool tải xoá file tạm nên video đang tải phải tải lại từ đầu | Đặt Active hours 05:00–23:00, hoặc chính sách không tự restart khi có người đăng nhập. App tự hồi phục (đã có) | H |
| 9 | Đ | Lệnh tắt máy ép đóng app sau 10 giây, không hỏi app có đang bận không | `pcbot_agent.py:924-945` (`shutdown /t 10` tự kéo theo `/f`) | Giết ffmpeg/yt-dlp giữa chừng, có thể để lại Part hỏng | Dây chuyền chỉ dùng ngủ đông. Thêm kiểm tra file trạng thái bận trước mọi lệnh ngủ/tắt theo lịch (mục Luồng bước 7) | P |
| 10 | S | Mỗi lượt cắt chỉ quét 1 lần; file mới hơn 30 giây bị bỏ qua | `pipeline.py:30,111`; không có quét lại | Video về giữa lượt phải chờ người bấm | Nhịp 10 phút của #2. Kích theo phiếu thì chờ 45 giây sau `xong_luc` | V |
| 11 | S | Không có tín hiệu "nhóm tải xong"; "đợt" của bot trộn mọi nhóm, chờ 90 giây | `monitor.py:440-507`; `check_all` trả về ngay khi xếp hàng xong (`watcher.rs:2023`) | Không báo đúng theo nhóm, không phát hiện video thiếu | Phiếu đợt (mục Giao kèo). App giữ danh sách id job của từng đợt, không đoán theo `saveFolder` | D |
| 12 | S | Tên nhóm 2 bên khác chữ (MỸ/Mỹ, Mỹ mới/Mỹ mới 1, HÀN/Hàn) | `settings.json` so với `projects.grp` | Nếu khớp theo tên nhóm thì cả nhóm không chạy | Khớp theo thư mục kênh. Tên nhóm chỉ dùng để hiển thị | D, V |
| 13 | S | Đường dẫn 2 bên khác dấu gạch (`/` và `\`) | Đo: so nguyên chuỗi lệch 154/154 kênh, chuẩn hoá xong còn lệch 1 | Khớp phiếu theo chuỗi thì toàn bộ báo "không khớp kênh" | Một hàm chuẩn hoá chung (luật 4 mục Giao kèo), có test riêng | D, V |
| 14 | S | Douyin: file ra thư mục kênh với tên mã CDN rồi mới đổi sang tên thật | `queue.rs:1062-1088` | Bình thường đổi tên xong trong vài giây, luật 30 giây che được. Nhưng nếu đổi tên chậm hoặc lỗi, tool cắt nhận file tên mã CDN; lúc đổi tên sau đó sẽ vướng file đang bị đọc, và phiếu ghi tên không khớp | Đổi tên ngay trong `.bqd-temp` trước khi chuyển ra. Chỉ ghi `xong` sau bước đổi tên | D |
| 15 | S | Video 18+ lỗi thì được hoàn suất và gỡ khỏi danh sách đã thấy | `watcher.rs:197-205`; 18+ không thử lại (`queue.rs:1250`) | Chế độ tự vét (view cao nhất) có thể chọn lại đúng video đó mỗi ngày, kênh không bao giờ có video | Giữ tạm như video live (`hold_or_skip_live`), không bỏ hẳn, vì sửa cookie xong là tải được. Ghi `bo_qua` vào phiếu, báo "cookie 18+ lỗi". Làm ở Đợt 2 | D |
| 16 | S | Video bị chặn bot nằm chờ tới 30 lần × thời gian nghỉ | `queue.rs:1215-1247`, hiện khoảng 1,5 giờ | Nếu chờ cả nhóm thì nhóm treo | Không dùng rào chắn. Phiếu có `han_chot`, quá hạn thì ghi `tre` | D |
| 17 | S | 18 kênh Nhật vẫn tải nhưng bên cắt tắt dây chuyền | Đo: mỗi kênh 1 video nằm 42,5 ngày | Phí băng thông và ổ đĩa. Phiếu báo "tải xong" nhưng không bao giờ "cắt xong" | Anh chọn: tắt tải hoặc bật cắt. Phiếu cắt ghi `kenh_tat` để báo cáo không tính là lỗi | H, V |
| 18 | S | Kênh 39 tải về mà không có kênh cắt; kênh 47 có kênh cắt mà không có nguồn | Đo: kênh 39 có 3 video nằm chờ | Video tồn mãi; kênh 47 cảnh báo "nguồn cạn" | Anh sửa cấu hình. Tool cắt ghi `khong_co_kenh` vào phiếu và báo mỗi ngày | H, V |
| 19 | S | Kiểm tra ElevenLabs chỉ cảnh báo, và bị bỏ qua trên đường chạy từ bot | `studio_page.py:6130-6145`, `6804` | Kênh thuyết minh lỗi giữa chừng. Kiểm tra hiện tại còn hỏng hẳn: gọi sai tên hàm nên không bao giờ chạy. Hiện 0 kênh thuyết minh nên rủi ro thấp | Đường Tự chạy phải kiểm tra, thiếu credit thì bỏ qua riêng các kênh thuyết minh và báo | V |
| 20 | S | Không kiểm tra ổ trống trước khi tải hoặc cắt | Chỉ bot báo khi dưới 20 GB (`tele_bot.py:38`); ổ C từng còn 3,19 GB | Xuất hỏng giữa chừng, DB có thể vỡ | Trước mỗi lượt: ổ chứa video dưới 30 GB hoặc ổ C dưới 10 GB thì dừng tự chạy và báo | D, V |
| 21 | S | Hẹn thử lại AI 15/30/45 phút chỉ nằm trong bộ nhớ | `studio_page.py:7565-7633` | Restart là mất, video nằm im | Nhịp 10 phút của #2 nhặt lại. Thêm test: khởi động lại thì video lỗi AI được làm lại trong 1 nhịp | V |
| 22 | S | Tool cắt dọn gốc sang Thùng rác, phiếu tải vẫn ghi `xong` | `pipeline.py:669` | Dễ báo nhầm `mat_file` | `mat_file` chỉ khi file không có trong thư mục **và** không có trong sổ `pipeline_files` | V |
| 23 | S | Ghi phiếu dở dang khi mất điện | Thiết kế mới | JSON hỏng, đọc ra rỗng thì coi nhầm là không có video | Luật 1–2 mục Giao kèo | D, V |
| 24 | S | Đợt chạy qua nửa đêm | `drip_date` theo ngày máy | Nếu có lượt quét sau 00:00 (bấm tay, hoặc quét lại sau khi hoàn suất) thì kênh được thêm suất của ngày mới, tải 2 video | Lưu ngày xếp hàng cho từng video đang chờ. Video xong sau 00:00 tính vào ngày xếp hàng, không ăn suất hôm nay. Làm ở Đợt 2 | D |
| 25 | N | Nhịp quét hiện đặt 1 phút | `settings.json`: `watchIntervalMin=1` | Bật lại thì quét 155 kênh mỗi phút, dễ bị YouTube chặn bot | Lịch tải thay thế. Nếu giữ kiểu cũ thì tối thiểu 30 phút | D, H |
| 26 | N | Bài ảnh TikTok ghi thẳng thư mục con, không qua file tạm | `tiktok_photo.rs:97-160` | Tool cắt không đụng tới (chỉ quét cấp 1), nhưng rác nằm mãi | Watcher không tải bài ảnh cho kênh dây chuyền; phiếu ghi `bo_qua` | D |
| 27 | N | Kiểm tra file đứng yên chạy lại ffprobe và băm mỗi 100 ms cho tới khi đủ 10 giây | `studio_page.py:7044-7047` | Tốn CPU khi có nhiều file cùng về | Giữ kết quả soi trước nếu chữ ký size/mtime chưa đổi; thử lại sau 10 giây thay vì 100 ms | V |
| 28 | N | 3 bot Telegram riêng (PCBot, tải, cắt) | Code 3 dự án | Tin báo chồng nhau, khó đọc | Báo cáo nhóm chỉ gửi từ bot cắt (bên biết cả 2 phiếu); bot tải bỏ tin "đợt" cũ khi Lịch tải bật | D, V |
| 29 | N | Hai lượt quét chồng nhau chờ khoá tới 10 phút | `watcher.rs:397-407` | Đợt sau bị chậm | Lịch tải chạy tuần tự; bấm tay trong lúc Lịch đang chạy thì báo "đang chạy đợt X" | D |
| 30 | N | `/chayok` của bot cắt dựa vào danh sách nhóm trong bộ nhớ | `tele_bot.py:977` | Bot khởi động lại là bấm nút cũ báo "danh sách đã cũ" | Không ảnh hưởng Tự chạy; để nguyên | — |

**Những thứ đã kiểm tra và an toàn, không cần sửa:** file tải dở nằm trong `.bqd-temp` ẩn, tool cắt không quét vào đó. Không có bước nào ghi lại file sau khi đã có tên cuối (trừ Douyin, #14). Bấm Chạy chồng lượt ở tool cắt đã có 7 ca test. Không có kênh cắt nào dùng chung thư mục. Cả 154 thư mục tải đều tồn tại trên đĩa. Ngủ đông chỉ đóng băng tiến trình, không giết.

## Nguyên tắc không ảnh hưởng chất lượng

Phần tự động được **thêm vào cạnh** code đang chạy tốt, không sửa vào đó. Tắt công tắc thì mỗi app chạy y như hôm nay. Mục này viết từ 3 bản phân tích ảnh hưởng. Mỗi bản xác định: chỗ chèn code an toàn, những test đang bảo vệ chỗ đó, và những lỗi cũ từng xảy ra ở đúng chỗ đó.

**6 luật chung cho cả 3 dự án:**

1. **Chỉ thêm, không viết lại.** Phần tự động gọi lại đúng các hàm nút bấm đang dùng. Không đổi kiểu trả về của hàm cũ: cần thêm dữ liệu thì bọc thêm hoặc truyền thêm tham số tuỳ chọn.
2. **Công tắc mặc định TẮT.** Cài đặt cũ vẫn nạp được (trường mới có giá trị mặc định). Bật công tắc không được làm gì ngay lập tức, vì test smoke của BQ Hung Video bấm mọi ô tick.
3. **Chụp nền trước khi sửa.** Chạy toàn bộ test hiện có, lưu kết quả vào nhánh. Test nào đỏ sẵn thì ghi lại, để sau này không đổ nhầm cho thay đổi mới. Sửa xong chạy lại: test cũ phải ra **y hệt** nền, cộng thêm test mới đạt.
4. **Đợt 1 chỉ làm phần tự động. Đợt 2 sửa lỗi lõi, mỗi lỗi một nhánh.** Anh duyệt xong lỗi này mới làm lỗi tiếp theo.
5. **Chạy thử 3 ngày với nhóm `test` (2 kênh)** rồi mới bật cho 137 kênh.
6. **Gặp chỗ tài liệu khác code thật thì dừng lại hỏi**, không tự đoán.

**Trạng thái nền đã biết (ghi lại trước khi sửa):**

- PCBot đang có thay đổi chưa commit trong 3 file firmware. Firmware ghi bản 1.7.3 nhưng commit cuối là 1.7.2. Phải commit hoặc lưu lại trước khi phiên kia làm.
- bqhungdown: script `scripts/test-live-filter.py` nhiều khả năng đang đỏ sẵn. Script kiểm tra rằng `was_live` không bị chặn, nhưng từ 31/10 app đã chặn `was_live`.
- BQ Hung Video: `_test_pipe_e2e.py` đang đỏ vì lý do font, không liên quan việc này. Bộ hồi quy đầy đủ chạy khoảng 50 phút. Có 14 cổng **không nằm trong** `_chay_hoi_quy.py` và phải chạy riêng.
- BQ Hung Video: **kiểm tra ElevenLabs hiện tại không bao giờ chạy.** Code gọi `eleven_keys()` nhưng config chỉ có `elevenlabs_keys`, nên danh sách key luôn rỗng. Code cũng đọc khoá `remaining` trong khi hàm trả về `remain` (`studio_page.py:6138-6140`, `dubbing.py:1223`). Đây là lỗi cũ, riêng biệt với việc tự động.

### bqhungdown: chỗ chèn và bẫy

| Việc thêm | Chỗ chèn an toàn | Test đang bảo vệ / phải mở rộng | Bẫy không được giẫm |
| --- | --- | --- | --- |
| Lịch tải | `spawn_scheduler` mới cạnh `spawn_monitor` (`watcher.rs:2029`). Gọi `check_all(Some(nhóm))` lần lượt. Chặn nhịp cũ bằng điều kiện ở `watcher.rs:2041`, không sửa giá trị `watch_auto_enabled` của anh | Test watcher 1361–1428 (hạn mức ngày, nguồn dự phòng, chạy đúng nhóm). Test settings 587, 669, 704 | Trường Settings mới phải có `#[serde(default)]` và sinh lại fixture (`BQD_UPDATE_FIXTURE=1`). Không giữ khoá `CHECKING` xuyên các nhóm. Không gọi `reconcile_all` từ lịch |
| Lấy id job của từng đợt | Thêm tham số tuỳ chọn (nơi gom id) vào `enqueue_new` (`watcher.rs:1282`). Giữ nguyên chữ ký cũ cho nút bấm, bot và monitor | — | Không đoán id bằng cách so danh sách hàng trước/sau, vì bot và bấm tay cũng thêm vào cùng lúc |
| Phiếu đợt | Module mới. Lấy đường dẫn cuối từ mục đã Completed (sau bước đổi tên Douyin). Phiếu tự lưu danh sách id của mình | Test queue 1508–1538 + test mới cho luật đóng phiếu | Hàng chỉ giữ 300 mục đã xong và bỏ mục xong khi khởi động lại, nên id không còn trong hàng thì tra lịch sử. Trạng thái "Failed" chưa phải là cuối cùng. File tạm không được mang đuôi video hoặc `.part` (bộ dọn rác 2 phút/lần sẽ xoá). Đổi tên đè có thể lỗi khi tool cắt đang mở file, nên phải thử lại |
| File trạng thái bận | Vòng 60 giây mới trong `lib.rs` setup, đọc `queue.list()` | Test mới cho bộ dựng JSON | Ghi `ban=false` lần cuối khi thoát êm. Lỗi quyền ghi ProgramData thì bỏ qua im lặng |
| Tự mở cùng Windows | Lối tắt Startup + tham số `--minimized`, ẩn cửa sổ trong setup (gần code khay `lib.rs:307`) | Test `bot_launcher`, `instance_lock` | Mở lần 2 là bật hộp thoại chặn: bot `/openapp` phải kiểm tra app đã chạy chưa. Socket cổng điều khiển phải giữ `no_inherit` (test 1055). Chỉ 1 bot |

### BQ Hung Video: chỗ chèn và bẫy

| Việc thêm | Chỗ chèn an toàn | Test đang bảo vệ / phải mở rộng | Bẫy không được giẫm |
| --- | --- | --- | --- |
| Tự chạy | QTimer mới thuộc **MainWindow**, cạnh `_bot_noi_giao_dien` (`main_window.py:130-150`). Gọi `_bot_chay_day_chuyen(nhóm đã lưu)` | Cổng 4 overlap, 5 lane, 6 shutdown, 7 huỷ, 14 mượt, `_test_tele_bot.py` CA 11 + cổng mới dùng đồng hồ giả, đăng ký vào `_chay_hoi_quy.py` | **Không treo vào `_poll_tick`**: nó dừng khi mở hộp chọn file và dừng hẳn khi DB vỡ. **Luôn truyền nhóm tường minh**: đọc combo có thể đọc nhầm combo của hộp thoại đã đóng. Phải có cờ "đang quét" riêng, vì `_bot_dang_nhan()` bằng 0 trong lúc quét nền |
| Không giật màn hình của anh | Cờ "không nhảy tới video" cho lượt tự chạy | Ca test mới | Mỗi lượt chạy hiện gọi `_pipe_focus`, có thể **đổi nhóm và đổi video đang xem**. Tự chạy 10 phút/lần mà không chặn thì màn hình anh bị giật liên tục |
| Đọc phiếu, ghi phiếu cắt, file bận | Đọc/ghi file qua `_chay_nen` (luồng nền). Nhịp 60 giây ở MainWindow, chỉ đọc số liệu trong RAM | Cổng 18 dọn rác, 77 lộ key, 99 ổ cứng, 100 WAL, 6 shutdown | **Không đặt file JSON trong thư mục kênh**: `collect_wipe` xoá mọi file cấp 1 ở đó. Không ghi key hoặc token vào file. Mọi emit từ luồng phải qua `safe_emit` |
| Cài đặt + báo cáo nhóm | Mục mới trong menu "⋮" của hộp Dây chuyền. Telegram gửi qua `_bot._viec` (luồng nền) | Cổng 1 pyflakes, 2 smoke (thêm hộp mới vào `HOP`), 3 pipe\_dialogs, 17 no\_popup, 97 tele\_bot | Nhãn nút bằng chữ, không emoji. `NoWheel*` import trong cùng hàm. Không dùng `Qt.Popup`. Test áp QSS thật. Bot đã có tin "HẾT hàng chờ", gộp chung để không báo trùng |
| Kiểm tra trước lượt | Dùng lại kiểm tra key AI và hàng tạm dừng có sẵn. Ổ đĩa: kiểm tra **từng ổ** chứa thư mục kênh. ElevenLabs: hàm dùng chung mới, chạy nền | Cổng 67, 77, 108 | Không bật hộp thoại nào trên đường tự chạy. Không gọi mạng trên luồng giao diện |

### PCBot: chỗ chèn và bẫy

| Việc thêm | Chỗ chèn an toàn | Test đang bảo vệ / phải mở rộng | Bẫy không được giẫm |
| --- | --- | --- | --- |
| Gửi lại WoL | Khối "chưa lên" ở `pcbot_esp32.ino:1079-1083`, bộ đếm nằm cạnh `wakeAt`, reset ở cả 4 chỗ đặt `wakeAt` | `esp-selftest` + hàm thuần mới có ca test | Cửa sổ 9 phút làm `TP_OFF_ALREADY` tưởng máy đang khởi động lâu hơn |
| Bù lịch BẬT 60 phút | `planTick` (`extras.ino:513`) **và** `nextFire` (`:103`), chỉ với `A_ON` | 3 ca selftest hiện có cho `nextFire` + ca mới | Đừng tăng `FIRE_GRACE_S` chung. Lịch tắt/ngủ phải giữ "không bao giờ chạy trễ". Lịch mới tạo vừa qua giờ vẫn không được chạy ngay |
| Kiểm tra app bận | **Quyết định trong firmware**, không trong agent. Agent đọc 2 file qua `safe_exchange_path` và gửi lên trong stats. Firmware truyền cờ vào `planTick`. Hoãn bằng `pendingFor`/`pendingAt` sẵn có | `test_agent_commands.py` + ca selftest mới | **Không thêm trường vào `struct Sched`**: sai kích thước là mọi lịch đã lưu bị xoá im lặng. Đặt trong agent thì chặn luôn lệnh bấm tay. Mỗi lần hoãn phải cảnh báo lại và giữ nguyên T (khoá của nút bỏ qua). Phải có `agentOnline()`. Không gửi thông báo mỗi nhịp (hàng tin chỉ có 12 chỗ) |
| Báo chưa đăng nhập | `check_alerts` của agent (`pcbot_agent.py:782`) | `test_agent_commands.py` | Khi `collect_stats` lỗi thì `user=""`: không được hiểu là chưa ai đăng nhập. Sau khi ngủ đông dậy, tính thời gian từ lúc thức chứ không tính uptime |

Lệnh chạy test nền của từng dự án nằm trong yêu cầu giao việc ở mục cuối.

## Ca test bắt buộc: bqhungdown (22 ca)

Mọi ca chạy tự động bằng `cargo test` hoặc test Python của bot. Test dùng thư mục tạm và đồng hồ giả, không đụng watchlist thật và không tải mạng thật. Riêng ca ghi "máy thật" thì làm tay lúc nghiệm thu.

| ID | Tình huống | Kết quả đúng |
| --- | --- | --- |
| D1 | Lịch tải bật, đến giờ bắt đầu | Chạy đúng 1 lần, làm các nhóm theo đúng thứ tự đã xếp |
| D2 | Mở app sau giờ bắt đầu (06:40, lịch 06:00) | Chạy ngay. Không chờ đến hôm sau |
| D3 | Mở app lại lần 2 trong cùng ngày | Nối đợt đang mở. Không tạo đợt mới, không cấp thêm suất |
| D4 | Lịch tải và nhịp quét cũ cùng bật | Không cho lưu cài đặt, hoặc nhịp cũ tự tắt kèm thông báo |
| D5 | Nhóm A có 58 kênh, 3 kênh không có video mới | Phiếu ghi 55 video + 3 kênh trong `kenh_khong_co_video` |
| D6 | Tất cả video nhóm A có kết quả cuối | Phiếu đóng với `xong`, `dong_luc` có giá trị, `seq` tăng |
| D7 | 1 video bị chặn bot, qua hạn chót | Phiếu đóng với `het_han`, video đó ghi `tre`. Video vẫn tải tiếp; xong thì file về đúng thư mục, phiếu đã đóng không bị sửa |
| D8 | Video live, chờ live, was\_live | Không tải, ghi `bo_qua` + lý do. Không chiếm suất ngày |
| D9 | Video 18+ lỗi 2 lần ở 2 ngày | (Đợt 2) Vào diện giữ tạm, không bị chọn lại mỗi ngày. Sửa cookie xong thì tải được |
| D10 | Douyin URL CDN | File ra thư mục kênh **chỉ với tên cuối**. Không lúc nào có file tên mã CDN ở cấp 1 |
| D11 | Bài ảnh TikTok trong kênh dây chuyền | Không tải, ghi `bo_qua` "không phải video" |
| D12 | 2 nguồn cùng 1 thư mục đích (nguồn dự phòng) | 1 video/ngày cho kênh đích, phiếu có 1 dòng kênh |
| D13 | Ghi phiếu, giết tiến trình giữa lúc ghi file `.tmp` | File phiếu thật vẫn là bản trước và đọc được. Mở lại thì dọn `.tmp` mồ côi |
| D14 | Một tiến trình khác đang mở đọc phiếu lúc đổi tên | Thử lại tối đa 5 lần, không crash, không mất cập nhật |
| D15 | Tắt app êm giữa đợt, mở lại | Đợt nối tiếp, video đang tải được tải lại. Phiếu không có video nào bị ghi 2 lần |
| D16 | App bị kill giữa đợt (giả mất điện) | Như D15, và không tải lại video đã `xong` |
| D17 | Đợt mở 23:30, chạy qua 00:00 | (Đợt 2) Video xếp hàng hôm qua mà xong sau 00:00 không ăn suất hôm nay |
| D18 | Đường dẫn có dấu, chữ Nhật, chữ Hàn, `/` lẫn `\`, `\` cuối | Hàm chuẩn hoá cho cùng một kết quả. Ca mẫu dùng chung với V18 |
| D19 | File trạng thái bận khi hàng rỗng, đang tải, tạm dừng | `ban` lần lượt false, true, true. `cap_nhat` mới trong vòng 60 giây |
| D20 | Thư mục `exchange` không tồn tại (chưa cài PCBot) | Không crash, ghi log 1 lần, vẫn tải |
| D21 | Ổ chứa video còn dưới ngưỡng | Không mở đợt mới, báo Telegram. File trạng thái ghi `ban=false` kèm lý do |
| D22 | Máy thật: khởi động Windows | App tự mở xuống khay, bot không bật trùng (khoá `bot.lock`), cổng 47321 không bị giữ |

Ngoài ra phải chạy lại toàn bộ test hiện có: `cargo test`, `python -m unittest discover -s tests` trong `telegram-bot`, `scripts/test-live-filter.py`.

## Ca test bắt buộc: BQ Hung Video (24 ca)

Viết thành file `_test_tu_chay.py` và `_test_phieu_dot.py`, có `import _test_guard` (cổng 17) và áp QSS thật nếu đụng giao diện. Test dùng đồng hồ giả và thư mục tạm. Quét thư mục, nhận file, ffprobe và hash chạy thật; chỉ thay AI bằng bản giả.

| ID | Tình huống | Kết quả đúng |
| --- | --- | --- |
| V1 | Tự chạy bật, trong khung giờ, tới nhịp | Gọi đúng hàm của nút ▶ đúng 1 lần, không mở hộp thoại |
| V2 | Ngoài khung giờ | Không chạy lượt mới. Job đang chạy vẫn làm nốt |
| V3 | Nhịp tới khi lượt trước đang nhận file | Bỏ nhịp này. Chỉ có 1 vòng nhận (giống CA4 của `_test_pipe_overlap`) |
| V4 | Nhịp tới khi hộp 🤖 Dây chuyền đang mở | Vẫn chạy (bài học cổng 14) |
| V5 | Nhịp tới khi đang mở hộp chọn file hệ thống (`_modal_busy`) | Hoãn tới khi hộp đóng, không crash |
| V6 | Hàng job đang tạm dừng (`tam_dung.flag`) | Không chạy. Ghi lý do vào log 1 lần mỗi giờ, không ghi mỗi nhịp |
| V7 | Không có key AI | Không chạy, báo Telegram 1 lần |
| V8 | File vừa về được 20 giây | Lượt này bỏ qua. Lượt kế (hoặc 45 giây sau khi kích theo phiếu) thì nhận |
| V9 | File đang lớn dần (giả tool tải ghi chậm) | Không nhận cho tới khi size đứng yên 10 giây và mtime cách hiện tại ≥ 30 giây |
| V10 | Phiếu tải ghi `xong`, file có trong thư mục | Nhận, phiếu cắt chuyển `dang_lam` rồi `xong` kèm số Part |
| V11 | Phiếu ghi `xong` nhưng sau 10 phút không có file, và không có trong sổ | Ghi `mat_file` và báo |
| V12 | Phiếu ghi `xong`, file đã bị tool cắt dọn sang Thùng rác | **Không** ghi `mat_file`, giữ `xong` |
| V13 | File có trong thư mục nhưng không có trong phiếu nào (tải tay) | Vẫn cắt bình thường |
| V14 | Phiếu JSON hỏng hoặc cụt nửa | Giữ bản đọc được lần trước, vẫn cắt theo quét. Hỏng 3 lần thì báo |
| V15 | Phiếu có `schema` = 2 | Bỏ qua phiếu, báo 1 lần, vẫn cắt |
| V16 | Video `tre` xong sau khi phiếu đã đóng | Cắt, ghi vào phiếu cắt của đợt cũ |
| V17 | Kênh trong phiếu có `pipe_on=0` (18 kênh Nhật) | Ghi `kenh_tat`. Báo cáo không tính là lỗi nhưng có nêu số lượng |
| V18 | Khớp thư mục: DB `D:/video ssmatool/...` so với phiếu `D:\video ssmatool\...\` | Khớp. Dùng chung ca mẫu với D18 |
| V19 | Thư mục trong phiếu không có kênh cắt (kênh 39) | Ghi `khong_co_kenh`, báo mỗi ngày 1 lần |
| V20 | Cả 2 phiếu của nhóm đã đóng | Gửi đúng 1 tin báo cáo nhóm. Khởi động lại app không gửi lại |
| V21 | Tắt app giữa lượt tự chạy, mở lại | Hồi phục như hiện có, không chạy trùng, video lỗi AI được làm lại trong 1 nhịp |
| V22 | Huỷ job trong lượt tự chạy | Nhịp sau **không** tự nhận lại (cổng 7 "huỷ là huỷ") |
| V23 | Ổ dưới ngưỡng; kênh thuyết minh mà ElevenLabs hết credit | Không chạy lượt mới / bỏ riêng kênh thuyết minh, báo rõ lý do |
| V24 | File trạng thái bận: có job, hết job, UI bị treo 5 phút | `ban` true → false. Khi treo thì `cap_nhat` cũ đi, không có luồng nền nào ghi hộ |

Ngoài ra phải chạy đủ các cổng test trong CLAUDE.md (pyflakes không còn "undefined name", `_test_app_smoke.py`, `_test_pipe_dialogs.py`, `_test_pipe_overlap.py` và các cổng còn lại). Nút cài đặt Tự chạy mới phải được `_test_app_smoke.py` bấm tới. Chữ trên nút dùng chữ, không dùng emoji dễ thiếu font (bài học v2.6.22).

## Ca test đầu-cuối và PCBot (16 ca)

Các ca P là test tự động của PCBot, chạy với ESP32 giả (`tests/test_agent_mock_esp.py`). Các ca E chạy trên máy thật với **2 kênh thử** trong nhóm `test`, không dùng kênh thật. Ca E1–E4 phải đạt 3 ngày liên tiếp thì mới bật cho 137 kênh.

| ID | Tình huống | Kết quả đúng |
| --- | --- | --- |
| P1 | Máy không lên sau lần WoL đầu | Gửi lại ở phút 3 và phút 6, phút 9 mới báo lỗi |
| P2 | ESP32 mất điện 05:45–06:20, lịch bật 05:50 | 06:20 bật bù và báo "bật trễ 30 phút" |
| P3 | Máy lên, 10 phút sau vẫn chưa có ai đăng nhập | Báo đỏ "Windows chưa đăng nhập" |
| P4 | 06:10 thiếu file trạng thái của 1 app | Báo "app X chưa chạy" |
| P5 | 23:00 ngủ đông, 1 app `ban=true` | Hoãn 15 phút, lặp lại. Tới 01:00 thì hỏi qua Telegram, không tự ngủ |
| P6 | 23:00 ngủ đông, file trạng thái cũ hơn 3 phút | Coi là không bận, ngủ, và báo "app X không phản hồi" |
| P7 | File trạng thái là junction, hardlink hoặc JSON hỏng | Bị `safe_exchange_path` từ chối, coi là "không rõ" và báo. Không crash |
| P8 | Bấm ⏭ Bỏ qua lần này khi đang hoãn vì app bận | Huỷ lịch hôm nay, không hoãn tiếp |
| E1 | Một ngày trọn vẹn: tắt máy từ tối hôm trước | 05:50 bật → đăng nhập → 2 app mở → tải 2 video → cắt ra Part → báo cáo nhóm `test` → 23:00 ngủ đông. Không ai chạm vào máy |
| E2 | Ngủ đông rồi bật lại (không phải bật nguội) | Như E1. App đã mở sẵn thì vẫn tự chạy đúng giờ vì đồng hồ nhảy |
| E3 | Rút điện máy lúc đang cắt | Có điện thì máy tự lên, video đang cắt được nối tiếp, không có Part hỏng trong thư mục xuất |
| E4 | Rút mạng 15 phút lúc đang tải | Video đang tải thử lại rồi xong. Tool cắt không nhận file dở |
| E5 | Anh bấm ▶ tay trong lúc Tự chạy đang chạy | Không cắt trùng, không nhân đôi job |
| E6 | Phiếu đợt bị xoá tay giữa ngày | Vẫn cắt hết video. Chỉ mất phần báo cáo nhóm đó, có báo |
| E7 | Tắt BQ Hung Video cả ngày | Tool tải vẫn tải và đóng phiếu. Mở tool cắt lại thì cắt bù hết. PCBot không bị chặn ngủ mãi |
| E8 | Windows Update đòi khởi động lại trong ngày | Không tự restart trong Active hours (kiểm tra cài đặt, không cần chờ update thật) |

## Thứ tự làm và yêu cầu giao cho từng dự án

Làm theo 6 bước. Bước 2 cho 3 dự án làm song song vì mỗi bên chỉ sửa code của mình. Chỗ duy nhất chạm nhau là giao kèo, và giao kèo chốt ở bước 1.

1. **Anh Hùng tự cài (hàng H trong bảng lỗ hổng):** tự đăng nhập Windows (#1), Active hours cho Windows Update (#8), xử lý 18 kênh Nhật (#17), kênh 39 và kênh 47 (#18).
2. **Chốt giao kèo:** phiên điều phối viết mục 11 (phiếu đợt) và mục 12 (trạng thái bận) vào `INTEGRATION.md`, thêm phần PCBot. Chép giống hệt vào cả 3 repo trong cùng một đợt. Sửa mục 5 (bỏ chữ "mặc định chỉ chạy khi bấm", thay bằng Tự chạy có công tắc, mặc định tắt).
3. **Đợt 1: 3 dự án làm song song phần tự động**, mỗi bên trên nhánh riêng `daychuyen-tu-dong`, chưa phát hành.
4. **Phiên điều phối duyệt:** đọc diff từng bên và đối chiếu với bảng lỗ hổng, chạy lại test của từng bên. Chạy ca khớp chung D18/V18 trên cả 2 bên với cùng dữ liệu mẫu.
5. **Chạy thử 3 ngày** với nhóm `test` (2 kênh), đạt E1–E8.
6. **Anh Hùng nghiệm thu**, rồi mới phát hành và bật cho toàn bộ 137 kênh.

**Yêu cầu dán vào phiên bqhungdown:**

```
[ĐỢT 1 — chỉ thêm phần tự động, không sửa lõi]
Đọc tài liệu "Quy trình dây chuyền tự động" (bản Markdown đính kèm), đặc biệt mục "Nguyên tắc không ảnh hưởng chất lượng" phần bqhungdown, và INTEGRATION.md mục 11-12 mới.
Nhánh daychuyen-tu-dong. KHÔNG push/phát hành khi tôi chưa duyệt.
BƯỚC 0 (bắt buộc): chạy nền và lưu kết quả vào nhánh: cd src-tauri && cargo test --lib ; npx tsc -b ; npx vitest run ; cd telegram-bot && python -m unittest discover -s tests ; python scripts/test-live-filter.py (có thể đỏ sẵn, ghi lại).
Làm các dòng: #3 tự mở cùng Windows, #4 Lịch tải, #11 phiếu đợt (lấy id qua tham số tuỳ chọn của enqueue_new), #12 #13 khớp theo thư mục + chuẩn hoá đường dẫn, #16 hạn chót đợt, #20 kiểm tra ổ đĩa, #25, #28, #29, file trạng thái bận.
KHÔNG làm ở đợt này: #14 Douyin, #15 18+, #24 nửa đêm, #26 bài ảnh (Đợt 2).
Mọi công tắc mới mặc định TẮT; trường Settings mới có #[serde(default)] + sinh lại fixture. Không đổi chữ ký check_all/check_watched_now/enqueue.
Test: D1-D8, D12-D16, D18-D22. Sau khi sửa chạy lại toàn bộ lệnh BƯỚC 0: test cũ phải ra y hệt nền.
Xong thì báo: tên nhánh, file đã sửa, kết quả nền vs sau sửa, chỗ làm khác tài liệu và vì sao. Tài liệu sai so với code thật: dừng và hỏi.
```

**Yêu cầu dán vào phiên BQ Hung Video:**

```
[ĐỢT 1 — chỉ thêm phần tự động, không sửa lõi]
Đọc tài liệu "Quy trình dây chuyền tự động" (bản Markdown đính kèm), đặc biệt mục "Nguyên tắc không ảnh hưởng chất lượng" phần BQ Hung Video, và INTEGRATION.md mục 11-12 mới.
Nhánh daychuyen-tu-dong. KHÔNG bump/tag khi tôi chưa duyệt.
BƯỚC 0 (bắt buộc): chạy nền và lưu kết quả: pyflakes; .venv\Scripts\python -u _chay_hoi_quy.py (không pipe qua tail); và riêng từng cổng KHÔNG nằm trong runner: _test_pipe_overlap, _test_shutdown_safety, _test_cancel_persist, _test_ui_smooth, _test_guard, _test_ai_gate, _test_db_corrupt_guard, _test_tpl_per_channel, _test_tpl_export_path, _test_don_rac, _test_pipe_integ, _test_100_kenh, _test_muot_tram_kenh, _test_pipe_e2e (đỏ sẵn vì font — ghi lại).
Làm các dòng: #2 Tự chạy (QTimer của MainWindow gọi _bot_chay_day_chuyen với nhóm đã lưu, có cờ "đang quét" riêng, KHÔNG nhảy màn hình _pipe_focus), #5, #10, #12, #13, #17, #18, #20 (kiểm tra từng ổ), #21, #22, #23, #28, phiếu cắt + file trạng thái bận.
KHÔNG làm ở đợt này: #19 sửa kiểm tra ElevenLabs đang hỏng, #27 (Đợt 2).
Không treo vào _poll_tick; không ghi JSON vào thư mục kênh (collect_wipe); không hộp thoại/mạng trên đường tự chạy; bật ô Tự chạy không được chạy ngay (smoke bấm mọi ô); nút bằng chữ, không emoji.
Test: V1-V22, V24, phần ổ đĩa của V23 (có import _test_guard), đăng ký cổng mới vào _chay_hoi_quy.py, mở rộng smoke/pipe_dialogs cho hộp cài đặt mới. Sau khi sửa chạy lại toàn bộ BƯỚC 0: test cũ phải ra y hệt nền.
Xong thì báo: tên nhánh, file đã sửa, kết quả nền vs sau sửa, chỗ làm khác tài liệu và vì sao. Tài liệu sai so với code thật: dừng và hỏi.
```

**Yêu cầu dán vào phiên PCBot:**

```
[ĐỢT 1 — chỉ thêm, hành vi cũ giữ nguyên khi chưa bật]
Đọc tài liệu "Quy trình dây chuyền tự động" (bản Markdown đính kèm), đặc biệt mục "Nguyên tắc không ảnh hưởng chất lượng" phần PCBot, và INTEGRATION.md mục 12.
TRƯỚC TIÊN: đang có thay đổi chưa commit trong pcbot_esp32.ino, tv.ino, tvadb.ino (FW 1.7.3) — hỏi tôi commit hay cất riêng, rồi mới tạo nhánh daychuyen-tu-dong.
Chưa nạp ESP32, chưa cập nhật agent khi tôi chưa duyệt.
BƯỚC 0: chạy nền test_agent_commands.py, test_agent_mock_esp.py, test_user_helper.py, esp-selftest (chỉ đọc), biên dịch thử firmware; lưu kết quả.
Làm các dòng: #1 báo chưa đăng nhập (agent check_alerts; user="" do lỗi KHÔNG phải chưa đăng nhập), #6 gửi lại WoL phút 3 và 6, #7 bù lịch BẬT 60 phút (planTick + nextFire, chỉ A_ON), #9 hoãn lịch ngủ/tắt khi app bận (quyết định trong FIRMWARE, agent chỉ gửi cờ qua stats; dùng pendingFor/pendingAt; mỗi lần hoãn phải cảnh báo lại).
KHÔNG thêm trường vào struct Sched, không đổi thứ tự enum Act, không tăng FIRE_GRACE_S chung, không ghi flash hoặc gửi tin mỗi nhịp.
Logic mới viết dạng hàm thuần + thêm ca vào esp-selftest. Tăng FW_VERSION và VERSION agent. Công tắc mặc định tắt qua settings.json/gen-header.
Test: P1-P8 + chạy lại toàn bộ BƯỚC 0: test cũ phải ra y hệt nền.
Xong thì báo: tên nhánh, file đã sửa, kết quả nền vs sau sửa, chỗ làm khác tài liệu và vì sao.
```

**Đợt 2, sửa lỗi lõi** (sau khi Đợt 1 đã chạy ổn 3 ngày; mỗi dòng một nhánh, anh duyệt xong dòng này mới làm dòng tiếp):

| Thứ tự | Dòng | Dự án | Ca test |
| --- | --- | --- | --- |
| 1 | #14 Douyin đổi tên trước khi chuyển ra | bqhungdown | D10 |
| 2 | #19 sửa kiểm tra ElevenLabs đang hỏng | BQ Hung Video | V23 (phần ElevenLabs) |
| 3 | #15 video 18+ giữ tạm thay vì lặp | bqhungdown | D9 |
| 4 | #24 suất ngày qua nửa đêm | bqhungdown | D17 |
| 5 | #26 bài ảnh TikTok trong kênh dây chuyền | bqhungdown | D11 |
| 6 | #27 giảm ffprobe lặp khi chờ file đứng yên | BQ Hung Video | thêm ca đo CPU |

**Tiêu chí nghiệm thu:** 62 ca test mới (D 22 + V 24 + P 8 + E 8) và mọi cổng test cũ của cả 3 dự án đều đạt. 30 dòng lỗ hổng đều có trạng thái "đã sửa" hoặc "anh chấp nhận giữ nguyên". 3 ngày chạy thử không ai phải chạm vào máy.

## Đợt 1b: nút điều khiển, chỉnh giờ, rà mọi lối chạy (bqhungdown, 04/11/2026)

Anh Hùng yêu cầu: mọi thứ có nút, chỉnh được giờ, rà mọi lối chạy (nhanh, chậm, tắt giữa chừng, bấm nhiều lần), và lỗi phải báo lên bot của CHÍNH app đó. Cùng nhánh `daychuyen-tu-dong`.

**Chuẩn điều khiển (giống cả 3 app).** Trong app VÀ qua bot đều có:

| Điều khiển | App (Cài đặt → Dây chuyền tự động) | Bot bqhungdown (`/lich`, menu 🗓 Lịch tải) |
| --- | --- | --- |
| Lịch tải Bật/Tắt | công tắc | ▶ Bật Lịch / ⏸ Tắt Lịch, `/lich bat`, `/lich tat` |
| Giờ bắt đầu | ô giờ | 🕐 −30 / +30 phút, `/lich gio 06:30` |
| Thứ tự nhóm | ↑ ↓ Bỏ + | ↑ từng nhóm, `/lich nhom MỸ, HÀN, -` (`-` = Chưa phân nhóm) |
| Hạn chót | ô số giờ | ⏳ −1 / +1 giờ, `/lich han 3` |
| Chạy ngay | ▶ Chạy ngay | ▶ Chạy ngay, `/lich chay` |
| Dừng lượt đang chạy | ⏹ Dừng lượt… (hỏi xác nhận, ô "huỷ job chưa tải") | ⏹ Dừng lượt… → Dừng / Dừng + huỷ job chưa tải / Thôi |
| Bỏ qua hôm nay | ⏭ Bỏ qua hôm nay | ⏭ Bỏ qua hôm nay, `/lich boqua` |
| Tin trạng thái tự sửa | khối tình trạng, 15 giây/lần | 1 tin bot tự sửa mỗi phút khi nội dung đổi |

Tin trạng thái gồm: đang làm nhóm nào, lần chạy tới lúc mấy giờ, lý do hoãn gần nhất, đợt đang mở, số video trễ, lỗi đang có. Cả app và bot đọc cùng `/api/daychuyen` (lệnh `daychuyen_status`) và gọi cùng lệnh Rust (`daychuyen_run_now`, `daychuyen_stop`, `daychuyen_skip_today`).

**Luật bấm:**

- Bấm nhiều lần chỉ chạy 1 lần: cờ `dang_quet` chiếm bằng compare-exchange; lượt thứ 2 báo "đang chạy rồi". Nút trong app khoá bằng ref trong lúc chờ.
- Nút trên tin bot CŨ hết hạn: mỗi bảng có mã riêng, gửi bảng mới là mã đổi. Bấm nút bảng cũ thì không làm gì, bot gửi bảng mới kèm lời "đã hết hạn".
- Đổi cài đặt có hiệu lực ở nhịp kế: lượt Lịch đọc lại cài đặt trước MỖI nhóm (thứ tự, hạn chót). Đã chạy xong hôm nay rồi mới dời giờ thì hôm nay không chạy lại.
- Giờ sai định dạng → app TỪ CHỐI cả lần lưu và báo rõ (trước đây `settings_store.rs` âm thầm bỏ qua). Danh sách nhóm rỗng → không cho bật, báo rõ.
- Dừng: không đụng file đang tải. Nhóm đang quét xong thì dừng, không xếp nhóm sau; phiếu đang mở đóng `ly_do_dong = huy`, video chưa xong ghi `tre`. Chọn "huỷ job chưa tải" thì chỉ huỷ job của đợt đang ở trạng thái Chờ (job đó ghi `loi` "đã huỷ"). Dừng = hôm nay không tự chạy lại; muốn chạy tiếp bấm Chạy ngay.
- Chạy ngay: chạy các nhóm HÔM NAY CHƯA QUÉT, bỏ qua giờ bắt đầu và lệnh Bỏ qua hôm nay, chạy được cả khi công tắc Lịch đang tắt. Nhóm đã quét hôm nay không quét lại; hết nhóm thì báo rõ.

**Việc đã sửa:**

| Mã | Việc | Cách sửa |
| --- | --- | --- |
| 1b-D1 | Đổi tên / xoá nhóm phải cập nhật `lichTaiNhom` | `WatchPage` gửi `lichTaiNhom` mới cùng lần lưu `watchGroups` (`lichSauDoiNhom`, giữ thứ tự). Nhóm trong Lịch không còn thì Lịch bỏ qua nhóm đó và báo bot (`nhom_mat`). |
| 1b-D2 | Nút + lệnh bot | `/lich` + menu 🗓 Lịch tải + nút trong Cài đặt nhanh (`handlers.py`), route `POST /api/daychuyen/{chay,dung,boqua}`, whitelist `lichTaiGio`, `lichTaiNhom`, `lichTaiHanChotGio`. |
| 1b-D3 | ▶ từng kênh và "Tải thêm" lúc Lịch chạy | Từ chối có báo (app + bot 409 "Lịch tải đang chạy đợt X"). "Tải thêm" đếm là lượt quét khác nên Lịch chờ nó xong. Lượt Lịch chờ khoá kênh tới 60 phút; quá hạn thì báo bot `bo_sot`, không lặng lẽ bỏ sót. |
| 1b-D4 | Bộ lịch không bao giờ kẹt | Lượt chạy trong task riêng có guard `GiuQuet`: rơi (kể cả panic) là nhả `dang_quet`. Panic → báo bot loại `ket`, hoãn 10 phút. `dot_id[..8]` đổi sang `get(..8)`. Lượt chạy quá 6 giờ, hoặc vòng lịch im quá 5 phút, đều báo "bộ lịch kẹt". |
| 1b-D5 | Kiểm ổ trước MỖI nhóm | Kiểm ổ của nhóm đó + ổ C trước mỗi nhóm. Ổ đầy giữa chừng thì dừng các nhóm sau và báo bot `o_day_giua`. |
| 1b-D6 | Ngủ đông qua ngày hôm sau | Trước mỗi nhóm so ngày bắt đầu lượt: đã sang ngày khác thì dừng lượt cũ, không ghi nhóm dở vào "đã quét" của ngày mới. Lượt ngày mới chạy 1 lần từ đầu, không chồng. Đồng hồ chỉnh lùi: giữ tiến độ, không coi là ngày mới. |
| 1b-D7 | Thoát êm ghi nốt; job 2 giây cuối không rơi | Job vừa xếp hàng đánh thức vòng ghi phiếu NGAY (`HopJob.bao`). Thoát êm rút nốt hộp job, ghi phiếu + sổ TRƯỚC `queue.shutdown()`. |

**Báo bot bqhungdown.** Tối đa 1 tin/giờ mỗi loại. Hết lỗi thì báo "đã ổn lại". Chỉ đánh dấu đã báo khi gửi thành công.

| Loại | Khi nào | Kiểu |
| --- | --- | --- |
| `khong_chay` | Lịch bật mà không chạy được: danh sách nhóm trống, ổ dưới ngưỡng trước nhóm đầu | lỗi đang có |
| `nhom_mat` | Nhóm trong Lịch không còn | lỗi đang có |
| `ghi_loi` | Ghi phiếu / `lich_tai.json` lỗi liên tục ≥ 10 phút | lỗi đang có |
| `ket` | Lượt chạy > 6 giờ, vòng lịch im > 5 phút (bot tự đo), hoặc panic | lỗi đang có / sự kiện |
| `loi_tat` | Tạo lối tắt Startup lỗi | lỗi đang có |
| `o_day_giua` | Ổ đầy giữa chừng, đã dừng các nhóm sau | sự kiện (gộp 1 tin/giờ) |
| `tre` | Đợt đóng `het_han` còn video trễ (kèm số) | sự kiện |
| `bo_sot` | Kênh bị bỏ sót vì chờ khoá quá 60 phút | sự kiện |

**Test (bqhungdown):** Rust `daychuyen::tests::b1`–`b14`, `settings_store::lich_tai_gio_sai_tu_choi_va_nhom_rong_khong_cho_bat`, `startup_shortcut::loi_tao_loi_tat_giu_cho_bot_bao_va_xoa_khi_on`. Gồm bấm 2 lần, tắt giữa chừng, đổi giờ giữa chừng, ngủ qua ngày, qua nửa đêm, đồng hồ lùi, panic. Vitest `SettingsPage.daychuyen1b.test.tsx` và `lib/lich-nhom.test.ts`. Bot `tests/test_daychuyen.py` (`LichBoardTests`, `DayChuyenAlertTests`).

## Điều phối và kiểm soát

Phiên "Quy trình khởi tạo công việc tự động" làm điều phối và không tự sửa code. Code do phiên của từng dự án sửa, vì phiên đó nắm luật test và cách phát hành riêng.

| Việc | Ai làm |
| --- | --- |
| Giữ tài liệu này và giao kèo chung (3 repo) | Phiên điều phối |
| Sửa code, viết test | Phiên bqhungdown, phiên BQ Hung Video, phiên PCBot |
| Duyệt diff, chạy lại test, đối chiếu bảng lỗ hổng | Phiên điều phối |
| Quyết định khi tài liệu và code thật lệch nhau | Anh Hùng (phiên điều phối đề xuất) |
| Nghiệm thu, cho phép phát hành | Anh Hùng |

**Điều kiện để phiên điều phối kiểm soát được thật, không phải đoán:**

- Phiên điều phối được thêm quyền đọc 3 thư mục dự án.
- Mỗi dự án có một phiên đang mở để nhận yêu cầu và trả báo cáo.
- Mỗi báo cáo xong việc phải ghi tên nhánh. Phiên điều phối đọc diff trên đúng nhánh đó và chạy lại test, không tin vào lời báo cáo.

Về sau, khi cần thêm tính năng liên quan nhiều tool (ví dụ PCBot thêm lệnh, hoặc đổi giờ chạy), làm theo cùng vòng: sửa giao kèo trước, giao việc cho từng phiên, duyệt, rồi mới nghiệm thu.
