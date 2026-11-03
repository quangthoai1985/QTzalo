# Kiến trúc hệ thống QTzalo

Tài liệu mô tả cách phân tách luồng xử lý, tổ chức module giao diện và cơ chế lưu trữ cục bộ của QTzalo (kế thừa từ `ctdozlite`).

---

## 1. Sơ đồ tổng thể

```text
┌────────────────────────────────────────────────────────────────────────┐
│                           TẦNG GIAO DIỆN (UI)                          │
│                                                                        │
│   ┌───────────────────────────────┐  ┌─────────────────────────────┐   │
│   │  GUI (src/gui.rs + src/gui/*) │  │     TUI (src/tui.rs)        │   │
│   │  • eframe / egui              │  │  • ratatui / crossterm      │   │
│   │  • Event-driven repaint       │  │  • 30ms terminal poll loop  │   │
│   └───────────────┬───────────────┘  └──────────────┬──────────────┘   │
└───────────────────┼─────────────────────────────────┼──────────────────┘
                    │ WorkerCmd (mpsc)                │
                    ▼                                 ▼
┌────────────────────────────────────────────────────────────────────────┐
│                     LUỒNG NỀN (TOKIO ASYNC WORKER)                     │
│                                                                        │
│   • WebSocket loop (`zpw_ws`): nhận tin nhắn & sự kiện VoIP tức thì    │
│   • HTTP Poller (`get_last_msgs` / `lp_control`): đồng bộ dự phòng     │
│   • Sticker Engine: gọi song song suggest + search & prefetch ảnh      │
│   • CallBridge (`src/call.rs`): quản lý báo hiệu & luồng RTP/RTCP      │
└───────────────────┬─────────────────────────────────┬──────────────────┘
                    │                                 │
                    ▼                                 ▼
┌───────────────────────────────────┐  ┌─────────────────────────────────┐
│        LƯU TRỮ CỤC BỘ             │  │      MẬT MÃ & PHIÊN MẠNG        │
│                                   │  │                                 │
│ • threads_cache.json (snapshot)   │  │ • ParamCipher v2 & SignKey MD5  │
│ • zalo_history.db (SQLite)        │  │ • AES-128-CBC / AES-256-CBC     │
│ • stickers_cache/ (WebP/PNG)      │  │ • Cookie jar đa domain zpw_sek  │
└───────────────────────────────────┘  └─────────────────────────────────┘
```

---

## 2. Tổ chức module GUI (`src/gui/`)

Để dễ bảo trì, phần giao diện đồ họa được chia thành các module con theo trách nhiệm:

- `src/gui.rs`: Khởi tạo `ZaloGuiApp`, quản lý trạng thái ứng dụng, xử lý hàng đợi `GuiEvent`, thanh tiêu đề cửa sổ và màn hình đăng nhập.
- `src/gui/theme.rs`: Bảng màu `ThemePalette` (Dark/Light), bộ icon vector `lucide`, vẽ avatar và nạp font tiếng Việt của hệ điều hành.
- `src/gui/stickers.rs`: Bảng chọn nhãn dán (`StickerPicker`), thuật toán gợi ý nhãn dán theo thời gian thực khi gõ (`resolve_contextual_sticker_suggestions`) và thanh popover gợi ý nổi trên ô nhập.
- `src/gui/worker.rs`: Định nghĩa `WorkerCmd`, luồng nền Tokio (`spawn_session_worker`) và bộ tải trước ảnh thumbnail nhãn dán (`preload_disk_sticker_thumbnails`, `prefetch_missing_sticker_thumbnails`).
- `src/gui/workspace.rs`: Bố cục 3 cột gồm thanh điều hướng, danh sách hội thoại, vùng khung chat chính và ngăn thông tin chi tiết bên phải.
- `src/gui/messages.rs`: Hiển thị bong bóng tin nhắn, nhãn dán, thẻ cuộc gọi, trích dẫn, cảm xúc (reaction), menu chuột phải và xử lý dán ảnh từ clipboard.
- `src/gui/modals.rs`: Các cửa sổ phụ gồm cài đặt, cuộc gọi thoại/video, xem ảnh phóng to, chuyển tiếp tin nhắn, tìm kiếm theo số điện thoại và Dev Inspector.
- `src/gui/media_view.rs`: Cửa sổ media riêng của app (từ 0.1.2) — phát video trong cửa sổ "Video" bằng `ffmpeg` (tải vào `stickers_cache/videos/`, giải mã ra khung PPM + PCM, quản lý và dừng tiến trình con), xem ảnh phóng to. Không mở trình duyệt, không mở trình phát ngoài trừ khi thiếu `ffmpeg`.
- `src/gui/demo.rs`: Dữ liệu mẫu offline cho chế độ `--demo` (dùng để chụp ảnh và kiểm thử giao diện). Không truy cập mạng, không đọc/ghi dữ liệu thật; constructor GUI bỏ qua session worker nên mọi lệnh gửi worker thành no-op.

