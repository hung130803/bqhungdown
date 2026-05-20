# Requirements Document

## Introduction

Tài liệu này mô tả yêu cầu cho ứng dụng desktop tải video đa nền tảng, được xây dựng trên Tauri 2 + React 18 + TypeScript + Vite. Ứng dụng sử dụng yt-dlp làm engine tải, ffmpeg để xử lý media, và aria2c như external downloader tùy chọn để tăng tốc. Mục tiêu là cung cấp trải nghiệm tải video nhanh hơn IDM thông qua multi-connection và parallel queue, đồng thời hỗ trợ tất cả các site mà yt-dlp hỗ trợ (1800+), với 6 nền tảng được làm nổi bật: YouTube, TikTok, Facebook, Instagram, Twitter/X, Twitch.

Workflow chuẩn của người dùng: Paste link → chọn chất lượng → chọn folder → tải. Tên file giữ nguyên theo metadata gốc của video, không thêm prefix/suffix/timestamp. Khi trùng tên, hệ thống hỏi người dùng để overwrite, skip, hoặc auto-rename. UI hiển thị bằng tiếng Việt, hỗ trợ dark mode, và phát thông báo hệ thống khi tải xong.

## Glossary

- **App**: Ứng dụng desktop multi-platform-video-downloader (Tauri shell + React frontend + Rust backend).
- **Frontend**: Phần React 18 + TypeScript chạy trong webview của Tauri.
- **Backend**: Phần Rust của Tauri xử lý IPC, sidecar, và filesystem.
- **YtDlp_Sidecar**: Tiến trình yt-dlp được Tauri spawn dưới dạng sidecar binary.
- **Ffmpeg_Sidecar**: Tiến trình ffmpeg được Tauri spawn dưới dạng sidecar binary, dùng để mux/convert media.
- **Aria2c**: External downloader tùy chọn được yt-dlp gọi qua tham số `--downloader aria2c` để tăng tốc tải.
- **Download_Item**: Một mục tải xuống trong queue, đại diện cho một video hoặc một entry trong playlist.
- **Short_ID**: Mã định danh ngắn 6-8 ký tự (hash) gán cho mỗi Download_Item, hiển thị trên UI.
- **Queue_Manager**: Thành phần backend quản lý hàng đợi tải, lập lịch concurrency, pause/resume/cancel/retry.
- **Quality_Format**: Một format trong danh sách `formats` do yt-dlp trả về (gồm format_id, resolution, fps, codec, bitrate, filesize).
- **Save_Folder**: Thư mục đích trên ổ đĩa nơi file được ghi.
- **Default_Folder**: Save_Folder mặc định được lưu trong Settings; ban đầu là thư mục Downloads của hệ điều hành.
- **History_Store**: Kho lưu lịch sử các Download_Item đã hoàn tất hoặc bị huỷ.
- **Settings_Store**: Kho lưu cấu hình người dùng (concurrency, Default_Folder, theme, language).
- **Filename_Template**: Template tên file truyền cho yt-dlp, mặc định `%(title)s.%(ext)s`.
- **Clipboard_Watcher**: Thành phần frontend lắng nghe nội dung clipboard để phát hiện URL hợp lệ.
- **Supported_Site**: Một site nằm trong danh sách `yt-dlp --list-extractors`.
- **Featured_Platform**: Một trong 6 nền tảng được làm nổi bật trên UI: YouTube, TikTok, Facebook, Instagram, Twitter/X, Twitch.

## Requirements

### Requirement 1 — Paste và validate URL đa nền tảng

**User Story:** Là người dùng, tôi muốn paste link video từ bất kỳ nền tảng nào yt-dlp hỗ trợ và nhận phản hồi xác thực ngay lập tức, để biết URL có thể tải được hay không trước khi tiếp tục.

#### Acceptance Criteria

1. THE Frontend SHALL cung cấp một ô input cho phép người dùng dán URL từ clipboard.
2. WHEN người dùng dán hoặc nhập một URL vào ô input, THE Frontend SHALL kiểm tra cú pháp URL (scheme http hoặc https, host hợp lệ) trong vòng 100ms.
3. WHEN một URL đã được xác thực cú pháp, THE Backend SHALL xác định Supported_Site tương ứng bằng cách so khớp host với danh sách extractor của YtDlp_Sidecar.
4. IF URL không khớp với bất kỳ Supported_Site nào, THEN THE Frontend SHALL hiển thị thông báo lỗi tiếng Việt nêu rõ URL không được hỗ trợ.
5. WHERE URL thuộc một Featured_Platform, THE Frontend SHALL hiển thị icon và tên nền tảng tương ứng cạnh ô input.

