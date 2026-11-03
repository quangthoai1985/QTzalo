# Nhật ký thay đổi

Mọi thay đổi đáng chú ý của QTzalo được ghi tại đây. Định dạng theo [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/), phiên bản theo [Semantic Versioning](https://semver.org/lang/vi/).

QTzalo là bản fork của [ctdozlite](https://github.com/ctdoteam1110/ctdozlite), tách ra từ commit `73849bd` (tag `v0.0.2` của upstream).

## [Unreleased]

Trọng tâm: sửa các lỗi phát hiện trong đợt kiểm toán mã nguồn ngày 03/11/2026
(`docs/dev/BAO_CAO_KIEM_TOAN_2026-11-03.md`), làm cho lỗi gửi/nhận không còn bị coi là thành công,
và giới hạn dung lượng cache trên đĩa. **Chưa tăng phiên bản, chưa phát hành.**

### Thêm mới

- **Huỷ tải:** mọi tệp/ảnh/video đang tải đều có nút **Huỷ**.
  - `src/download.rs`: `CancelFlag`, `CANCELLED_ERROR`, `download_to_dir_with_cancel`; vòng tải kiểm cờ
    giữa các đoạn, xoá `.part` khi bị huỷ.
  - `WorkerCmd::CancelDownload { url }` + sổ cờ theo URL trong worker;
    `GuiEvent::DownloadCancelled`/`VideoCancelled`; `DownloadState::Cancelled` hiện "Đã huỷ tải · Tải lại".
  - Card "Đang tải video…" nay thật sự dừng tiến trình tải (trước chỉ ẩn card).
  - Dọn entry `Cancelled`/`Failed` cũ trong map tải về (giữ tối đa 64).
- **Thanh trạng thái:** `status_msg` được render ở thanh dưới cửa sổ, nên mọi phản hồi của worker
  (lỗi gửi, làm mới thất bại, mất mạng, lỗi cuộc gọi) không còn biến mất. Lỗi gửi tin có thêm toast.
- **Dọn cache khi khởi động:** `paths::trim_cache_dir` xoá tệp cũ hơn 30 ngày rồi cắt bớt theo thứ tự
  cũ nhất cho tới khi `stickers_cache/` ≤ 64 MiB. Zalo PC không dọn cache của nó (22 GB sau ~3 tháng,
  `re-notes/12`), nên QTzalo đặt giới hạn ngay từ đầu.
- **TUI dùng con trỏ bền:** `TuiApp` nạp/ghi `sync_state_<uin>.json` (debounce 2 s, lưu khi thoát,
  xoá khi đăng xuất) — trước đây TUI luôn tải lại từ `lastActionId = 1`.
- **GUI đăng nhập mật khẩu** nạp lại con trỏ đã lưu của tài khoản đó (trước chỉ nhánh QR làm việc này).
- **`src/reaction.rs`:** API rollback cho cảm xúc lạc quan (`rollback_mine`, `snapshot_mine`,
  `restore_mine`), hàng đợi sự kiện cảm xúc cho tin chưa nạp (`apply_deferred`) và log có giới hạn
  khi bỏ sự kiện — cảm xúc trên tin ngoài cửa sổ 200 tin không còn mất im lặng.
- **Chỉ báo "đang nhập" có thật:** `call::extract_typing_events` đọc control `typing`/`typing_end`
  từ socket `602` và `lp_control`; `GuiEvent::Typing` cập nhật `typing_indicators`, tự hết hạn sau
  6 s và tôn trọng công tắc *Bạn sẽ thấy bạn bè đang soạn tin nhắn* (trước chỉ có dữ liệu demo).
- **Con trỏ `lp_control` bền:** `CtrlPollState` (queue `603_1`/`604_0`) nay lưu trong
  `sync_state_<uin>.json` (định dạng **version 2**, file version 1 vẫn nạp được và chạy lại
  control một lần) — trước mỗi lần mở app phải tải lại toàn bộ control event.
- **Huỷ cảm xúc khi gửi lỗi:** GUI chụp `snapshot_mine` trước khi gỡ, và `GuiEvent::ReactionFailed`
  gọi `rollback_mine`/`restore_mine` để bong bóng trở về đúng trạng thái trước khi thả.
- **Tải tệp/video dài:** thêm `Session::streaming_http()` — cùng cookie jar/UA/header với client API
  nhưng **không** đặt timeout tổng, chỉ `read_timeout` 60 s; worker dùng nó cho tải đính kèm và video.
- **Double-click trả lời** tôn trọng công tắc *Nhấp đúp để trả lời* (trước luôn bật).
- **Bỏ dữ liệu hiển thị không thật:** dấu "Đang hoạt động"/"Trực tuyến" chỉ hiện khi bật
  *Hiển thị trạng thái hoạt động*; "Tổng số tin nhắn" → "Số tin đang hiển thị"; mục ảnh/video đã
  chia sẻ liệt kê cả video (bấm mở player) và đếm đúng; sidebar chỉ hiện dung lượng thật thay cho
  thanh % với mẫu số 1 GB cứng; bỏ hậu tố "· Đã nhận" khi không có dữ liệu đã đọc.
- **Cài đặt trung thực:** 20 công tắc/ô chọn chưa được backend đọc (`autostart_on_boot`,
  `remember_login`, `language`, các mục quyền riêng tư/lời mời, `quick_messages_enabled`,
  `hidden_chat_pin_enabled`, `split_priority_inbox`, `excel_paste_mode`, `suggest_mentions`,
  `enable_gif_search`, …) nay hiện **disabled** kèm chip "(sắp có)" và tooltip, thay vì bật/tắt
  mà không có tác dụng. Các mục đang chạy thật giữ nguyên.
- **Tin mã hoá đầu cuối (E2EE):** `chat::e2ee_placeholder_needed` nhận diện theo trường tường minh
  (`e2eeStatus`/`e2ee`) và hiện bong bóng giữ chỗ *"[Tin nhắn mã hoá đầu cuối — xem trên điện thoại]"*
  thay vì rác; không cố giải mã, không gọi mạng, tin thường không bị ảnh hưởng (có test chống hồi quy).
  Đánh dấu **[I]**: tài khoản thật hiện có `e2eeStatus = 0` nên chưa có tin thật để kiểm chứng.
- **Phần B (bước B3.2 phía mã):** `call::extract_transfer_events` đọc 4 sự kiện chuyển dữ liệu
  (`user_confirm`, `transfer_error`, `syncmsg_info`, `transfer_after_login`) từ `controls[]`;
  `BackupInfo` chỉ giữ **cấu trúc + độ dài** (không giữ URL có token, `encrypted_key`, `temp_key`),
  và worker ghi cấu trúc đó vào `voip.log` khi `--dev` để thu bằng chứng `act_type` còn thiếu.
  Chưa gọi `pull_mobile_msg`, chưa tải/giải mã file sao lưu.
- **Nén cơ sở dữ liệu lịch sử:** `store::MessageStore::compact` (WAL checkpoint TRUNCATE + VACUUM,
  **không xoá tin**) và `compact_if_needed`; gọi lúc khởi động khi DB vượt **32 MiB**
  (`store::AUTO_COMPACT_MIN_BYTES`) — chống phình file/WAL như Zalo (`re-notes/12`).
- **TUI nhận cảm xúc theo thời gian thực:** vòng poll nền của TUI poll thêm `lp_control`
  (`call::poll_controls`, chặn 4 s/lượt) với con trỏ lấy từ `sync_state_<uin>.json`, áp
  `reaction::extract_events` → `apply_to_threads`; con trỏ được lưu theo cơ chế debounce sẵn có.
  Trước đây TUI chỉ thấy cảm xúc sau khi nạp lại hội thoại.
- **Chặn thao tác trên tin chưa có mã máy chủ:** `chat::delete_message_for_me`/`recall_message`
  từ chối id rỗng/`out_*`/`local_*` kèm thông điệp tiếng Việt, thay vì gửi id vô nghĩa.
- **Giảm ghi đĩa:** `chat::save_threads_cache` chỉ ghi khi nội dung thực sự đổi (so hash trong
  process + kiểm file tồn tại), không còn serialize/ghi toàn bộ lịch sử mỗi vòng.
- **Nút "Dọn dẹp ngay"** trong *Cài đặt → Dữ liệu & làm mới*: dọn cache ảnh/video (tệp >30 ngày, giữ
  ≤64 MiB) và nén CSDL lịch sử (WAL checkpoint + VACUUM) ngay, không cần khởi động lại, không xoá tin.
- **Xoá/thu hồi dùng `cliMsgId` thật của tin** (trước luôn gửi `ts` ms, và gửi `now` khi thiếu `ts`);
  nếu tin không có cả `cliMsgId` lẫn thời điểm gửi thì từ chối với lỗi tiếng Việt thay vì gửi id bịa.
- **Test:** 225 → **269** test qua (1 bỏ qua).

### Sửa lỗi

- **Lịch sử nạp sai tin:** `store::messages_for` trả `limit` tin **mới nhất** (trước trả tin cũ nhất),
  nên hội thoại > 200 tin hiển thị đúng sau khi khởi động lại.
- **SQLite:** bật `journal_mode=WAL`, `busy_timeout=5000`, `synchronous=NORMAL`; `delete_message` chỉ
  xoá đúng `msg_id` (trước xoá mọi tin cùng `ts`).
- **Gửi tin không còn "thành công giả"** (`src/chat.rs`):
  - mọi hàm gửi đều đọc `error_code` (`send_reaction` trước đây bỏ qua);
  - tải lên từng phần thất bại thì không gửi tin (trước `break` im lặng rồi vẫn gửi);
  - gửi thẻ liên kết/trả lời thất bại trả lỗi thay vì âm thầm đổi thành tin văn bản;
  - trích dẫn tin chưa có mã máy chủ bị từ chối thay vì bịa `msgId` (`now - 1000`);
  - timeout long-poll không còn bị coi là "không có tin mới".
- **Đủ 13/13 hành động ghi trong `chat.rs`** kiểm envelope qua một helper duy nhất
  (`ensure_action_ok`): thêm xoá phía tôi, thu hồi, huỷ/từ chối lời mời kết bạn; thẻ liên kết và
  trích dẫn trả kèm `error_message` của máy chủ; `get_group_history` ghi lại lỗi gốc trước khi fallback.
- **Cuộc gọi video bị đăng ký thành thoại:** nhánh `401` đọc `typeRequest` trước `type`
  (`call::call_type_from_payload`), nên video call từ GUI gửi đúng loại.
- **Âm thanh/video cuộc gọi:**
  - tìm `ffmpeg` qua `video::find_ffmpeg` (nhận bản WinGet), không còn chỉ dò `PATH`;
  - không gửi khung H.264 tổng hợp khi không có frame camera (bật lại bằng `ZALO_CALL_FAKE_VIDEO=1`);
  - không fallback loopback mic→loa của chính mình khi media lỗi (bật lại bằng `ZALO_CALL_LOOPBACK=1`),
    thay vào đó báo lỗi rõ trong UI.
- **Rò rỉ log:** payload signaling cuộc gọi đã giải mã chỉ được in khi `--dev`/`ZALO_CALL_TRACE=1`.

### Tài liệu và CI

- Sửa tài liệu theo thực tế: `README` (macOS best-effort, quy trình phát hành, `sync_state_<uin>.json`,
  sơ đồ mã nguồn đủ 24 module), `docs/KIEN_TRUC_HE_THONG.md` (9 file `gui/`, 4 bảng SQLite),
  `docs/CAI_DAT.md` (phiên bản 0.1.3), `docs/HUONG_DAN_SU_DUNG.md` (đủ tham số CLI), `docs/README.md`.
- CI: thêm bước **MSRV 1.88** và **`cargo audit`** vào job `test` (audit fail khi có lỗ hổng, cảnh báo
  `unmaintained` không làm fail); ghim phiên bản Inno Setup (`6.7.1`) thay vì cài bản mới nhất.
- Thêm `docs/dev/BAO_CAO_KIEM_TOAN_2026-11-03.md` (báo cáo kiểm toán đầy đủ),
  `re-notes/12_zalo-electron-va-dung-luong.md` (vì sao Zalo Electron ổn định nhưng phình dung lượng,
  số đo thật trên máy và bài học cho QTzalo), `re-notes/13_db_cross_v4.md` (bản đồ PE của module
  `db-cross-v4`), `re-notes/14_db_cross_v4_disasm.md` (disassemble bằng `dumpbin`: sửa địa chỉ
  4 hàm N-API, chốt magic header **`ZDB4.0`** big-endian, **AES-256 + PKCS#7**, **liblzma/XZ**) và
  `re-notes/15_db_cross_v4_decrypt.md` (**chốt định dạng**: layout header byte, hash header **XXH32**,
  **KDF = password zero-pad 32 byte**, **AES-256-CBC IV 0**, thứ tự **giải mã → parse header → giải nén**,
  kèm pseudocode Rust; MD5 payload và IV nhiều chunk còn **[U]**).

## [0.1.3] - 2026-11-03

Trọng tâm: thả cảm xúc ngay khi rê chuột, và thấy cảm xúc của mọi người theo thời gian thực. Bằng chứng thu với tài khoản thật: `re-notes/10_bang_chung_dong_bo.md` §17–18.

### Thêm mới

- **Thả cảm xúc khi rê chuột:**
  - Rê chuột vào tin thì hiện một nút tròn cạnh bong bóng; bấm vào là thả 👍.
  - Dừng chuột trên nút thì bung thanh 6 cảm xúc. Nếu bạn đã thả, thanh có thêm nút ✕ để gỡ.
- **Đếm cảm xúc giống Zalo:**
  - mỗi lần bấm cộng thêm một; chọn cảm xúc khác thì thêm cạnh cảm xúc cũ;
  - nút ✕ gỡ hết cảm xúc của bạn trên tin;
  - rê lên chip thì hiện chi tiết, ví dụ "❤️ 3 · 😆 1 — có bạn".
- **Cảm xúc theo thời gian thực:**
  - Cảm xúc của người khác, và của chính bạn thả từ điện thoại, hiện ngay (socket `612`).
  - Cảm xúc thả lúc QTzalo tắt được bù khi mở app (queue `610`/`611` và `lp_control`).

### Sửa lỗi

- Thả cảm xúc không còn xoá cảm xúc của người khác trên cùng tin.
- Cảm xúc trên **tin của chính bạn** (kể cả mọi tin trong "Cloud của tôi") nay hiện đúng cả hai chiều. Zalo định danh những tin này bằng `cliMsgId`; trước đây QTzalo gửi sai id và bỏ qua sự kiện từ điện thoại.
- `lp_control` (kênh dự phòng cho sự kiện cuộc gọi): khi danh sách id của queue dài, mọi lần kéo sau lần đầu đều bị máy chủ trả 413. Nay QTzalo chỉ gửi 10 id mới nhất, như Zalo PC.

### Ghi chú

- Cảm xúc được lưu theo dạng mới (từng người). Nếu quay lại bản 0.1.2, cache hội thoại sẽ bị bỏ qua; lịch sử trong SQLite vẫn còn.

## [0.1.2] - 2026-11-02

Trọng tâm: mở QTzalo sau một thời gian tắt máy thì tin nhắn tự đồng bộ đủ như trên điện thoại. Mọi thay đổi dựa trên bằng chứng thu với tài khoản thật (`re-notes/10_bang_chung_dong_bo.md`).

### Thêm mới

- **Tự bắt kịp tin nhắn khi mở app**
  - Nhớ vị trí đã đồng bộ của từng hàng đợi tin nhắn, theo từng tài khoản (`sync_state_<uin>.json`). Lần mở sau chỉ lấy phần mới; trước đây app tải lại mọi thứ từ lúc đăng nhập.
  - Kéo bù tới khi máy chủ báo hết (trước đây tối đa 3 đợt), cho cả tin 1-1 và tin nhóm.
  - Dải "Đang đồng bộ tin nhắn… N tin" dưới đầu danh sách hội thoại, sau đó hiện "Đã đồng bộ N tin nhắn".
  - Khởi động lúc chưa có mạng thì tự thử lại sau 5/15/30/60 giây, thay vì bỏ qua.
- **Báo thiếu tin**: khi máy chủ báo đã bỏ bớt một phần hàng đợi, app hiện banner "Tin nhắn chưa được đồng bộ đủ" kèm khoảng thời gian có thể thiếu và lời nhắc xem trên điện thoại. Có nút đóng.
- **Danh sách hội thoại giống điện thoại**: tab Tin nhắn chỉ hiện hội thoại có hoạt động, sắp theo thời điểm hoạt động cuối (lấy từ máy chủ, kể cả trên máy mới chưa có tin). Ô tìm kiếm vẫn tìm được mọi bạn bè và nhóm.
- **Lịch sử "Cloud của tôi"/nhóm liền mạch**: khi trang tin mới nhất chưa chạm tới tin đã có trên máy, app tải thêm các trang cũ hơn.
- **Ảnh, video, tệp trong tin nhắn**
  - Không còn hiện đường link: ảnh hiện thành ảnh; video có ảnh xem trước, nút ▶, thời lượng và dung lượng; tệp có tên và dung lượng; tin thoại có thẻ riêng. Tin cũ đã lưu dạng link cũng được nhận ra.
  - Bấm ảnh → cửa sổ "Xem ảnh" riêng của app (phóng to, kéo, tải về).
  - Bấm video → tải vào cache rồi phát trong cửa sổ "Video" của QTzalo: có tiếng, tạm dừng, tua, tắt tiếng, phím tắt. Cần `ffmpeg` trên máy; không có thì mở bằng trình phát mặc định của Windows.
  - Bấm tệp → tải về "Thư mục lưu file" rồi mở bằng ứng dụng mặc định.
  - Không chỗ nào còn mở trình duyệt.

### Sửa lỗi

- **Tên nhóm hiện thành "Nhóm <số>"**: app hỏi thông tin nhóm kèm đúng phiên bản máy chủ vừa đưa, nên máy chủ không trả tên nào. Ngoài ra, tên thật bắt đầu bằng chữ "Nhóm …" bị coi là tên tạm và bị ghi đè khi có tin mới. Hai nhóm khác nhau cũng không còn bị gộp chỉ vì trùng tên hoặc trùng ảnh mặc định.
- **Long-poll lặp liên tục khi có tin nhóm chờ**: máy chủ giữ tin nhóm ở một hàng đợi mà app không theo dõi, nên trả mãi cùng một trang. App nay theo dõi mọi hàng đợi tin nhắn máy chủ báo, và dừng khi con trỏ không tiến.
- Lần làm mới lúc khởi động có thể bị kẹt và không bao giờ tải lịch sử nhóm.

### Bảo mật

- `sync_state_<uin>.json` chỉ chứa con trỏ hàng đợi (không có khoá, không có nội dung). File được ghi nguyên tử với quyền chủ sở hữu, `uin` được kiểm tra chỉ gồm chữ số, và file bị xoá khi đăng xuất (S17).
- Phát video bằng `ffmpeg` (S18):
  - chỉ chạy `ffmpeg.exe` thật (không chạy `.cmd`/`.bat`);
  - tham số truyền thẳng, không qua shell;
  - `-protocol_whitelist file` và danh sách định dạng cho phép;
  - chỉ phát tệp trong thư mục cache của QTzalo;
  - khung hình tối đa 1280 px;
  - đóng cửa sổ thì dừng mọi tiến trình con.
- Trace gỡ lỗi `ZCALL_WS_TRACE=1` chỉ ghi cấu trúc (tên trường, số lượng, con trỏ); nội dung tin chỉ còn độ dài (S19).

### Phát triển

- Module mới: `src/sync_state.rs` (con trỏ bền, tín hiệu thiếu tin, đồng hồ `msgId` → thời gian), `src/trace_shape.rs`, `src/video.rs`, `src/gui/media_view.rs`.
- Bảng SQLite mới `missing_ranges`, `thread_activity`.
- Biến demo mới: `QTZALO_DEMO_SYNC=1|done`, `QTZALO_DEMO_MISSING=1`, `QTZALO_DEMO_VIDEO=<mp4>`, `QTZALO_DEMO_IMAGE=1`, `QTZALO_DEMO_VIDEO_PENDING=1`.
- `QTZALO_FFMPEG=<đường dẫn ffmpeg.exe>` chỉ định `ffmpeg` dùng để phát video. Biến thử nghiệm `QTZALO_LP_QUEUES` (chỉ khi `--dev`).

## [0.1.1] - 2026-11-01

### Thêm mới

- **Tải tệp, video và ảnh gốc về "Thư mục lưu file"**
  - Nút tải về trên thẻ tệp, thẻ video, ảnh trong khung chat (và trong menu chuột phải), cùng nút "Tải về" trên thanh công cụ trình xem ảnh.
  - Hiển thị tiến trình (phần trăm khi máy chủ báo dung lượng), trạng thái "Đã tải" với nút **Mở** / **Mở thư mục**, báo lỗi rõ ràng và nút **Thử lại**.
  - Tải xong hiện thông báo có nút **Mở** và **Mở thư mục**.
  - Tải theo luồng thẳng xuống đĩa, không giữ cả file trong RAM.
- **Quản lý cache ảnh** trong *Cài đặt → Dữ liệu & làm mới → Lưu trữ trên máy*: xem đường dẫn thư mục dữ liệu (nút "Mở thư mục dữ liệu"), dung lượng cache ảnh (tính ở luồng nền), nút **Xoá cache ảnh** có hộp xác nhận. Chỉ xoá `stickers_cache/`; phiên đăng nhập, lịch sử tin nhắn và cài đặt không bị đụng tới. Ảnh tự tải lại sau khi xoá.
- **Số thành viên nhóm thật** ở header khung chat ("N thành viên"), lấy từ trường `totalMember` của danh sách nhóm nếu máy chủ trả về; nếu không có vẫn hiện "Nhóm".

### Sửa lỗi

- Ảnh dán từ clipboard không còn bị ghi vào thư mục đang đứng khi chạy app; nay nằm trong cache của thư mục dữ liệu và được dọn sau 24 giờ.
- Nút **Thay đổi** thư mục lưu file dùng hộp chọn thư mục của hệ điều hành (trước đây phải chọn một file rồi lấy thư mục cha, không chọn được thư mục rỗng).
- Bố cục cửa sổ Cài đặt bị dàn ngang và tràn màn hình; nút "Làm mới hội thoại" mất chữ ở giao diện sáng.

### Bảo mật

- Tính năng tải về coi URL và tên file trong tin nhắn là dữ liệu không tin cậy (S15 trong `docs/SECURITY_AUDIT.md`):
  - chỉ tải qua `https://` (`http://` chỉ cho CDN Zalo), kiểm tra lại sau chuyển hướng;
  - làm sạch tên file (ký tự cấm, ký tự điều khiển và ký tự đảo chiều U+202E, tên thiết bị Windows như `CON`/`COM1`, độ dài ≤ 150 ký tự);
  - không bao giờ ghi đè file có sẵn (`tên (1).ext`), ghi vào `*.part` rồi mới đổi tên;
  - trên Windows gắn Mark-of-the-Web (`Zone.Identifier`, `ZoneId=3`) để SmartScreen/Office cảnh báo đúng;
  - không tự mở file thực thi/script (`.exe .msi .bat .cmd .ps1 .js .lnk .jar .reg .iso`…): nút **Mở** chỉ mở thư mục chứa file.
- Dependency: nâng `ratatui` 0.30 và `crossterm` 0.29, loại bỏ `lru 0.12.5` (unsound, RUSTSEC-2026-0002 / RUSTSEC-2026-0253) và `paste` (hết bảo trì). Cập nhật patch: `log`, `memmap2`, `rustix`, `smallvec`, `unicode-width`, `webbrowser`, `webpki-roots`, `zune-core`. `cargo audit`: 0 lỗ hổng, còn 2 cảnh báo hết bảo trì (`audiopus_sys`, `ttf-parser`).

### Phát triển

- Module mới `src/download.rs` (chính sách tải tách khỏi I/O, có unit test và test end-to-end với server cục bộ).
- Biến môi trường cho chế độ `--demo`: `QTZALO_DEMO_DOWNLOADS=1`, `QTZALO_DEMO_SETTINGS=<tab>`, `QTZALO_DEMO_CONFIRM_CLEAR=1`.

## [0.1.0] - 2026-11-01

Phiên bản đầu tiên của QTzalo.

### Bảo mật

- **Lưu dữ liệu an toàn**
  - `session.json`, lịch sử `zalo_history.db`, `threads_cache.json`, `app_settings.json`, cache ảnh, log và dump nay nằm trong một thư mục riêng của người dùng:
    - Windows: `%APPDATA%\QTzalo`
    - Linux: `~/.local/share/qtzalo`
    - macOS: `~/Library/Application Support/QTzalo`
  - Trước đây các file này nằm ở thư mục đang đứng khi chạy app.
  - File chứa thông tin đăng nhập được ghi nguyên tử với quyền `0600`, thư mục có quyền `0700`.
  - File của bản cũ được tự động chuyển sang khi khởi động.
  - Đặt biến `QTZALO_DATA_DIR` để đổi vị trí lưu.
- **Tắt log chẩn đoán mặc định:** `call_trace.log` (chứa payload báo hiệu cuộc gọi đã giải mã) và `voip.log` chỉ được ghi khi chạy `--dev` hoặc đặt `ZALO_CALL_TRACE=1` / `ZALO_VOIP_LOG`.
- **Link preview không còn làm lộ IP**
  - Tin nhắn đến chỉ dùng kết quả `parselink` phía máy chủ Zalo.
  - App chỉ truy cập trực tiếp với link do chính bạn gửi: chỉ `https`, chặn IP nội bộ/loopback, ghim DNS, kiểm tra lại từng lần chuyển hướng, giới hạn 256 KiB.
  - Ảnh: chỉ tải qua `https` (`http` chỉ cho CDN Zalo), giới hạn 15 MiB, không đọc đường dẫn mạng/UNC.
- **Sinh số ngẫu nhiên an toàn:** IMEI/device-id, session cuộc gọi, salt mật khẩu và khoá tham số dùng nguồn ngẫu nhiên của hệ điều hành (`getrandom`) thay cho bộ sinh theo thời gian.
- **WebSocket**
  - Cookie phiên chỉ gửi tới `wss://` thuộc domain Zalo; địa chỉ `ws://` không mã hoá bị loại.
  - `ZCALL_WS` chỉ có hiệu lực ở chế độ `--dev`.
  - Dữ liệu giải nén giới hạn 32 MiB.
  - Cookie HTTP thêm cờ `Secure`.
- **Mật khẩu**
  - Bỏ tham số `--pass <mật khẩu>` (lộ trong danh sách tiến trình), thêm `--pass-stdin`.
  - Ô nhập mật khẩu trong terminal không còn hiện ký tự.
- **Dependency**
  - Nâng `tokio` lên 1.54.1 (vá RUSTSEC-2026-0384) và `clipboard-win` lên 5.4.3 (bản cũ đã bị yank).
  - `cargo audit` báo 0 lỗ hổng.
  - Còn 5 cảnh báo: `lru` unsound qua ratatui 0.29; `audiopus_sys`, `paste`, `ttf-parser` hết bảo trì.
- Xoá đường dẫn máy phát triển còn sót (`F:\CODE\...`) và các đường dẫn ZaloCall.exe tương đối.

### Sửa lỗi

- **Mất phiên đăng nhập khi rớt mạng (#21, #3)**
  - Chỉ mã lỗi xác thực rõ ràng (`600`, `-102`) mới bị coi là hết hạn.
  - Lỗi mạng, lỗi HTTP hay phản hồi không đọc được: giữ nguyên phiên và báo "Không kết nối được máy chủ Zalo".
  - Phiên hết hạn chỉ bị thay thế khi đăng nhập mới thành công.
- **Đăng xuất không còn xoá lịch sử tin nhắn cục bộ** (`zalo_history.db`).
- **Danh sách bạn bè lỗi 404 (#22, #3)**
  - Domain đã kiểm chứng luôn được ưu tiên; `zpw_service_map_v3` từ máy chủ chỉ bổ sung domain còn thiếu.
  - Khi nạp phiên, toàn bộ domain được khôi phục (trước chỉ 3/13).
  - `get_friends` thử host `friend` trước và hiện lỗi thật thay vì danh sách rỗng.
- **Sai giờ hiển thị (#4):** giờ tin nhắn, danh sách hội thoại và log dùng múi giờ của máy (có xử lý giờ mùa hè) thay vì UTC.
- **Gỡ khoá gọi thử nghiệm**
  - Trước đây chỉ gọi được liên hệ có tên chứa "hoang", "do thanh" hoặc "dtc"; nay gọi được mọi liên hệ 1-1.
  - Gọi nhóm báo rõ "Chưa hỗ trợ gọi nhóm".
- Đăng nhập bằng mật khẩu ở chế độ TUI giờ lưu phiên đăng nhập.
- Sửa lỗi tràn số khi cắt ảnh đại diện.

### Giao diện

- **Thiết kế lại toàn bộ theo bố cục 3 cột**, giao diện sáng mặc định, tông xanh nhạt và bo góc mềm. Giao diện tối vẫn chọn được trong Cài đặt.
- **Thanh điều hướng trái** gồm 3 nhóm:
  - Hộp thư: Tin nhắn, Khác, Chưa đọc, Người lạ, Trò chuyện ẩn.
  - Lưu trữ: Cloud của tôi, zCloud kèm thanh dung lượng dữ liệu trên máy.
  - Danh bạ: Bạn bè, Nhóm và cộng đồng, Lời mời kết bạn.
  - Có badge đếm, chân trang hồ sơ kèm nút cài đặt. Mục chưa có tính năng hiện "Sắp có".
- **Danh sách hội thoại**
  - Avatar chữ cái nhiều màu, badge chưa đọc, hàng đang chọn tô xanh nhạt.
  - Ô tìm kiếm có phím tắt Ctrl+K (⌘K trên macOS).
  - Giờ hiển thị kiểu HH:MM / Hôm qua / T2…CN / dd/mm.
- **Thu gọn cột:**
  - Nút thu gọn đưa thanh điều hướng về dạng chỉ còn icon và danh sách hội thoại về dạng chỉ còn avatar.
  - Có hiệu ứng chuyển động, trạng thái được lưu lại.
  - Cửa sổ hẹp hơn 1100 px tự thu gọn thanh điều hướng; hẹp hơn 760 px thu gọn cả hai cột.
- **Khung chat**
  - Nền gradient nhẹ, header có avatar, tên và số thành viên.
  - Chip ngày "Hôm nay" / "Thứ Ba, 27/10".
  - Bong bóng gửi đi màu xanh đặc, nhận vào màu sáng có tên người gửi.
  - Chip reaction, khối trích dẫn, thẻ tệp, chỉ báo "đang nhập".
  - Khung soạn tin có nút (+) mở menu ảnh/tệp/dán clipboard, nút sticker, emoji và nút gửi tròn.
- Cửa sổ mặc định 1280×800, tối thiểu 480×360. Thanh tiêu đề hiện "QTzalo" và phiên bản.
- **Font chữ (#5, #6)**
  - Tự dò font hệ thống theo thứ tự: font Latin hỗ trợ tiếng Việt, font chữ Hán (CJK, đọc được file `.ttc`), rồi font emoji/ký hiệu.
  - Hỗ trợ đường dẫn font của Fedora, Debian/Ubuntu, Arch, macOS và Windows (Segoe UI, Microsoft YaHei, Segoe UI Emoji).
- Hiển thị giờ trên giao diện dùng múi giờ của máy, bỏ phép cộng cứng UTC+7 (#4).
- **Đổi nút "Đồng bộ tin nhắn" thành "Làm mới hội thoại" (#23)**
  - Thông báo đúng việc nút thực sự làm và không còn hiện khi app tự làm mới lúc khởi động.
  - Đồng bộ toàn bộ lịch sử (#19) để phiên bản sau.
- Chế độ `--demo`: giao diện với dữ liệu mẫu offline, không kết nối mạng, không đọc hay ghi dữ liệu thật. Dùng để chụp ảnh và kiểm thử giao diện.
- Sửa lỗi crash `clamp(min > max)` khi khung chat quá hẹp, vạch đen giữa các panel, và lỗi theme hệ thống ghi đè bảng màu.

### Build & phát hành

- Workflow CI mới (`.github/workflows/ci-release.yml`):
  - Lấy phiên bản từ `Cargo.toml` cho mọi tên file và metadata; tag phải khớp phiên bản.
  - Phát hành bằng nút *Run workflow* (tick `release`) hoặc push tag `v<phiên bản>`.
  - Quyền token mặc định chỉ đọc; chỉ job phát hành có quyền ghi.
  - Mọi action bên thứ ba được ghim theo commit SHA.
  - Bỏ bản AppImage, vì `appimagetool` không có bản phát hành cố định để kiểm tra checksum.
  - Release chỉ được tạo khi test và bản Windows build thành công, kèm `SHA256SUMS.txt`.
- Bộ cài Windows (Inno Setup) đổi thương hiệu thành **QTzalo**:
  - `AppId` mới (không đụng bản ctdozlite đã cài) và cài file chạy thành `QTzalo.exe`.
  - Không cần quyền Administrator; yêu cầu Windows 10 trở lên.
  - Khi gỡ chỉ xoá dữ liệu người dùng nếu được đồng ý.
- Bản Windows link tĩnh C runtime (`+crt-static`) nên chạy được trên máy chưa cài Visual C++ Redistributable.
- `build.rs` nhúng icon theo **máy đích** (`CARGO_CFG_TARGET_OS`) thay vì máy build; hỗ trợ MinGW qua `windres`.
- Thêm `scripts/check-windows.sh` để kiểm tra mã dành riêng cho Windows ngay trên Linux.
- Gói Linux đổi tên thành `qtzalo` (`.deb`, `.tar.gz`). `install.sh` hỗ trợ `--user`, `uninstall.sh` hỗ trợ `--purge`.
- Xoá thư mục `web-bundle/` (JavaScript đã làm rối của Zalo, không dùng khi build).

### Tài liệu

- Viết lại `README.md` cho QTzalo: yêu cầu **Rust ≥ 1.88** (đã kiểm chứng 1.87 không build được), vị trí lưu dữ liệu, quy trình phát hành, ghi công dự án gốc.
- Thêm `docs/CAI_DAT.md` (hướng dẫn cài đặt, xử lý SmartScreen), `docs/SECURITY_AUDIT.md` (kết quả rà soát), `SECURITY.md`.
- Bổ sung `docs/KHAC_PHUC_SU_CO.md`: font, phiên đăng nhập, giờ hiển thị.

[0.1.3]: https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.3
[0.1.2]: https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.2
[0.1.1]: https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.1
[0.1.0]: https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.0