---

## 3. Cơ chế đồng bộ bất đồng bộ

### 3.1. Tách biệt luồng vẽ giao diện và luồng mạng
- Luồng giao diện (UI thread) không bao giờ gọi trực tiếp các hàm mạng chặn (blocking). Mọi thao tác mạng được gửi qua kênh `tokio::sync::mpsc::unbounded_channel` dưới dạng `WorkerCmd`.
- Luồng worker chạy trên một Tokio runtime 2 worker threads, xử lý kết nối WebSocket `zpw_ws`, các truy vấn REST API và giải mã ảnh trên nền. Khi có kết quả, worker gửi `GuiEvent` về UI kèm lệnh `egui_ctx.request_repaint()`.

### 3.2. Gợi ý nhãn dán & Prefetch ảnh song song
- Khi người dùng gõ từ khóa trong ô soạn thảo (sau khoảng trễ debounce 145ms cho chữ cái hoặc 0ms cho biểu tượng cảm xúc/từ khóa đã cache), UI gửi lệnh `WorkerCmd::SuggestComposerStickers`.
- Worker chuẩn hóa chuỗi truy vấn tiếng Việt (sinh cả dạng có dấu, không dấu và từ đồng nghĩa/viết tắt), sau đó gọi đồng thời hai API `/api/message/sticker/suggest/stickers` (`cmd 11741`) và `/api/message/sticker/search` (`cmd 11747`).
- Ngay khi nhận danh sách ID nhãn dán, worker nạp trước các file ảnh đã có trong `stickers_cache/` lên bộ nhớ texture trước khi gửi `GuiEvent::ComposerStickerSuggestions`, đồng thời tải song song các ảnh còn thiếu qua HTTP để tránh hiện tượng nhấp nháy khung chờ.

---

## 4. Lưu trữ dữ liệu 2 tầng

1. **Snapshot JSON (`threads_cache.json`)**: Lưu danh sách hội thoại và các tin nhắn gần nhất để hiển thị ngay khi khởi động ứng dụng.
2. **SQLite (`zalo_history.db`)**: Lưu trữ lâu dài toàn bộ tin nhắn theo bảng `messages(msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json)` có đánh chỉ mục trên `(thread_id, ts)`. Khi đồng bộ tin nhắn mới, hệ thống khử trùng theo `msg_id` và theo cặp `(text, is_outgoing, ts)` trong biên độ 5 giây.

Ngoài `messages`, cơ sở dữ liệu còn có 3 bảng (2 bảng sau thêm ở bản 0.1.2):

3. **`threads(thread_id, name, is_group, last_ts)`**: bản ghi rút gọn của danh sách hội thoại, dùng để dựng lại danh sách và hoạt động gần nhất khi mở app.
4. **`missing_ranges(id, conv_id, from_ts, to_ts, reason, created_at, dismissed)`**: các khoảng thời gian mà app biết là có thể thiếu tin (máy chủ bỏ bớt hàng đợi, con trỏ bị reset). Đây là nguồn của banner "Tin nhắn chưa được đồng bộ đủ"; `conv_id = "*"` nghĩa là áp cho mọi hội thoại.
5. **`thread_activity(thread_id, is_group, last_msg_id, updated_at)`**: hoạt động cuối của từng hội thoại theo `msg_id`, phục vụ việc xếp danh sách hội thoại giống điện thoại.

Ba bảng `threads`, `missing_ranges`, `thread_activity` đều dùng câu lệnh tham số hoá (`params!`) khi ghi/đọc, không nối chuỗi SQL.