### Requirement 2 — Fetch video metadata và danh sách chất lượng

**User Story:** Là người dùng, tôi muốn xem thông tin video (tiêu đề, thumbnail, thời lượng) và danh sách chất lượng có sẵn trước khi tải, để chọn đúng định dạng mong muốn.

#### Acceptance Criteria

1. WHEN một URL hợp lệ đã được xác thực, THE Backend SHALL gọi YtDlp_Sidecar với tham số `--dump-single-json` để lấy metadata.
2. WHEN metadata được trả về thành công, THE Frontend SHALL hiển thị tiêu đề, thumbnail, thời lượng, tên kênh và danh sách Quality_Format.
3. THE Frontend SHALL trình bày danh sách Quality_Format kèm độ phân giải, fps, codec, kích thước file ước tính cho từng mục.
4. IF YtDlp_Sidecar trả về lỗi hoặc timeout sau 30 giây, THEN THE Frontend SHALL hiển thị thông báo lỗi tiếng Việt và cho phép người dùng thử lại.
5. WHERE URL trỏ đến một playlist, THE Backend SHALL trả về metadata cho từng entry kèm tổng số entry.

### Requirement 3 — Chọn chất lượng video hoặc audio-only

**User Story:** Là người dùng, tôi muốn chọn chất lượng video hoặc chế độ audio-only, để tải đúng định dạng phục vụ nhu cầu của tôi.

#### Acceptance Criteria

1. THE Frontend SHALL cung cấp tuỳ chọn chế độ tải gồm "Video" và "Audio only (MP3)".
2. WHILE chế độ "Video" được chọn, THE Frontend SHALL hiển thị danh sách Quality_Format video kèm tuỳ chọn "Best" (chọn format chất lượng cao nhất theo `bv*+ba/b`).
3. WHILE chế độ "Audio only (MP3)" được chọn, THE Frontend SHALL ẩn danh sách video format và mặc định chọn audio bitrate cao nhất.
4. WHEN người dùng chọn một Quality_Format, THE Frontend SHALL lưu format_id đã chọn để truyền cho YtDlp_Sidecar khi bắt đầu tải.
5. IF không có Quality_Format video nào khả dụng, THEN THE Frontend SHALL vô hiệu hoá chế độ "Video" và hiển thị thông báo tiếng Việt giải thích lý do.

### Requirement 4 — Chọn folder lưu với ghi nhớ folder gần nhất

**User Story:** Là người dùng, tôi muốn chọn folder lưu video và App ghi nhớ folder lần trước, để tôi không phải chọn lại mỗi lần tải.

#### Acceptance Criteria

1. THE Frontend SHALL hiển thị Default_Folder làm Save_Folder gợi ý cho mỗi lần tải mới.
2. WHEN người dùng nhấn nút "Chọn folder", THE Frontend SHALL mở hộp thoại chọn thư mục của hệ điều hành thông qua Tauri dialog API.
3. WHEN người dùng xác nhận một Save_Folder mới, THE Settings_Store SHALL cập nhật Default_Folder thành Save_Folder vừa chọn.
4. WHEN App khởi động lần đầu tiên, THE Settings_Store SHALL khởi tạo Default_Folder bằng thư mục Downloads của hệ điều hành.
5. IF Save_Folder đã chọn không tồn tại hoặc không có quyền ghi tại thời điểm bắt đầu tải, THEN THE Backend SHALL trả về lỗi và THE Frontend SHALL nhắc người dùng chọn folder khác.

### Requirement 5 — Download với multi-connection để vượt tốc độ IDM

**User Story:** Là người dùng, tôi muốn tải video với tốc độ vượt IDM thông qua multi-connection, để tận dụng băng thông tối đa.

#### Acceptance Criteria

