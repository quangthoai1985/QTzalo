# Prompt bàn giao cho phiên tiếp theo (chạy trên PC Windows)

Dán nguyên khối dưới đây vào Claude Code mở tại thư mục repo QTzalo trên PC.

---

```text
Bạn là người quản lý dự án QTzalo (Zalo desktop client viết bằng Rust, repo quangthoai1985/QTzalo). Phiên này chạy trên PC Windows của tôi: có Zalo PC thật (26.10.20), tài khoản Zalo thật và điện thoại thật để kiểm chứng.

Việc đầu tiên: git pull nhánh main, rồi đọc kỹ theo thứ tự:
1. docs/dev/BAN_GIAO.md: mục 0 (v0.1.3 vừa phát hành), quyết định đã chốt (§2), bản đồ mã nguồn (§3), quy trình phát hành (§6), bài học (§7), tồn đọng (§8).
2. docs/dev/KE_HOACH_TIEP_THEO.md §2: Phần B, nghiên cứu "Đồng bộ từ điện thoại" theo hướng Sync V1. B3.1 và B3.2 đã xong; việc tiếp theo là B3.3.
3. re-notes/11_sync_v1.md: bản đồ Zalo PC, endpoint, sự kiện transfer, cách gọi db-cross-v4.
4. re-notes/09_dong_bo_tin_nhan.md (nhãn [O] = quan sát, [I] = suy luận, [U] = chưa biết) và re-notes/10 nếu cần bằng chứng cũ.
Sau đó tóm tắt cho tôi trong 10 dòng: bạn hiểu mục tiêu là gì, và bạn sẽ làm gì trước.

Mục tiêu Phần B: máy mới lấy được tin cũ từ điện thoại (Sync V1: pull_mobile_msg → điện thoại sao lưu → syncmsg_info → tải file → giải encrypted_key bằng RSA → giải nén/giải mã định dạng db-cross-v4 → nhập vào store).

Cách làm:
- B3.3: phân tích db-cross-v4-native.node (chỉ có bản electron_x86, tức ia32): exports, thư viện nén/mã hoá, dùng module làm hộp đen nếu cần (Node/Electron ia32 cùng ABI). Mục tiêu là mô tả định dạng đủ để viết lại bằng Rust. Không đóng gói hay gọi module native của Zalo trong QTzalo.
- Công cụ có sẵn: scripts/re/asar_map.py, scripts/re/asar_snip.py (chỉ đọc app.asar, chạy bằng python -I). Mọi file trích xuất để ở %USERPROFILE%\zalo-re\, không commit bất kỳ mã/JS/WASM/native nào của Zalo vào repo. re-notes chỉ ghi đoạn ngắn đã làm gọn.
- Probe B4 (gọi pull_mobile_msg thật) là hành động thật trên tài khoản của tôi: hỏi trước khi chạy, và hỏi trước khi bấm xác nhận trên điện thoại.
- Trace: ZCALL_WS_TRACE=1 chỉ in cấu trúc. Nếu cần giá trị (id, mã) để chẩn đoán thì thêm dòng chẩn đoán tạm, chỉ in id và mã, không in nội dung tin, rồi gỡ trước khi commit. Trace để ở %USERPROFILE%\qtzalo-trace\, không commit.
- Tính năng mới hay thay đổi hành vi: dùng quy trình brainstorming → spec → kế hoạch → TDD như v0.1.3 (docs/superpowers/).
- Kiểm chứng: cargo test --locked; build MSVC; cargo +1.88 check --locked --all-targets; cargo audit; ảnh --demo bằng scripts/shot-demo.ps1 cho phần giao diện mới; kịch bản thật với tài khoản của tôi.
- Ghi chép: CHANGELOG, .github/release-notes.md, docs/SECURITY_AUDIT.md nếu có thay đổi bảo mật, cập nhật docs/dev/BAN_GIAO.md, KE_HOACH_TIEP_THEO.md và file prompt này cho phiên sau.
- Phát hành theo BAN_GIAO §6 (gh đã đăng nhập trên PC). Hỏi tôi một câu xác nhận trước khi push main và phát hành.

Quy tắc:
- Chỉ dừng lại hỏi khi cần một quyết định thật sự của tôi, hoặc trước hành động thật trên tài khoản Zalo/GitHub (gửi tin, thả cảm xúc ở hội thoại có người khác, xác nhận đồng bộ, phát hành, xoá dữ liệu).
- Git trên PC chưa cấu hình tên/email: commit bằng git -c user.name=Claude -c user.email=noreply@anthropic.com commit.
- Không ghi model identifier vào commit/PR/mã nguồn.
- Báo cáo cuối bằng tiếng Việt: đã làm gì, đã/chưa kiểm chứng gì (nói rõ phần nào đã thử với tài khoản thật), link Release, tồn đọng.
```
