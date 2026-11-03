## QTzalo v{{VERSION}}

Ứng dụng Zalo desktop native viết bằng Rust, không chính thức. Bản build từ commit `{{COMMIT}}`.

### Tải về

| Hệ điều hành | File |
|---|---|
| **Windows 10/11 x64 (bộ cài)** | `QTzalo-{{VERSION}}-setup-windows-x86_64.exe` |
| Windows 10/11 x64 (portable) | `QTzalo-{{VERSION}}-portable-windows-x86_64.exe` |
| Ubuntu 22.04+ / Debian 12+ | `qtzalo_{{VERSION}}_amd64.deb` |
| Linux khác (glibc ≥ 2.35) | `qtzalo-{{VERSION}}-linux-x86_64.tar.gz` |
| macOS Apple Silicon (thử nghiệm, nếu có) | `QTzalo-{{VERSION}}-macos-arm64.dmg` |

Kiểm tra file bằng `SHA256SUMS.txt`. Bộ cài Windows chưa ký số: nếu SmartScreen cảnh báo, bấm **More info → Run anyway** sau khi đã kiểm tra SHA256. Hướng dẫn chi tiết: [docs/CAI_DAT.md](https://github.com/quangthoai1985/QTzalo/blob/v{{VERSION}}/docs/CAI_DAT.md).

### Điểm mới

Bản này là kết quả của đợt kiểm toán toàn bộ mã nguồn ngày 03/11/2026: sửa các lỗi "âm thầm" (thành công giả, lịch sử nạp sai), làm giao diện trung thực hơn và dọn dung lượng đĩa.

- **Lịch sử và gửi tin:**
  - hội thoại dài hiển thị đúng tin **mới nhất** sau khi mở lại (trước đây nạp nhầm tin cũ nhất);
  - gửi tin thất bại **không còn báo thành công**: mọi hành động ghi (gửi, thu hồi, xoá, cảm xúc, lời mời kết bạn…)
    đều kiểm mã lỗi máy chủ và hiện lý do;
  - tải tệp lên lỗi thì không gửi tin nữa; gửi link/trả lời lỗi trả lỗi thay vì âm thầm đổi thành tin văn bản;
  - xoá/thu hồi dùng đúng `cliMsgId` của tin, không gửi id bịa.
- **Tải tệp:** mọi tệp/ảnh/video đang tải đều có nút **Huỷ**; tệp lớn tải chậm không còn bị cắt sau 35 giây.
- **Cảm xúc:** nếu gửi cảm xúc thất bại, bong bóng tự trở về trạng thái trước đó (trước đây lệch với máy chủ).
- **"Đang nhập":** QTzalo hiện chỉ báo đang nhập của người khác (bật/tắt trong Cài đặt → Tin nhắn).
- **Cài đặt trung thực:** 20 mục chưa có tác dụng nay hiện mờ kèm nhãn **(sắp có)** thay vì bật/tắt vô ích;
  các mục đang chạy thật (thư mục tải, chủ đề, thông báo, nhấp đúp để trả lời, hiện trạng thái hoạt động…)
  giữ nguyên.
- **Bỏ dữ liệu hiển thị không thật:** dấu "Đang hoạt động"/"Trực tuyến" chỉ hiện khi bạn bật;
  "Tổng số tin nhắn" đổi thành "Số tin đang hiển thị"; mục ảnh/video đã chia sẻ liệt kê cả video và mở được
  trình phát; bỏ hậu tố "· Đã nhận" khi không có dữ liệu đã đọc.
- **Dung lượng (áp dụng nghiên cứu Zalo PC):**
  - tự dọn cache ảnh/video cũ hơn **30 ngày**, giữ tối đa **64 MiB**, ngay khi khởi động;
  - tự nén cơ sở dữ liệu lịch sử khi vượt **32 MiB** (chỉ nén, **không xoá tin**);
  - thêm nút **Dọn dẹp ngay** trong Cài đặt → Dữ liệu & làm mới.
- **Tin nhắn mã hoá đầu cuối (E2EE):** nếu gặp, QTzalo hiện bong bóng giữ chỗ
  *"Tin nhắn mã hoá đầu cuối — xem trên điện thoại"* thay vì hiển thị rác; không giải mã.
- **TUI:** con trỏ đồng bộ được lưu bền và nhận cảm xúc theo thời gian thực qua `lp_control`.

### Lưu ý

- Dữ liệu người dùng nằm ở `%APPDATA%\QTzalo` (Windows) / `~/.local/share/qtzalo` (Linux). Lần đầu chạy, dữ liệu cũ trong thư mục làm việc được tự động chuyển vào đây.
- `sync_state_<uin>.json` được nâng lên **định dạng 2** (thêm con trỏ `lp_control`). File định dạng 1 vẫn nạp được: QTzalo chỉ tải lại phần sự kiện điều khiển một lần, không mất lịch sử.
- Trên một máy vừa đăng nhập lần đầu, máy chủ Zalo chỉ gửi tin từ sau lần đăng nhập đó; lịch sử cũ (nhất là của nhóm) chưa lấy được. Tính năng "đồng bộ từ điện thoại" vẫn đang được nghiên cứu (định dạng file sao lưu đã được phân tích, chưa có tính năng).
- Bản này **chưa** thay đổi định dạng cache hội thoại hay cơ sở dữ liệu SQLite, nên quay lại 0.1.3 không mất dữ liệu.
- QTzalo không liên kết với VNG / Zalo. Dùng client bên thứ ba có thể vi phạm điều khoản dịch vụ của Zalo.

Xem đầy đủ: [CHANGELOG.md](https://github.com/quangthoai1985/QTzalo/blob/v{{VERSION}}/CHANGELOG.md).