1. WHEN một Download_Item bắt đầu tải, THE Backend SHALL gọi YtDlp_Sidecar với tham số `-N 16` để bật 16 kết nối song song mặc định.
2. WHERE Aria2c có sẵn trong Settings_Store, THE Backend SHALL truyền `--downloader aria2c` và `--downloader-args "aria2c:-x 16 -s 16 -k 1M"` cho YtDlp_Sidecar.
3. WHILE một Download_Item đang tải, THE Backend SHALL phát sự kiện tiến trình ít nhất mỗi 500ms gồm bytes đã tải, tổng bytes, tốc độ hiện tại và ETA.
4. THE Queue_Manager SHALL cho phép tối đa N Download_Item tải song song, trong đó N là giá trị max concurrency từ Settings_Store.
5. IF kết nối mạng bị gián đoạn trong khi tải, THEN THE Backend SHALL tự động retry tối đa 3 lần với khoảng cách tăng dần 2s, 5s, 10s trước khi đánh dấu Download_Item là failed.

### Requirement 6 — Hiển thị Download_Item với Short_ID

**User Story:** Là người dùng, tôi muốn mỗi mục tải có một mã định danh ngắn dễ đọc, để tham chiếu nhanh khi nói chuyện hoặc kiểm tra log.

#### Acceptance Criteria

1. WHEN một Download_Item được tạo, THE Backend SHALL sinh một Short_ID dài 6 đến 8 ký tự bằng cách hash URL kết hợp với timestamp.
2. THE Frontend SHALL hiển thị Short_ID của mỗi Download_Item bên cạnh tiêu đề trong queue và history.
3. THE Backend SHALL đảm bảo Short_ID là duy nhất trong phạm vi queue và History_Store hiện hành.
4. IF phát sinh xung đột Short_ID, THEN THE Backend SHALL sinh lại Short_ID mới cho đến khi đảm bảo tính duy nhất.

### Requirement 7 — Giữ nguyên tên file gốc và xử lý trùng tên

**User Story:** Là người dùng, tôi muốn tên file tải xuống giữ đúng tên gốc từ metadata video, và khi trùng tên thì App hỏi tôi cách xử lý, để tôi kiểm soát hoàn toàn cách lưu file.

#### Acceptance Criteria

1. THE Backend SHALL truyền Filename_Template `%(title)s.%(ext)s` cho YtDlp_Sidecar khi bắt đầu tải.
2. THE Backend SHALL KHÔNG thêm prefix, suffix hoặc timestamp vào tên file đầu ra.
3. WHEN một file đích đã tồn tại trong Save_Folder ngay trước khi ghi, THE Frontend SHALL hiển thị hộp thoại với ba lựa chọn: "Ghi đè", "Bỏ qua", "Tự động đổi tên".
4. WHEN người dùng chọn "Tự động đổi tên", THE Backend SHALL thêm hậu tố ` (n)` vào tên file với n là số nguyên nhỏ nhất từ 1 trở lên sao cho tên mới không trùng.
5. WHEN người dùng chọn "Bỏ qua", THE Queue_Manager SHALL đánh dấu Download_Item là skipped và chuyển sang mục tiếp theo.
6. WHEN người dùng chọn "Ghi đè", THE Backend SHALL ghi đè file đích bằng nội dung mới.

### Requirement 8 — Queue Manager với pause, resume, cancel, retry và concurrency

**User Story:** Là người dùng, tôi muốn quản lý hàng đợi tải với các thao tác pause, resume, cancel, retry và đặt số lượng tải đồng thời, để kiểm soát băng thông và thứ tự ưu tiên.

#### Acceptance Criteria

1. THE Queue_Manager SHALL hỗ trợ các trạng thái cho mỗi Download_Item: queued, downloading, paused, completed, failed, cancelled, skipped.
2. WHEN người dùng nhấn pause trên một Download_Item đang downloading, THE Backend SHALL gửi tín hiệu dừng cho YtDlp_Sidecar và chuyển trạng thái sang paused.
3. WHEN người dùng nhấn resume trên một Download_Item paused, THE Backend SHALL khởi động lại YtDlp_Sidecar với cờ `--continue` để tiếp tục từ byte cuối cùng.
4. WHEN người dùng nhấn cancel trên một Download_Item, THE Backend SHALL kết thúc tiến trình YtDlp_Sidecar liên quan và chuyển trạng thái sang cancelled.
5. WHEN người dùng nhấn retry trên một Download_Item failed hoặc cancelled, THE Queue_Manager SHALL đưa Download_Item về trạng thái queued với cùng URL và Quality_Format đã chọn.
6. WHEN giá trị max concurrency trong Settings_Store thay đổi, THE Queue_Manager SHALL điều chỉnh số Download_Item đang downloading để không vượt giá trị mới trong vòng 2 giây.

