# Douyin: lượt xem — KHÔNG LẤY ĐƯỢC (tra ngày 17/08/2026)

**Kết luận: Douyin KHÔNG công khai lượt xem cho người ngoài. Không có đường nào
lấy được bằng cách tử tế. Đừng tra lại.**

Tài liệu này ghi lại đã thử những gì, để 3 tháng nữa không ai mất công đo lại.

## 1. Gốc rễ: chính Douyin không cho xem

Trên trang cá nhân Douyin, video hiện **lượt xem cho CHÍNH CHỦ**, nhưng hiện
**lượt tim cho người khác**. Đây là thiết kế của Douyin, không phải giới hạn kỹ
thuật. Người ngoài không có cách nào thấy con số đó trên giao diện.

→ Giao diện đã không hiện thì **không API nào lấy được**.

(Các bài "cách xem lượt xem của người khác" trên mạng Trung Quốc đều dẫn tới
công cụ TRẢ TIỀN hoặc 巨量星图 — sàn của chính ByteDance, và phải được chủ kênh
**tự cấp quyền** thì mới thấy số.)

## 2. Đã thử những đường nào

| Đường | Kết quả |
|---|---|
| `aweme/v1/web/aweme/post/` (app đang dùng, có cookie đăng nhập) | `play_count` = 0 ở **21/21** bài (đo 16/08) |
| `aweme/v1/web/aweme/detail/` (từng video) | Thư viện f2 **cố ý tắt** trường này, ghi rõ `不从该接口获取` = "không lấy từ cửa này" |
| Trang chia sẻ `douyin.com/video/<id>` | Đo 17/08: HTTP 200, 72.914 byte, **không có bất kỳ trường thống kê nào** — chỉ là vỏ JS rỗng |
| `aweme/v2/web/aweme/stats/` | Đây là cửa **GHI**, không phải đọc: nó gửi `play_delta=1` để **tăng** lượt xem. Không trả về số nào |

## 3. Thư viện mã nguồn mở — đọc mã thật, không tin lời giới thiệu

| Thư viện | Lấy được lượt xem Douyin? |
|---|---|
| `Evil0ctal/Douyin_TikTok_Download_API` | KHÔNG — cả kho **0 lần** xuất hiện `play_count`; 49 cửa trong `endpoints.py` không cửa nào có `statistics` |
| `Johnserf-Seed/f2` | KHÔNG — `f2/apps/douyin/filter.py:1358` trường `play_count` bị **comment lại** kèm ghi chú `不从该接口获取` |
| `Johnserf-Seed/TikTokDownload` | KHÔNG — nay chỉ là vỏ mỏng gọi lại `f2` |
| `jiji262/douyin-downloader` | KHÔNG — cả kho **0 lần** xuất hiện `play_count` |
| yt-dlp (`DouyinIE`) | Có ánh xạ `view_count` ← `statistics.play_count`, nhưng nguồn vẫn là `aweme/detail` → vẫn nhận số 0 |

Chỗ **duy nhất** trong f2 có `play_count` chạy thật là `FriendFeedFilter`
(luồng gợi ý "bạn bè" khi đã đăng nhập). Không tra được theo ID video, nên
**vô dụng** với việc quét kênh.

## 4. Chỉ có bên BÁN API lấy được — và giá phải trả

TikHub bán cửa `fetch_multi_video_statistics` (50 video/lượt, **0,025 USD/lượt**),
tự nó ghi: *"Phần lớn các cửa Douyin không còn trả lượt xem nữa, chỉ lấy được
qua cửa này."* Cửa này chạy trên **API bản điện thoại**, cần chữ ký cấp app
(X-Gorgon / X-Argus).

**prodown KHÔNG ký được kiểu đó** — `douyin_sign.rs` chỉ có chữ ký bản web
(`a_bogus`). Không thư viện mã nguồn mở nào làm được chữ ký cấp app.

Nếu vẫn muốn mua: 250 bài = 5 lượt = **0,125 USD/kênh**. 300 kênh = **37,5 USD
mỗi lần quét toàn bộ**.

## 5. Thay vào đó nên lấy gì (MIỄN PHÍ, 0 lượt gọi thêm)

Gói JSON mà app **đang tải sẵn** có đủ các khoá này (đo 16/08):

```
admire_count · collect_count · comment_count · digg_count
play_count(=0) · recommend_count · share_count
```

App hiện **chỉ bóc `digg_count`** (`TikwmStats` trong `douyin_scraper.rs` chỉ có
đúng 1 trường). Số thật đo được từ dữ liệu kênh anh Hùng:

```
digg_count 35.316 | comment_count   908 | share_count 12.272
digg_count 12.733 | comment_count   325 | share_count  1.593
```

→ **`comment_count` và `share_count` là số THẬT, khác 0, và không tốn thêm
một lượt gọi nào.** Theo tài liệu vận hành Douyin, video nổi thường có lượt
chia sẻ **cao hơn** lượt bình luận — nên `share_count` là chỉ báo "hot" tốt
nhất trong nhóm lấy được miễn phí.

## 6. TikTok quốc tế thì NGƯỢC LẠI — có lượt xem thật

Đo thật 17/08 bằng chính yt-dlp của app, đường `--flat-playlist` (đường nhanh,
không tốn lượt gọi thêm):

```
41.100.000 · 7.900.000 · 9.100.000 · 5.800.000 · 31.600.000
```

Bộ bóc của app (`parse_flat_entry`) **đã đọc sẵn** `view_count`, nên kênh TikTok
đáng lẽ đã có lượt xem. Douyin và TikTok là **hai kho tách biệt** — đừng suy từ
cái này ra cái kia.

## 7. Nguyên tắc phải giữ

**Không được nhét `play_count = 0` vào như thể là số thật.** Anh Hùng sẽ lọc/sắp
xếp trên số bịa mà không biết. Không có thì để trống và ghi rõ "không có".
Chỗ này `douyin_scraper.rs` đang làm ĐÚNG (`view_count: None`) — giữ nguyên.
