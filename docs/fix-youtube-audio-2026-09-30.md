# Sửa tải âm thanh YouTube chậm — 30/09/2026

## Hiện tượng và bằng chứng

Phần hình lớn tải nhanh, sau đó thanh tiến độ chuyển sang file m4a khoảng
30–67 MB và tốc độ chỉ còn vài chục KB/s. Hai phiên BQHungDown 0.5.2 đang
chạy đồng thời; ảnh còn báo WinError 3 trên file tạm âm thanh.

Đọc tiến trình thực tế thấy aria2c đang tải audio/mp4, itag 140, client
VISIONOS, hai kết nối mỗi tác vụ. Không có cờ giới hạn tốc độ trong lệnh.
Đo cùng hai URL âm thanh: GET liền đạt 29.6–31.8 KiB/s; Range giới hạn 2 MiB
ở giữa file đạt 13,527–13,923 KiB/s; Range 8 MiB đạt 21,319–27,583 KiB/s.
Đây là bằng chứng theo thời điểm đo, không phải cam kết tốc độ mọi video.

## Thay đổi

- Nhúng postprocessor yt-dlp BqdAudio; bật bằng `--plugin-dirs` và
  `--use-postprocessor BqdAudio:when=before_dl` cho download YouTube.
- Chỉ đổi downloader của luồng HTTP/HTTPS audio-only trên googlevideo.com
  sang native HTTP, mỗi yêu cầu tối đa 2 MiB. Giữ cookies, proxy, kiểm tra
  Content-Range, retry, progress, resume và merge của yt-dlp.
- Phục hồi tham số downloader ngay khi lượt audio kết thúc hoặc lỗi.
  Video, HLS/DASH fragments, live, subtitle và nền tảng khác giữ hành vi cũ.
- File âm thanh `.part` của aria2 có `.part.aria2` phải tải lại riêng phần
  âm thanh: nó có thể là sparse/preallocated, không thể dùng độ dài file
  làm số byte liên tục đã tải. Không xóa phần hình đã tải xong.
- Giữ khóa Windows trên `com.prodown.app/instance.lock` suốt vòng đời app,
  trước mọi khôi phục hàng đợi/dọn rác. Phiên thứ hai báo mở phiên hiện có.
- Giải thích dưới tiến độ YouTube rằng hình và âm thanh có thể tải riêng.

## Kiểm chứng

- Dùng yt-dlp đang cài `2026.09.27.232945` với plugin mới, tải riêng bản thử
  một audio 43,351,862 byte: hoàn tất 2.05 s, 20.13 MiB/s. ffprobe nhận AAC,
  thời lượng 2678.65 s. Không thay đổi/hủy hàng đợi đang chạy.
- `python scripts/test-audio-download.py`: kiểm tra phạm vi áp dụng, phục hồi
  tham số khi lỗi, native HTTP thật với server localhost, hash toàn bộ file,
  resume phần tải liên tục và chuyển đổi từ aria2 sparse file.
- Đặt `BQD_TEST_YTDLP` tới exe cần kiểm tra để chạy thêm đúng executable đó.
  Test CLI xác nhận plugin được nạp và audio không rơi lại vào aria2c.
  Test ghép tạo video/audio bằng ffmpeg, tải qua localhost, ghép bằng yt-dlp
  và xác nhận MP4 cuối chứa đủ hai stream qua ffprobe.
- Bản yt-dlp đang cài: 5 test Python đạt, gồm tải và ghép thật với localhost.
- Rust: 227 test cũ đạt, 1 ignored (`--no-default-features`);
  test mới khóa phiên đạt với default features.
- Frontend: 124/124 test đạt.

## Dùng thử và giới hạn

Thoát hẳn CẢ HAI phiên bản cũ từ khay hệ thống trước khi mở bản sửa.
Khóa mới chỉ ngăn các phiên có bản sửa; exe cũ chưa biết khóa này.
Bản chạy thử dùng cùng cấu hình và dữ liệu với app đã cài, không phải profile
cách ly. Không mở hai bản cạnh nhau. Chưa push/tag/release.

Lỗi WinError 3 chưa được bắt tại thời điểm xảy ra; hai phiên dọn rác độc lập
là nguy cơ có căn cứ trong code, chưa chứng minh là nguyên nhân duy nhất.
Thay đổi này không thể phục hồi các file tạm đã bị xóa trước đó.

Plugin dựa vào `YoutubeDL.dl` và native HttpFD của yt-dlp; khi nâng cấp trình
tải cần chạy lại các test CLI/merge. Không thêm `--http-chunk-size` toàn cục
vì sẽ đổi cả luồng video lớn, trái với phạm vi đo và sửa lần này.