### Requirement 9 — Batch và playlist download

**User Story:** Là người dùng, tôi muốn tải nhiều URL cùng lúc hoặc cả playlist trong một thao tác, để tiết kiệm thời gian khi cần tải số lượng lớn.

#### Acceptance Criteria

1. THE Frontend SHALL cho phép người dùng dán nhiều URL phân cách bởi dòng mới trong một ô batch input.
2. WHEN người dùng gửi batch input, THE Backend SHALL tạo một Download_Item riêng cho mỗi URL hợp lệ và đưa vào Queue_Manager.
3. WHEN URL được nhận diện là playlist, THE Frontend SHALL hiển thị danh sách entry kèm checkbox cho phép chọn các entry cần tải.
4. WHEN người dùng xác nhận lựa chọn entry của playlist, THE Backend SHALL tạo một Download_Item riêng cho mỗi entry được chọn.
5. WHERE URL là playlist và người dùng chọn "Tải tất cả", THE Backend SHALL truyền tham số `--yes-playlist` cho YtDlp_Sidecar và tạo Download_Item cho mọi entry.

### Requirement 10 — Audio-only extraction sang MP3

**User Story:** Là người dùng, tôi muốn chuyển video sang MP3 với chất lượng cao, để nghe nhạc khi không cần video.

#### Acceptance Criteria

1. WHEN người dùng chọn chế độ "Audio only (MP3)" và bắt đầu tải, THE Backend SHALL truyền các tham số `-x --audio-format mp3 --audio-quality 0` cho YtDlp_Sidecar.
2. THE Backend SHALL sử dụng Ffmpeg_Sidecar để chuyển audio stream sang MP3 với bitrate VBR cao nhất do yt-dlp xác định.
3. WHEN quá trình chuyển đổi MP3 hoàn tất, THE Backend SHALL ghi file `.mp3` vào Save_Folder với cùng Filename_Template như chế độ video.
4. IF Ffmpeg_Sidecar không khả dụng, THEN THE Backend SHALL trả về lỗi và THE Frontend SHALL hiển thị thông báo tiếng Việt yêu cầu cài đặt lại App.

### Requirement 11 — Subtitle download và auto-translate

**User Story:** Là người dùng, tôi muốn tải phụ đề SRT đi kèm video và tuỳ chọn dịch tự động sang ngôn ngữ tôi chọn, để xem nội dung dễ hiểu hơn.

#### Acceptance Criteria

1. THE Frontend SHALL hiển thị danh sách ngôn ngữ phụ đề có sẵn từ metadata cho mỗi Download_Item.
2. WHEN người dùng chọn một hoặc nhiều ngôn ngữ phụ đề, THE Backend SHALL truyền `--write-subs --sub-langs <codes> --convert-subs srt` cho YtDlp_Sidecar.
3. WHERE người dùng bật tuỳ chọn auto-translate sang một target language, THE Backend SHALL truyền `--write-auto-subs --sub-langs <target>` cho YtDlp_Sidecar.
4. WHEN tải xong, THE Backend SHALL ghi mỗi file phụ đề `.srt` vào cùng Save_Folder với video, dùng Filename_Template `%(title)s.%(ext)s`.
5. IF không có phụ đề nào khả dụng cho ngôn ngữ được chọn, THEN THE Frontend SHALL hiển thị thông báo tiếng Việt và tiếp tục tải video không kèm phụ đề.

### Requirement 12 — Clipboard auto-detect URL

**User Story:** Là người dùng, tôi muốn App tự động phát hiện URL khi tôi copy link vào clipboard, để có thể bắt đầu tải nhanh mà không cần dán thủ công.

#### Acceptance Criteria

1. WHILE App đang chạy ở foreground và Clipboard_Watcher được bật trong Settings_Store, THE Clipboard_Watcher SHALL kiểm tra clipboard mỗi 1000ms.
2. WHEN Clipboard_Watcher phát hiện một URL mới khớp với một Supported_Site, THE Frontend SHALL hiển thị banner gợi ý kèm URL và nút "Tải nhanh".
3. WHEN người dùng nhấn "Tải nhanh" trên banner, THE Frontend SHALL tự động điền URL vào ô input và kích hoạt fetch metadata theo Requirement 2.
4. THE Settings_Store SHALL cho phép người dùng bật hoặc tắt Clipboard_Watcher.
5. IF cùng một URL đã hiển thị banner trong 60 giây gần nhất, THEN THE Clipboard_Watcher SHALL không hiển thị banner cho URL đó lần thứ hai.

