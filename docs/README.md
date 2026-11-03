# Tài liệu QTzalo

Tài liệu cho người dùng và người phát triển QTzalo (`zalo-rust`).

## Cho người dùng

| Tài liệu | Nội dung |
|---|---|
| [Cài đặt](CAI_DAT.md) | Tải và cài trên Windows / Linux / macOS, kiểm tra SHA256, xử lý SmartScreen, gỡ cài đặt. |
| [Hướng dẫn sử dụng](HUONG_DAN_SU_DUNG.md) | Tham số dòng lệnh, thao tác GUI, phím tắt TUI. |
| [Xử lý sự cố](KHAC_PHUC_SU_CO.md) | Mã lỗi máy chủ Zalo, font, phiên đăng nhập, giờ hiển thị, lỗi build. |

## Cho người phát triển

| Tài liệu | Nội dung |
|---|---|
| [Kiến trúc hệ thống](KIEN_TRUC_HE_THONG.md) | Phân tách UI thread / Tokio worker, WebSocket + HTTP polling, lưu trữ SQLite. |
| [Đặc tả giao thức Zalo](GIAO_THUC_ZALO.md) | `zpw_ver=694`, `ParamCipher v2`, `PasswordCipher`, `signkey`, mã hoá AES. |
| [Bàn giao dự án](dev/BAN_GIAO.md) | Trạng thái hiện tại, quyết định đã chốt, quy trình build/phát hành, phạm vi phiên bản kế tiếp, bài học điều phối agent. |
| [Kế hoạch tiếp theo](dev/KE_HOACH_TIEP_THEO.md) | Kế hoạch đang thực hiện (Phần A đã xong ở 0.1.2; Phần B "Đồng bộ từ điện thoại" theo hướng Sync V1, các bước B3.x). |
| [Rà soát bảo mật](SECURITY_AUDIT.md) | Các vấn đề bảo mật và lỗi chức năng đã phát hiện, trạng thái khắc phục. |
| [Báo cáo kiểm toán 03/11/2026](dev/BAO_CAO_KIEM_TOAN_2026-11-03.md) | Kiểm toán toàn bộ 41 file Rust: tính năng đã có, 40 việc tồn đọng kèm `file:dòng`, đề xuất thứ tự sửa. |
| [Kế hoạch production](KE_HOACH_PRODUCTION.md) | Lộ trình kế thừa từ dự án gốc. |
| [../re-notes/](../re-notes/) | Ghi chú reverse-engineering giao thức (đăng nhập, dữ liệu chat, cuộc gọi). |
| [re-notes/12](../re-notes/12_zalo-electron-va-dung-luong.md) | Vì sao Zalo PC (Electron) ổn định nhưng phình dung lượng: số đo thật trên máy và bài học cho QTzalo. |
| [re-notes/13](../re-notes/13_db_cross_v4.md) | Bản đồ nhị phân `db-cross-v4` (Phần B, B3.3): PE32 ia32, export ẩn sau `_register_addon_`, XZ/LZMA, AES tự viết, MD5, container nhiều DB, tên trường protobuf. |
| [re-notes/14](../re-notes/14_db_cross_v4_disasm.md) | Disassemble `db-cross-v4` bằng `dumpbin`: địa chỉ 4 hàm N-API, magic header `ZDB4.0`, AES-256 + PKCS#7, liblzma/XZ; KDF và thứ tự nén↔mã hoá còn mở. |
| [re-notes/15](../re-notes/15_db_cross_v4_decrypt.md) | Chốt định dạng `db-cross-v4`: layout header byte (magic `ZDB4.0`, XXH32, danh sách file), KDF = password zero-pad, AES-256-CBC IV 0, thứ tự giải mã→header→giải nén, pseudocode Rust. |
| [re-notes/16](../re-notes/16_db_cross_v4_payload_framing.md) | Framing payload `db-cross-v4`: mỗi hội thoại một blob, bố cục blob (u32 + 32 hex MD5 bị XOR + 32 B + 16 B + 16 B + khối 16 B), khoá XOR = 32 hex của `MD5(password)`, pseudocode `decrypt()`. |

## Build và chạy nhanh

Yêu cầu Rust ≥ 1.88, CMake, `pkg-config`, trình biên dịch C. Chi tiết theo từng hệ điều hành xem [README](../README.md#tự-build-từ-mã-nguồn).

```bash
git clone https://github.com/quangthoai1985/QTzalo.git
cd QTzalo
CMAKE_POLICY_VERSION_MINIMUM=3.5 cargo build --release --locked
./target/release/zalo-rust
```

Các lệnh hữu ích khi phát triển:

```bash
cargo test --locked
```

```bash
cargo +1.88 check --locked --all-targets   # kiểm tra phiên bản Rust tối thiểu (MSRV)
```

```bash
cargo audit                                # lỗ hổng đã biết; cảnh báo unmaintained không làm fail
```

```bash
scripts/check-windows.sh
```

```bash
./target/release/zalo-rust --gui --dev
```
