# ProDowwn Web — Hướng dẫn Deploy & Sử dụng

## Tổng quan

ProDowwn Web là phiên bản chạy trên trình duyệt, dành cho **iPhone/iPad hoặc bất kỳ thiết bị nào**.

- **Frontend**: React app (đã build sẵn trong `dist-web/`)
- **Backend**: Node.js server chạy yt-dlp để tải video
- **Cách hoạt động**: Paste link video → server tải → gửi về browser → iPhone lưu

---

## Cách 1: Deploy lên Railway (Miễn phí, khuyến nghị)

### Bước 1: Tạo tài khoản Railway
1. Vào https://railway.app
2. Đăng nhập bằng GitHub
3. Free tier: **500 giờ/tháng** (đủ dùng nhiều tháng)

### Bước 2: Deploy
1. Trên Railway, click **New Project** → **Deploy from GitHub repo**
2. Chọn repo `prodowwn`
3. Railway sẽ tự động dùng `Dockerfile` để build
4. Đợi deploy xong (~3-5 phút lần đầu)
5. Railway sẽ cho URL, ví dụ: `https://prodowwn.up.railway.app`

### Bước 3: Cập nhật API URL
Sau khi có URL Railway, chỉnh sửa `vite.web.config.ts`:

```ts
// Đổi dòng này:
proxy: { "/api": { target: "https://YOUR-URL.up.railway.app", ... } }
//                    ^^^^^^^^^^^^^^^ Thay bằng URL Railway của bạn
```

Sau đó rebuild: `npm run web:build` rồi push lên GitHub. Railway sẽ deploy lại.

### Bước 4: Mở trên iPhone
1. Vào URL Railway (ví dụ: `https://prodowwn.up.railway.app`)
2. Thêm vào màn hình chính: nhấn **Share** → **Add to Home Screen**
3. Dùng như app thông thường

---

## Cách 2: Tự host trên VPS/Linux (không giới hạn)

```bash
# Clone repo
git clone https://github.com/YOUR_USERNAME/prodowwn.git
cd prodowwn

# Build web frontend
npm install
npm run web:build

# Build và chạy Docker
docker build -t prodowwn .
docker run -d -p 3001:3001 --restart unless-stopped prodowwn

# Mở http://YOUR_SERVER_IP:3001
```

---

## Cách 3: Chạy local trên máy tính (để test)

```bash
# Terminal 1: Chạy backend
cd prodowwn
npm run server:start

# Terminal 2: Chạy frontend web
npm run web:dev

# Mở http://localhost:5173
```

---

## Cách dùng trên iPhone

1. **Mở web**: Vào URL đã deploy
2. **Paste link**: Dán link video (YouTube, TikTok, Douyin, Instagram...)
3. **Chọn chất lượng**: Video hoặc Audio
4. **Nhấn Tải**: Video sẽ được tải về iPhone qua trình duyệt

### Lưu video trên iPhone:
- iOS sẽ hỏi mở bằng app nào → chọn **Files** để lưu vào local
- Hoặc dùng app **Documents** (miễn phí trên App Store) để quản lý file đã tải

### Lưu ý quan trọng:
- **Dung lượng video**: Video lớn có thể cần vài phút để tải. Giữ tab trình duyệt mở.
- **Nếu tải bị lỗi**: Thử dùng VPN vì một số nền tảng chặn IP Việt Nam
- **iOS giới hạn**: Safari trên iOS không cho phép tải file > 4GB. Video rất dài nên tải trên máy tính.

---

## Nền tảng hỗ trợ (Web)

| Nền tảng | Hỗ trợ | Ghi chú |
|-----------|---------|---------|
| YouTube | ✅ | Video + Audio |
| TikTok | ✅ | Video + Audio |
| Douyin | ✅ | Video |
| Instagram | ✅ | Video/Reels |
| Facebook | ✅ | Video |
| Twitter/X | ✅ | Video |
| Reddit | ✅ | Video |
| Pinterest | ✅ | Video/Ảnh |

---

## Khắc phục lỗi thường gặp

### Lỗi "Connection refused"
- Server chưa start → kiểm tra Railway dashboard

### Lỗi "URL không hỗ trợ"
- Link sai format → dùng link đầy đủ (không rút gọn)

### Video tải rất chậm
- Server yếu → nâng cấp Railway plan
- Dùng VPN để tránh bị chặn theo khu vực

### Không lưu được video
- iOS yêu cầu dùng app Files: nhấn giữ video → Share → Save to Files

---

## So sánh Desktop vs Web

| Tính năng | Desktop (Tauri) | Web |
|-----------|----------------|-----|
| Tải kênh YouTube | ✅ | ❌ |
| Tải kênh Douyin | ⚠️ (khó) | ❌ |
| Tải video đơn lẻ | ✅ | ✅ |
| Chạy trên iPhone | ❌ | ✅ |
| Không cần cài đặt | ❌ | ✅ |
| Tốc độ tải | Nhanh (local) | Phụ thuộc server |
| Playlist | ✅ | ❌ |