### Requirement 13 — Download history và re-download

**User Story:** Là người dùng, tôi muốn xem lịch sử các video đã tải và tải lại bất kỳ mục nào chỉ với một cú nhấp, để truy cập lại nội dung đã tải dễ dàng.

#### Acceptance Criteria

1. WHEN một Download_Item chuyển sang trạng thái completed, failed, hoặc cancelled, THE History_Store SHALL ghi nhận URL, Short_ID, tiêu đề, Quality_Format, Save_Folder, trạng thái và timestamp.
2. THE Frontend SHALL cung cấp một trang History hiển thị tất cả mục trong History_Store theo thứ tự thời gian giảm dần.
3. WHEN người dùng nhấn "Tải lại" trên một mục history, THE Backend SHALL tạo một Download_Item mới với cùng URL và Quality_Format và đưa vào Queue_Manager.
4. THE Frontend SHALL cung cấp ô tìm kiếm cho phép lọc history theo tiêu đề hoặc URL.
5. WHEN người dùng nhấn "Xoá" trên một mục history, THE History_Store SHALL xoá mục đó khỏi kho lưu.

### Requirement 14 — Dark mode và UI tiếng Việt

**User Story:** Là người dùng, tôi muốn UI hiển thị bằng tiếng Việt và hỗ trợ dark mode, để sử dụng ứng dụng thoải mái trong mọi điều kiện ánh sáng và ngôn ngữ.

#### Acceptance Criteria

1. THE Frontend SHALL hiển thị toàn bộ chuỗi giao diện bằng tiếng Việt theo mặc định.
2. THE Frontend SHALL hỗ trợ hai theme: "light" và "dark".
3. WHEN người dùng chuyển theme trong Settings, THE Frontend SHALL áp dụng theme mới trong vòng 200ms mà không cần khởi động lại App.
4. WHEN App khởi động, THE Frontend SHALL áp dụng theme đã lưu trong Settings_Store; nếu chưa có giá trị, THE Frontend SHALL theo theme hệ điều hành.
5. WHERE người dùng chọn ngôn ngữ khác trong Settings, THE Frontend SHALL chuyển toàn bộ chuỗi giao diện sang ngôn ngữ đó.

### Requirement 15 — System notification khi tải xong

**User Story:** Là người dùng, tôi muốn nhận thông báo hệ thống khi mỗi video tải xong, để biết kết quả mà không cần mở App.

#### Acceptance Criteria

1. WHEN một Download_Item chuyển sang trạng thái completed, THE Backend SHALL gửi system notification qua Tauri notification API kèm tiêu đề video và Short_ID.
2. WHEN người dùng nhấn vào notification, THE App SHALL mở cửa sổ chính và focus vào Download_Item tương ứng.
3. IF một Download_Item chuyển sang trạng thái failed, THEN THE Backend SHALL gửi system notification kèm lý do lỗi tóm tắt.
4. WHERE thông báo hệ thống bị tắt trong Settings_Store, THE Backend SHALL không gửi notification cho mọi Download_Item.

### Requirement 16 — Settings cho concurrency, default folder, theme và language

**User Story:** Là người dùng, tôi muốn cấu hình max concurrency, default folder, theme và ngôn ngữ trong Settings, để tuỳ biến App phù hợp với máy và sở thích của tôi.

#### Acceptance Criteria

1. THE Frontend SHALL cung cấp trang Settings với các trường: max concurrency, Default_Folder, theme, language, bật/tắt Clipboard_Watcher, bật/tắt notification, bật/tắt Aria2c.
2. THE Settings_Store SHALL chấp nhận giá trị max concurrency là số nguyên trong khoảng 1 đến 10 với mặc định 3.
3. WHEN người dùng thay đổi bất kỳ giá trị nào trong Settings, THE Settings_Store SHALL ghi giá trị mới xuống đĩa trong vòng 500ms.
4. WHEN App khởi động, THE Settings_Store SHALL nạp toàn bộ cấu hình từ đĩa và áp dụng trước khi Frontend render lần đầu.
5. IF file cấu hình bị hỏng hoặc thiếu, THEN THE Settings_Store SHALL khởi tạo lại với giá trị mặc định và ghi đè file cấu hình.
