# Bàn giao dự án QTzalo — từ v0.1.3 sang bản tiếp theo

> Tài liệu này dành cho phiên làm việc (người hoặc AI) tiếp theo. Đọc hết trước khi sửa code.
> Cập nhật lần cuối: 2026-11-03, khi phát hành v0.1.3. Mục 0 là phần mới; mục 0a giữ tóm tắt của v0.1.2; mục 1 trở đi giữ nội dung bàn giao của 0.1.1 và được sửa ở những chỗ đã thay đổi. Bản bàn giao trước xem lịch sử git của file này.

## 0. v0.1.3 (2026-11-03) — tóm tắt cho phiên sau

- **Phiên chạy trên PC Windows của người dùng**, với tài khoản Zalo thật và điện thoại thật.
- **Phát hành:**
  - nhánh `claude/v0.1.3` được fast-forward vào `main` (`daa4d6e`), đã push, nhánh cục bộ đã xoá;
  - [Release v0.1.3](https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.3) do workflow `ci-release.yml` build (run `41653890025`), tag `v0.1.3` = `daa4d6e`;
  - có 5 file: bộ cài và bản portable Windows, `.deb`, `.tar.gz`, `SHA256SUMS.txt`;
  - **thiếu `.dmg`**: job macOS (best effort) bị huỷ sau 15 phút chờ máy chạy của GitHub, chưa chạy bước nào. Vì vậy workflow báo `failure`, nhưng job phát hành vẫn chạy vì chỉ cần `test` + `windows` xanh.
- **Đã làm:**
  - **Thả cảm xúc khi rê chuột:** nút tròn ngoài mép bong bóng (bấm = 👍); dừng chuột 250 ms thì bung thanh 6 cảm xúc; nút ✕ gỡ (`src/gui/messages.rs`: `hover_reaction_controls`, `advance_react_bar`, `menu_or_popup_open`).
  - **Nhận reaction theo thời gian thực:** socket `612` lúc đang mở; queue `610_1`/`611_1` và `lp_control` để bù sau khi tắt máy (`src/reaction.rs`, `GuiEvent::ReactionsReceived`).
  - **Đếm cộng dồn giống Zalo** (người dùng chọn): mỗi lần bấm là một mục; gỡ thì xoá hết các lần bấm của người đó.
  - **Tin của chính mình được định danh bằng `cliMsgId`:** thêm trường `ChatMessage.cli_msg_id`; gửi `gMsgID` dạng số và `cMsgID = cliMsgId`.
  - **Sửa `lp_control` bị 413:** chỉ gửi tối đa 10 id mới nhất của mỗi queue (`chat::newest_action_ids`).
  - **Phần B:** B3.1 (bản đồ Zalo PC 26.10.20) và B3.2 (sự kiện transfer `user_confirm` / `transfer_error` / `syncmsg_info`) ở `re-notes/11_sync_v1.md`.
- **Quy trình phiên này:** brainstorming → spec `docs/superpowers/specs/2026-11-02-reaction-hover-realtime-design.md` (có mục §0 ghi các sửa đổi sau nghiệm thu) → kế hoạch `docs/superpowers/plans/2026-11-02-reaction-hover-realtime.md` → làm theo TDD → một agent review toàn nhánh (0 Critical, 3 Important, đã sửa) → nghiệm thu thật.
- **Kiểm chứng:**
  - `cargo test --locked`: 225 qua, 1 bỏ qua.
  - Build MSVC sạch; `cargo +1.88 check --locked --all-targets` sạch; `cargo audit`: 0 lỗ hổng, 2 cảnh báo hết bảo trì.
  - Người dùng xác nhận reaction khớp hai chiều, cả với tin của người khác lẫn tin của chính mình ("Cloud của tôi"), và cách đếm cộng dồn khớp điện thoại.
  - Bằng chứng: `re-notes/10` §17–19.
- **Chưa kiểm chứng:** reaction trong nhóm từ QTzalo (`/api/group/reaction`; nhận reaction nhóm đã thấy trong trace); Linux/macOS; TUI.
- **Phát hiện quan trọng:**
  1. Reaction đến qua `612`, `610_1`/`611_1` và `lp_control`; máy chủ gửi lại cả reaction do chính client gửi.
  2. Gỡ cảm xúc là `rIcon = ""`, `rType = -1`.
  3. Với tin vừa gửi của chính mình, `gMsgID = 0` và tin được định danh bằng `cliMsgId`.
  4. Zalo đếm mỗi lần bấm.
  5. Trace chỉ in cấu trúc thì không thấy những lỗi phụ thuộc vào **giá trị** (`gMsgID = 0`, `cMsgID` sai). Phải thêm dòng chẩn đoán tạm, chỉ in id/mã, mới thấy.
- **Việc tiếp theo:** Phần B bước B3.3 (định dạng `db-cross-v4`), xem `KE_HOACH_TIEP_THEO.md` §2 và `re-notes/11`. Tồn đọng của 0.1.3 ở §8.

## 0b. Sau v0.1.3 (03/11/2026) — đợt sửa lỗi kiểm toán, chưa phát hành

Phiên này đọc toàn bộ mã nguồn, đối chiếu tài liệu và kho nghiên cứu, rồi sửa theo thứ tự đã
thống nhất. Báo cáo đầy đủ: `docs/dev/BAO_CAO_KIEM_TOAN_2026-11-03.md`. CHANGELOG: mục `[Unreleased]`.
**Chưa tăng phiên bản, chưa commit/phát hành ở bước này của phiên** (xem §6 khi phát hành).

- **Đã sửa (P0):**
  - `store::messages_for` trả tin **mới nhất** (trước trả cũ nhất); thêm WAL/`busy_timeout`;
    `delete_message` chỉ xoá đúng `msg_id`.
  - Huỷ tải: `CancelFlag` + `download_to_dir_with_cancel` + `WorkerCmd::CancelDownload` +
    `GuiEvent::{DownloadCancelled, VideoCancelled}` + `DownloadState::Cancelled`; nút Huỷ ở thẻ tệp/ảnh,
    pill ảnh, card video; tải lại được.
  - (vòng 2 đã sửa) client tải tách sang `Session::streaming_http()` — xem bên dưới.
  - TUI và nhánh đăng nhập mật khẩu dùng `SyncState` bền; TUI xoá `sync_state_<uin>.json` khi logout.
  - `call.rs` đọc `typeRequest` trước `type` ⇒ video call không bị đăng ký thành thoại.
  - `status_msg` được render ở thanh trạng thái dưới cửa sổ + toast cho lỗi gửi tin.
- **Đã sửa (P1):** `chat.rs` kiểm `error_code` cho mọi sender, bỏ envelope giả, upload phần lỗi không
  gửi tin, link/quote lỗi trả lỗi thay vì âm thầm gửi text, không bịa id trích dẫn, timeout long-poll
  không còn là "hết tin"; `reaction.rs` có API rollback + hàng đợi sự kiện cho tin chưa nạp;
  `media.rs` dùng `video::find_ffmpeg`, không gửi H.264 tổng hợp, không fallback loopback (đều opt-in);
  log signaling chỉ in khi dev; `paths::trim_cache_dir` dọn cache >30 ngày và cap 64 MiB lúc khởi động.
- **Kiểm chứng (vòng 1):** `cargo test --locked` **250 pass / 0 fail / 1 ignored**; `cargo check --locked
  --all-targets` 0 warning; `cargo +1.88 check --locked --all-targets` sạch; `cargo audit` 0 lỗ hổng,
  2 cảnh báo `unmaintained` (`audiopus_sys`, `ttf-parser`) như trước.
- **Tài liệu/CI:** README + `KIEN_TRUC` + `CAI_DAT` + `HUONG_DAN_SU_DUNG` + `docs/README` sửa theo thực tế;
  CI thêm MSRV 1.88 và `cargo audit` vào job `test`, ghim Inno Setup `6.7.1`; thêm
  `re-notes/12_zalo-electron-va-dung-luong.md` (số đo thật: Zalo chiếm ~24,3 GB, 91,7 % là cache media;
  `%APPDATA%\QTzalo` 64,7 MB nhưng 98,4 % là `stickers_cache` chưa evict) và báo cáo kiểm toán.
- **Đã sửa vòng 2 (hoàn tất Đợt 2):**
  - `Session::streaming_http()` (không timeout tổng, `read_timeout` 60 s, dùng chung cookie jar) nay
    được worker dùng cho tải đính kèm và video → hết rủi ro huỷ tệp lớn sau 35 s.
  - **Chỉ báo "đang nhập" chạy thật:** `call::extract_typing_events` + `GuiEvent::Typing`, hết hạn 6 s,
    tôn trọng công tắc `show_typing_indicator`; `double_click_to_reply` cũng đã có tác dụng.
  - **`CtrlPollState` được lưu** trong `sync_state_<uin>.json` (**version 2**; file version 1 vẫn nạp
    được và chỉ replay control một lần).
  - **Rollback cảm xúc đã nối:** GUI chụp `snapshot_mine` trước khi gỡ và xử lý
    `GuiEvent::ReactionFailed` bằng `rollback_mine`/`restore_mine` (sentinel uid `"0"` — xem bài học).
  - **`chat.rs` đủ 13/13 hành động ghi kiểm envelope** (`ensure_action_ok`), gồm xoá phía tôi, thu hồi,
    huỷ/từ chối lời mời kết bạn; link/quote kèm `error_message`.
  - **Bỏ dữ liệu hiển thị không thật:** presence chỉ hiện khi bật, "Số tin đang hiển thị", mục media
    liệt kê cả video, sidebar chỉ hiện dung lượng thật, bỏ "· Đã nhận".
- **Kiểm chứng (vòng 2):** `cargo test --locked` **257 pass / 0 fail / 1 ignored**;
  `cargo check --locked --all-targets` 0 warning; `cargo +1.88 check --locked --all-targets` sạch;
  `cargo audit` 0 lỗ hổng, 2 cảnh báo `unmaintained` như trước.
- **Tồn đọng còn lại (ưu tiên giảm dần):**
  1. ~18 công tắc Cài đặt còn lại chỉ lưu JSON (autostart, language, quyền riêng tư, quick messages,
     GIF, …) — hoặc nối thật, hoặc ẩn/ghi "sắp có". Chỉ `show_typing_indicator`,
     `double_click_to_reply`, `show_online_status`, `download_dir`, chủ đề, sticker là có tác dụng.
  2. Không ACK hàng đợi (`DELETE_QUEUE_ACK_*`); không nhận diện E2EE (chưa có bong bóng giữ chỗ).
  3. `delete_message_for_me`/`recall_message` chưa chặn `msg.id` kiểu `out_*`/`local_*` (GUI đang chặn).
  4. Bảng `messages` chưa có retention/VACUUM; `threads_cache.json` vẫn ghi toàn bộ mỗi vòng.
  5. TUI chưa có cuộc gọi/reaction realtime (ngoài phạm vi các đợt này).
  6. `GUI` vẫn chưa có tab "Đồng bộ từ điện thoại"; Phần B vẫn chưa có dòng mã nào (chỉ nghiên cứu).
- **Việc tiếp theo:** Phần B bước B3.3 (định dạng `db-cross-v4`). Vòng 2 đã lập **bản đồ nhị phân**
  trong `re-notes/13_db_cross_v4.md`: PE32 i386, ImageBase `0x10000000`, **chỉ 1 export**
  `_register_addon_` @RVA `0x103334` chứa **bảng 4 con trỏ** (4 adapter N-API) → 4 hàm cần đọc;
  import KERNEL32+WSOCK32+`napi_*` delay-load, **không** CryptoAPI/OpenSSL ⇒ crypto/nén tĩnh;
  **XZ/LZMA** (magic @RVA `0x10586C`, hằng số liblzma 5.3.1alpha); **AES tự viết** (S-box+invSbox
  @RVA `0x1048D8`); **MD5** cho toàn vẹn (IV lặp 3 chỗ trong `.text`); container **nhiều DB**
  (mỗi hội thoại một DB); `parseBinNet` để lộ tên trường protobuf trùng schema `Sync2.Message`.
  Bằng chứng âm quan trọng: PC **không** dùng layout/field của Android (`pwd_*`, `[ReadHeader]`) nên
  không áp được sang. Bước kế tiếp **bắt buộc cần Ghidra** (máy chưa có `dumpbin`/`objdump`):
  đọc bảng 4 con trỏ, hàm header `0x1AA00`, hàm crypto `0x25980` (KDF/mode/IV), wrapper nén
  `0x3F5F0`/`0x43350`, và xác nhận SHA-256 dùng cho check XZ hay KDF.
- **Đã sửa vòng 3 (Đợt 2 + Đợt 3 mở rộng):**
  - **20 công tắc Cài đặt chưa có tác dụng** nay hiện `disabled` + chip "(sắp có)" + tooltip
    (helper `soon_toggle`/`soon_switch`/`soon_choice`/`soon_radio` trong `gui/modals.rs`), giữ đủ mục
    để thấy lộ trình nhưng không giả vờ hoạt động.
  - **E2EE:** `chat::e2ee_placeholder_needed` (chỉ theo `e2eeStatus`/`e2ee` tường minh) +
    bong bóng giữ chỗ; không giải mã, không gọi mạng; test chống hồi quy cho tin thường.
  - **Phần B B3.2 phía mã:** `call::extract_transfer_events` (4 act, `BackupInfo` chỉ giữ cấu trúc +
    độ dài, không giữ token/`encrypted_key`/`temp_key`); worker log cấu trúc vào `voip.log` khi `--dev`.
    Đây là bộ đọc sẵn sàng để **thu bằng chứng `act_type`** khi người dùng chạy bản dev có trace.
  - **SQLite:** `MessageStore::compact`/`compact_if_needed` (WAL checkpoint + VACUUM, không xoá tin),
    tự chạy lúc khởi động khi DB > 32 MiB.
  - Test: **265 pass / 1 ignored**; `cargo check --all-targets` 0 warning; MSRV 1.88 sạch;
    `cargo audit` 0 lỗ hổng (2 unmaintained).
- **Tồn đọng còn lại (ưu tiên giảm dần):**
  1. Chưa ACK hàng đợi (`DELETE_QUEUE_ACK_*`) — cần bằng chứng thật, ACK sai mất tin.
  2. Chưa có retention cho `messages`/`raw_json` (chỉ nén, không xoá — đúng ý người dùng).
  3. `delete_message_for_me`/`recall_message` chưa chặn `msg.id` `out_*`/`local_*` (GUI đang chặn).
  4. `threads_cache.json` vẫn ghi toàn bộ lịch sử mỗi vòng.
  5. TUI chưa có cuộc gọi/reaction realtime; nav "Khác"/"Trò chuyện ẩn"/zCloud vẫn là "sắp có".
  6. **Phần B:** đã có bộ đọc sự kiện transfer + bản đồ nhị phân `db-cross-v4`, nhưng chưa có
     `pull_mobile_msg`/RSA/tải/giải mã; còn thiếu Ghidra để chốt header/KDF/cipher.
- **Việc tiếp theo:** (a) chạy bản `--dev` với trace để thu `act_type` thật của 4 sự kiện transfer
  (đây là dữ liệu duy nhất còn thiếu cho B6.2); (b) cài Ghidra, đọc 4 con trỏ N-API ở `0x103334`,
  hàm header `0x1AA00`, crypto `0x25980`, wrapper nén `0x3F5F0`/`0x43350`; (c) sau đó mới viết
  `sync_v1` bằng Rust. **Không** gọi `pull_mobile_msg` khi chưa hỏi người dùng (hành động thật trên
  tài khoản, có nguy cơ khoá bảo mật).
- **Bài học phiên này (bổ sung):** khi UI có 28 công tắc mà chỉ 8 cái có tác dụng, cách rẻ nhất để
  trung thực là disable + nhãn "(sắp có)" thay vì xoá — vẫn giữ lộ trình và không phá cài đặt đã lưu.
  Với E2EE, chỉ nhận diện theo trường tường minh; mọi suy đoán từ nội dung/độ dài đều có nguy cơ
  biến tin thường thành placeholder.
- **Đã sửa vòng 4 (Đợt 2 nốt + B3.3 thực chất):**
  - **TUI reaction realtime:** poll thêm `lp_control` (chặn 4 s/lượt) dùng con trỏ trong
    `sync_state_<uin>.json`; áp cảm xúc qua `reaction::apply_to_threads`. TUI không mở WebSocket
    nên cảm xúc chậm hơn GUI một nhịp poll.
  - **Chặn id `out_*`/`local_*`** khi xoá/thu hồi (thông điệp tiếng Việt rõ) thay vì gửi id vô nghĩa.
  - **`threads_cache.json` chỉ ghi khi nội dung đổi** (hash trong process + kiểm file tồn tại) ⇒ hết
    write amplification mỗi vòng.
  - **B3.3 có bước ngoặt:** tìm thấy `dumpbin.exe` của VS Build Tools (không cần Ghidra) ⇒
    `re-notes/14` (159 dòng, 541.381 lệnh được parse): **chốt 4 hàm thật** (`getVersion=0x119A`,
    `parseBinNet=0x1C35`, `decompressAndDecryptDb=0x6299`, `_V2=0x665E`; **sửa** kết luận sai của
    `re-notes/13` về bảng con trỏ `0x103334`), **magic header `"ZDB4.0"`** + đọc **big-endian** + băm
    luồng 64 KiB, **AES-256 + PKCS#7** (S-box chuẩn, `[I]` CBC), **liblzma tĩnh có XZ stream header**
    (⇒ XZ, không phải raw LZMA2), SHA-256 thuộc liblzma (không phải KDF).
    Còn **[U]**: KDF (`0x1C0C0` **không** gọi MD5/SHA, nghi password zero-pad), layout byte của header,
    và thứ tự nén↔mã hoá (hàm giải mã `0x29680` không có call site trực tiếp).
  - Test: **268 pass / 1 ignored**; `cargo check --all-targets` 0 warning; MSRV 1.88 sạch;
    `cargo audit` 0 lỗ hổng (2 unmaintained).
- **Tồn đọng còn lại (ưu tiên giảm dần):**
  1. Chưa ACK hàng đợi (`DELETE_QUEUE_ACK_*`) — cần bằng chứng thật, ACK sai mất tin.
  2. Chưa có retention cho `messages`/`raw_json` (chỉ nén, không xoá — đúng ý người dùng).
  3. `delete_message_for_me`/`recall_message` vẫn fallback `cliMsgId = now` khi `ts <= 0` (nên chặn
     như `quote_target_ids`).
  4. TUI: chưa có cuộc gọi, chưa hiện "đang nhập"; nav "Khác"/"Trò chuyện ẩn"/zCloud vẫn "sắp có".
  5. **Phần B:** đã có bộ đọc sự kiện transfer + bản đồ nhị phân + disassemble; nhưng chưa có
     `pull_mobile_msg`/RSA/tải/giải mã, và **chưa chốt KDF + layout header**.
- **Việc tiếp theo:** (a) đọc trọn `0x28470` (1.109 lệnh) + `0x29680` (287 lệnh) và tìm con trỏ hàm
  gọi `0x29680` trong `.data` để chốt thứ tự nén↔mã hoá; (b) đọc `0x1AE00` từ `0x1AF30`→`0x1B8BB` để
  lấy layout header byte chính xác; (c) đọc `0x27890` xem 32 byte khoá được nạp từ đâu (giải KDF).
  Khi có đủ 3 thứ đó mới viết được `sync_v1/decrypt.rs`; **vẫn không** gọi `pull_mobile_msg` khi chưa
  được người dùng đồng ý (hành động thật trên tài khoản).
- **Bài học phiên này (bổ sung 2):** công cụ có sẵn trong toolchain đôi khi đủ mạnh — `dumpbin` của
  VS Build Tools thay được Ghidra cho việc đọc hàm theo địa chỉ. Khi dumpbin cắt cột byte ở 6 byte,
  phải suy độ dài lệnh từ **địa chỉ dòng kế tiếp**, nếu không sẽ lệch và bỏ sót lệnh (đã làm hỏng
  một vòng phân tích trước khi phát hiện).
- **Bài học phiên này:**  - Đọc lại mã nguồn bằng kiểm toán độc lập tìm ra 3 lỗi "âm thầm" mà test cũ không bắt: nạp tin cũ
    nhất, video call thành audio, `status_msg` không render. Test xanh không có nghĩa là hành vi đúng
    nếu không có test cho đúng chiều dữ liệu.
  - Nhiều agent sửa song song thì phải chia theo **file** và giữ "điểm nối" (enum, hàm API) ở một người;
    đã có 2 lần build đỏ tạm thời vì enum mới chưa phủ match — chấp nhận được nhưng phải nói rõ thứ tự.
  - `cargo` lock giữa các agent làm một lần chạy test trả exit 1 dù kết quả "ok"; phải đọc lại exit code.

## 0a. v0.1.2 (2026-11-02) — giữ lại để tham chiếu

- **Phiên này chạy trên PC Windows của người dùng**, với tài khoản Zalo thật và điện thoại thật.
- **Nhánh `claude/v0.1.2`** được fast-forward vào `main`; Release `v0.1.2` do CI build.
- **Đã làm:**
  - Phần A của `docs/dev/KE_HOACH_TIEP_THEO.md`: con trỏ hàng đợi bền theo tài khoản, kéo bù tới khi hết, banner thiếu tin, nối lịch sử Cloud/nhóm.
  - Danh sách hội thoại giống điện thoại (A9).
  - Ảnh/video/tệp không còn hiện link; cửa sổ xem ảnh và cửa sổ phát video riêng (A8, do agent làm).
  - Sửa tên nhóm hiện thành "Nhóm <số>"; sửa lỗi long-poll lặp ở queue `16`.
  - Chi tiết: `CHANGELOG.md` `[0.1.2]`.
- **Bằng chứng thật:** `re-notes/10_bang_chung_dong_bo.md`, gồm các lượt A0, A7 và ngày 02/11. Trace thô nằm ở `%USERPROFILE%\qtzalo-trace\`, **không commit**.
- **Kiểm chứng của 0.1.2:**
  - `cargo test --locked`: 191 qua, 1 bỏ qua.
  - Build MSVC trên PC sạch; `cargo +1.88 check --locked --all-targets` sạch.
  - `cargo audit`: 0 lỗ hổng, 2 cảnh báo hết bảo trì.
  - Kịch bản thật:
    - con trỏ đã lưu: không có tin mới → 0 tin, có 3 tin mới → đúng 3;
    - một ngày nhiều tin 1-1 và nhóm: kéo đủ theo trang;
    - 61/61 nhóm đúng tên;
    - người dùng thử và xác nhận ảnh, video (kể cả video cũ trên `dlmd.me`) và tệp mở trong app.
  - Ảnh `--demo` đã xem: `docs/assets/qtzalo-sync.png` (sáng), bản tối trong scratchpad, `qtzalo-media-cards.png`, `qtzalo-image-window.png`, `qtzalo-video-window.png`.
- **Chưa kiểm chứng:**
  - Banner thiếu tin với `evict` thật (chưa gặp);
  - tắt máy lâu hơn một ngày;
  - Linux/macOS;
  - tiếng video chỉ được người dùng xác nhận gián tiếp ("Chạy tốt").
- **Phát hiện quan trọng:**
  1. Phiên QTzalo nhận queue `510_1, 511_1, 515_0, 517_0, 518_0` (không phải `510_0/513_0`).
  2. Kéo về không xoá tin khỏi hàng đợi.
  3. Long-poll phải theo dõi `15/16` nếu máy chủ báo.
  4. Thiết bị mới không lấy được lịch sử nhóm trước lần đăng nhập đầu tiên.
  5. Không nhóm nào E2EE.
  6. Có sự kiện `transfer_after_login` mang `temp_key` (liên quan Phần B).
- **Việc tiếp theo:** Phần B (đồng bộ từ điện thoại, hướng Sync V1), chỉ khi người dùng đồng ý. Đọc `re-notes/09` §9 và `KE_HOACH_TIEP_THEO.md` §2 (đã thêm bước phân tích APK Android và câu hỏi B6.6–B6.7).

## 1. Trạng thái của v0.1.1 (giữ lại để tham chiếu)

- **Repo:** `quangthoai1985/QTzalo`, bản fork của `ctdoteam1110/ctdozlite` (tách từ commit `73849bd` = tag upstream `v0.0.2`).
- **`main` = commit phát hành 0.1.1** (fast-forward từ nhánh `claude/release-0.1.1`). Release [QTzalo v0.1.1](https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.1) do CI build (bộ cài + portable Windows, `.deb`, `.tar.gz`, `.dmg`, `SHA256SUMS.txt`). v0.1.0 vẫn còn trên trang Releases.
- **Đã làm trong 0.1.1** (chi tiết: `CHANGELOG.md`):
  - Tải tệp/video/ảnh về "Thư mục lưu file" — `src/download.rs` (chính sách + I/O), `WorkerCmd::DownloadAttachment`, `GuiEvent::Download{Progress,Finished,Failed}`, `DownloadState` và `downloads`/`download_toast` trong `ZaloGuiApp`; nút trên thẻ (`src/gui/messages.rs`, `BubbleAction::{Download,OpenDownloaded,RevealDownloaded}`), trình xem ảnh và toast (`src/gui/modals.rs`).
  - Quản lý cache ảnh trong *Cài đặt → Dữ liệu & làm mới* (`WorkerCmd::{ComputeCacheSize,ClearImageCache}`), dọn `clipboard_*.png` > 24 h khi khởi động (`paths::cleanup_old_clipboard_images`).
  - Ảnh dán clipboard ghi vào `paths::cache_dir()`; chọn thư mục lưu file bằng hộp chọn thư mục (`pick_native_folder_dialog`).
  - Số thành viên nhóm thật từ `totalMember` của `getmg-v2` (cache trong tiến trình `chat::group_member_count`, **không** lưu vào `Thread`/`threads_cache.json`).
  - ratatui 0.30 / crossterm 0.29 (bỏ `lru` unsound và `paste`) + vài bản patch.
- **Kiểm chứng cục bộ của 0.1.1:** `cargo test --locked` 147 pass / 1 bỏ qua; `scripts/check-windows.sh` sạch; `cargo +1.88 check --locked --all-targets` sạch; `cargo audit` 0 lỗ hổng, 2 cảnh báo hết bảo trì; ảnh giao diện `--demo` (sáng/tối, Cài đặt) đã xem — `docs/assets/qtzalo-download.png`.
- **CHƯA kiểm chứng:** chưa chạy trên Windows thật (MotW, `explorer /select`, hộp chọn thư mục PowerShell), chưa thử với tài khoản Zalo thật (tải file thật từ CDN Zalo, xoá cache thật, `totalMember` có thật trong payload hay không).
- **Sau phát hành 0.1.1:** đã nghiên cứu cơ chế đồng bộ của Zalo PC (`re-notes/09_dong_bo_tin_nhan.md`) và lập kế hoạch tiếp theo (`docs/dev/KE_HOACH_TIEP_THEO.md`). Chưa có thay đổi mã nguồn nào sau `d8ba11c`.
- **Nhật ký thay đổi và trạng thái từng lỗi:** `CHANGELOG.md`, `docs/SECURITY_AUDIT.md` (S1–S16, B1–B9), `docs/CAI_DAT.md`, `README.md`.

## 2. Quyết định đã được người dùng chốt

| Chủ đề | Quyết định |
|---|---|
| Tên sản phẩm | **QTzalo**. Cargo package/binary vẫn là `ctdozlite` / `zalo-rust`. Bộ cài Windows cài file chạy thành `QTzalo.exe`. |
| Nhánh & phát hành | **Được merge vào `main` và phát hành Release từ `main`** bằng fast-forward, không viết lại lịch sử. Người dùng đã đồng ý lại cho v0.1.1. |
| Thư mục dữ liệu | `%APPDATA%\QTzalo` / `~/.local/share/qtzalo` / `~/Library/Application Support/QTzalo` (`src/paths.rs`). Biến `QTZALO_DATA_DIR` ghi đè. |
| Giao diện | 3 cột tông sáng theo ảnh mẫu của người dùng; cột trái và cột hội thoại thu gọn được còn icon/avatar; vẫn có giao diện tối. |
| Phạm vi v0.1.1 | Đã làm và phát hành (P0 đủ, P1: số thành viên + dependency; chỉ báo đang nhập chưa làm vì thiếu bằng chứng giao thức). |
| Khi nào hỏi người dùng | Chỉ dừng lại hỏi khi cần một quyết định thật sự (ví dụ: đổi tên binary, xoá dữ liệu người dùng, thay đổi hành vi bảo mật lớn). |

## 3. Bản đồ mã nguồn

| Khu vực | File | Ghi chú |
|---|---|---|
| Khởi động, CLI | `src/main.rs` | `--gui` (mặc định), `--tui`, `--qr`, `--dev`, `--demo`, `--logout`, `--pass-stdin` |
| Đường dẫn dữ liệu | `src/paths.rs` | `data_dir()`, `session_file()`, `history_db()`, `cache_dir()`, `logs_dir()`, `settings_file()`, `write_private()` (ghi nguyên tử, 0600), `migrate_legacy_files()` |
| Tải tệp | `src/download.rs` | `download_url_allowed`, `sanitize_filename`, `pick_unique_path`, `download_to_dir` (stream → `.part` → hard link không ghi đè), `write_mark_of_the_web`, `open_downloaded` (không mở file thực thi), `dir_size`, `clear_dir_contents` |
| Định dạng giờ | `src/timefmt.rs` | `hm_from_ms`, `local_day_index`, `local_weekday`… dùng `chrono::Local` |
| Phiên & HTTP | `src/session.rs` | `SessionStatus {Valid, Expired, Unreachable}`, `check_session()`, domain đã kiểm chứng thắng `zpw_service_map_v3`, chỉ gửi cookie tới host https của Zalo |
| Đăng nhập | `src/login.rs`, `src/qr.rs`, `src/param_cipher.rs`, `src/crypto.rs` | ParamCipher v2, sinh số ngẫu nhiên bằng `getrandom` |
| Tin nhắn / API | `src/chat.rs` (lớn, khoảng 4,6k dòng) | `get_friends`, `get_groups` (`/api/group/getmg-v2`), `get_group_history`, upload ảnh/tệp, `parse_link_metadata(s, url, allow_direct_fetch)` |
| WebSocket | `src/socket.rs` | chỉ dùng `wss://` của Zalo; giải nén giới hạn 32 MiB; `spawn_with_cursors` kéo queue từ con trỏ đã lưu (`WsCursors`) |
| Đồng bộ (0.1.2) | `src/sync_state.rs` | `SyncState` (`sync_state_<uin>.json`), `merge_ws_queue_status`, `gap_signals`/`record_gaps_from` (bảng `missing_ranges`), `MsgIdClock` + `load_conversation_activity` (bảng `thread_activity`) |
| Reaction (0.1.3) | `src/reaction.rs` | `MsgReaction` (một lần bấm; `emoji` rỗng = mốc gỡ), `extract_events` (`612/610/611/lp_control`, `gMsgID` hoặc `cliMsgId`), `apply`, `add_mine`/`remove_mine`, `summary`, `apply_to_threads`; gửi: `chat::reaction_content` + `send_reaction`; giao diện: `gui/messages.rs` (`hover_reaction_controls`) |
| Trace (0.1.2) | `src/trace_shape.rs` | `ZCALL_WS_TRACE=1`: in cấu trúc payload, không in nội dung |
| Video (0.1.2) | `src/video.rs`, `src/gui/media_view.rs` | tìm `ffmpeg.exe`, tham số đã khoá chặt, cửa sổ phát video; `WorkerCmd::PrepareVideo` tải vào `stickers_cache/videos/` |
| Cuộc gọi | `src/call.rs`, `src/media.rs`, `src/voip_log.rs`, `src/audio.rs` | log chỉ ghi khi `--dev` / `ZALO_CALL_TRACE=1` / `ZALO_VOIP_LOG` |
| Lưu lịch sử | `src/store.rs` | SQLite với truy vấn tham số hoá |
| GUI — vòng lặp & sự kiện | `src/gui.rs` | `enum GuiEvent`, `AppSettings` (gồm `download_dir`), `ZaloGuiApp`, `is_demo_mode()` |
| GUI — worker nền | `src/gui/worker.rs` | `enum WorkerCmd`; mọi việc mạng chạy ở đây (tokio) |
| GUI — bố cục | `src/gui/sidebar.rs` (thanh điều hướng + danh sách hội thoại), `src/gui/workspace.rs` (khung chat, composer), `src/gui/messages.rs` (bong bóng, thẻ link/ảnh/tệp, mở URL, file picker, clipboard), `src/gui/modals.rs` (Cài đặt 7 tab, trình xem ảnh, cuộc gọi…), `src/gui/stickers.rs`, `src/gui/theme.rs` (bảng màu, dò font), `src/gui/demo.rs` (dữ liệu mẫu offline) | |
| TUI | `src/tui.rs`, `src/ui.rs` | |
| Công cụ dev | `src/bin/*`, `scripts/` | `call_probe`, `test_fetch`, …; `scripts/shot-demo.ps1` (chụp ảnh `--demo` trên Windows bằng PrintWindow); `scripts/re/asar_map.py`, `asar_snip.py` (đọc `app.asar` của Zalo PC, chỉ đọc, chạy bằng `python -I`) |

## 4. Build, test, kiểm chứng

```bash
# Rust >= 1.88 (1.87 bị cargo từ chối vì cookie_store/darling/image/time). Stable mới nhất đều chạy được.
export CMAKE_POLICY_VERSION_MINIMUM=3.5   # cần khi CMake >= 4 (audiopus_sys)
cargo build --locked --bin zalo-rust
cargo test --locked                        # phải xanh
bash scripts/check-windows.sh              # cargo check cho x86_64-pc-windows-gnu (cài target + mingw nếu thiếu)
cargo audit                                # đã cài sẵn trong môi trường cloud trước; nếu thiếu: cargo install cargo-audit --locked
```

- **Thư viện hệ thống cần trên Linux:** `build-essential cmake pkg-config libasound2-dev libxkbcommon-dev libxcb-shape0-dev libxcb-xfixes0-dev`. Để chụp ảnh giao diện cần thêm `libgl1-mesa-dri libegl1 libxkbcommon-x11-0 libxcursor1 libxrandr2 libxi6 fonts-noto-core fonts-noto-cjk xvfb imagemagick`.
- **Chụp ảnh giao diện không cần tài khoản thật** (chế độ demo không mở mạng và không ghi dữ liệu):

  ```bash
  HOME=$(mktemp -d) xvfb-run -a -s "-screen 0 1280x800x24" sh -c \
    "./target/debug/zalo-rust --gui --demo & sleep 12; import -window root /tmp/shot.png; kill %1"
  ```

  Các biến `QTZALO_DEMO_*` trong `src/gui/demo.rs` / `src/gui.rs` chỉnh kích thước, theme, trạng thái thu gọn và hội thoại được chọn. Xem ảnh bằng công cụ đọc ảnh trước khi kết luận giao diện đã đúng.

## 5. Việc tiếp theo — xem `docs/dev/KE_HOACH_TIEP_THEO.md`

- v0.1.2 (tự bắt kịp tin khi mở app) và v0.1.3 (thả cảm xúc, reaction theo thời gian thực) đã phát hành.
- Việc tiếp theo là **Phần B: "Đồng bộ từ điện thoại" theo hướng Sync V1**. B3.1 và B3.2 đã xong (`re-notes/11`); bước kế tiếp là **B3.3**, phân tích định dạng file sao lưu mà module native `db-cross-v4` (chỉ có bản ia32) giải mã.
- Phiên tiếp theo chạy **trên PC Windows** của người dùng, có tài khoản Zalo thật.

- Kế hoạch chi tiết, tiêu chí nghiệm thu, các bước nghiên cứu Sync V1 và cách dựng môi trường PC: `docs/dev/KE_HOACH_TIEP_THEO.md`.
- Prompt dán vào phiên mới: `docs/dev/PROMPT_BAN_GIAO.md`.
- Cơ sở nghiên cứu: `re-notes/09_dong_bo_tin_nhan.md` (bundle Zalo PC trong lịch sử git + tài liệu phân tích Zalo PC 26.10.20 do người dùng cung cấp, tài liệu này không có trong repo).

## 6. Quy trình phát hành (đã chạy thành công cho v0.1.0 → v0.1.3)

> Trên PC của người dùng, `gh` đã đăng nhập: v0.1.3 phát hành bằng `git push origin main`, `gh workflow run ci-release.yml --ref main -f release=true`, chờ bằng `gh run watch <id> --exit-status` (chạy nền), kiểm tra bằng `gh release view v<x.y.z>`. Các bước GitHub MCP bên dưới dùng cho môi trường cloud.

1. Sửa `Cargo.toml` thành `version = "<phiên bản mới>"`, chạy `cargo build` để `Cargo.lock` cập nhật dòng `ctdozlite`.
2. `CHANGELOG.md`: thêm mục `## [x.y.z] - <ngày>` (Thêm mới / Sửa lỗi / Bảo mật) và link ở cuối file. Cập nhật `docs/SECURITY_AUDIT.md` nếu có thay đổi bảo mật.
3. `.github/release-notes.md`: cập nhật mục "Điểm mới" cho phiên bản mới. Template có sẵn `{{VERSION}}` / `{{COMMIT}}`.
4. Commit theo Conventional Commits, kết thúc bằng hai dòng attribution mà hệ thống cung cấp cho phiên. Push nhánh làm việc; CI chạy cả bản Windows trên mọi push vào `claude/**`.
5. Khi CI xanh: fast-forward `main` (`git push origin HEAD:main`; trước đó kiểm tra `git merge-base --is-ancestor origin/main HEAD`).
6. Kích hoạt phát hành bằng GitHub MCP:
   - `actions_run_trigger` với `method=run_workflow`, `workflow_id=ci-release.yml`, `ref=main`, `inputs={"release": true}`.
   - Job `release` tự tạo tag `v<version>` tại commit đã build và chỉ chạy khi `test` + `windows` xanh.
   - Release đã tồn tại thì job dừng có chủ ý, nên mỗi lần phát hành phải tăng phiên bản.
7. Kiểm tra bằng `get_release_by_tag v<phiên bản>` rằng release có đủ file:
   - `QTzalo-0.1.1-setup-windows-x86_64.exe`
   - `QTzalo-0.1.1-portable-windows-x86_64.exe`
   - `qtzalo_0.1.1_amd64.deb`
   - `qtzalo-0.1.1-linux-x86_64.tar.gz`
   - `QTzalo-0.1.1-macos-arm64.dmg` (macOS là best-effort)
   - `SHA256SUMS.txt`

## 7. Bài học khi điều phối nhiều agent

- **Tạo trước "điểm nối" dùng chung rồi mới giao việc.** Ví dụ ở v0.1.0: `paths.rs`, `timefmt.rs`. Với v0.1.1, coordinator nên tự thêm các biến thể `WorkerCmd`/`GuiEvent` cho tải file cùng một module thuần (ví dụ `src/download.rs` chứa làm sạch tên file, đặt tên tránh trùng, chính sách URL, có test), commit và push, rồi mới giao. Khi đó agent GUI và agent worker không sửa chồng lên nhau.
- **Phân quyền file rõ ràng cho từng agent.** Ghi rõ được sửa file nào và cấm sửa file nào. Agent cần thứ ngoài quyền thì ghi vào báo cáo.
- **Worktree có thể được tạo từ `main` cũ, không phải commit nền vừa push.** Sau khi giao việc, chạy `git worktree list`; nếu sai, nhắn agent `git stash && git merge --ff-only <commit nền> && git stash pop` (hoặc `git rebase <commit nền>` nếu đã commit).
- **Đĩa có hạn (khoảng 25 GB trống lúc bắt đầu).** Mỗi thư mục `target` debug khoảng 3,5 GB. Không để mỗi agent copy riêng một thư mục `target`: dùng chung, dùng `cargo check` nhiều hơn `cargo build`, và xoá worktree đã merge (`git worktree remove --force --force …`). Nếu đầy đĩa, xoá `target/*/incremental` và `target/x86_64-pc-windows-gnu`.
- **Agent có thể dừng vì chạm giới hạn sử dụng.** Phần việc vẫn được lưu; khi giới hạn được đặt lại, gửi `SendMessage` để agent làm tiếp, không tạo agent mới.
- **Chế độ tự động chặn việc chạy code bên ngoài chưa review** (ví dụ chạy test trên nhánh PR của upstream). Khi đó đọc diff tĩnh rồi tự viết lại.
- **Hook dừng phiên báo file untracked `.claude/`** (thư mục worktree). Thêm `.claude/` vào `.git/info/exclude`; không commit thư mục này.
- **Không dùng `gh` CLI cho việc khác ngoài tìm kiếm.** Mọi thao tác GitHub (Actions, Release, PR) dùng GitHub MCP. CI chạy được `gh` trên runner, điều này không bị ảnh hưởng.
- **Không thể ngủ chờ (sleep/poll).** Để chờ CI, dùng `send_later` hẹn tự kiểm tra lại sau khoảng 10–12 phút. Run Windows mất khoảng 9–10 phút.
- **Không ghi model identifier** vào commit, PR hay mã nguồn.
- **(0.1.1) Worktree của agent được tạo từ `73849bd` (commit upstream cũ), không phải `main`.** Cách nhắn agent `git merge --ff-only <commit nền>` vẫn hiệu quả; ghi luôn lệnh này vào đầu prompt giao việc.
- **(0.1.1) Dùng chung `CARGO_TARGET_DIR` giữa các worktree tiết kiệm đĩa nhưng có thể khiến binary là bản build của agent khác** (cùng metadata package). Agent phải `touch` file của mình trước khi build để chụp ảnh và copy binary ngay sau khi build. Đặt tên file chụp/script riêng cho từng agent (scratchpad dùng chung).
- **(0.1.2, PC Windows)** Một số lưu ý khi làm trên máy người dùng:
  - Nút X của QTzalo chỉ **thu nhỏ** cửa sổ; muốn thoát hẳn phải bấm Ctrl+Q. Khi đo "tắt máy", phải kiểm tra tiến trình đã thoát thật.
  - Để không phải nhờ người dùng thao tác, chạy app có hẹn giờ: `timeout 180 ./bin/zalo-rust-xxx.exe --gui --dev` trong Git Bash.
  - Chạy **bản sao** của file exe ở `%USERPROFILE%\qtzalo-trace\bin\`, để `target` vẫn build tiếp được trong lúc app đang chạy.
  - Không chạy hai bản QTzalo cùng lúc với cùng một tài khoản: cả hai cùng ghi `sync_state` và DB.
- **(0.1.2)** `Get-Content`/`Set-Content` của PowerShell 5.1 đọc/ghi theo ANSI và thêm BOM, đã làm hỏng `Cargo.toml` một lần. Sửa file bằng công cụ Edit, không dùng `-replace` + `Set-Content`.
- **(0.1.2)** Git trên PC chưa cấu hình tên/email: commit bằng `git -c user.name=Claude -c user.email=noreply@anthropic.com commit …`. Message nhiều dòng thì truyền qua file (`-F`).
- **(0.1.2)** Scratchpad dùng chung với agent: agent đã ghi đè script chụp ảnh của coordinator. Đặt tiền tố riêng cho từng agent (`a8_…`).
- **(0.1.2)** Bỏ một giới hạn an toàn có thể làm lộ ra lỗi có sẵn. Bỏ giới hạn 3 đợt long-poll đã biến lỗi "không theo dõi queue 16" thành khoảng 10 yêu cầu/giây. Luôn có điều kiện "con trỏ phải tiến" khi lặp, và đếm số yêu cầu trong trace sau mỗi thay đổi vòng lặp.
- **(0.1.2)** Agent bị dừng khi tiến trình cha thoát vẫn giữ nguyên worktree và transcript: `SendMessage` cho nó làm tiếp được, kể cả gộp nhánh coordinator vào.
- **(0.1.3) Unit test dựng theo cấu trúc trace không bắt được lỗi phụ thuộc giá trị.** Hai lỗi thật (`cliMsgId` của tin của chính mình, cách đếm cộng dồn) chỉ lộ ra khi người dùng thử. Với tính năng tương tác hai chiều, nên sớm chạy một lượt có dòng chẩn đoán tạm (chỉ in id và mã, không in nội dung), và thử cả tin của mình lẫn tin của người khác, cả 1-1 lẫn Cloud.
- **(0.1.3) egui 0.29:** menu chuột phải **không** nằm trong `memory.popup`, nên `any_popup_open()` không bắt được; phải kiểm `egui::menu::BarState::load(ctx, Id::new("__egui::context_menu"))`. Panel nằm trên lớp nền được đăng ký phủ cả màn hình, nên `ctx.layer_id_at(p)` dùng được để biết con trỏ có đang ở trên một popup không.
- **(0.1.3)** Trên PC, `gh` dùng được cho Actions và Release. Chờ CI bằng một lệnh `gh run watch` chạy nền, không cần hẹn giờ kiểm tra lại.
- **(0.1.3)** Chụp ảnh giao diện trên Windows: `scripts/shot-demo.ps1` (PrintWindow cờ 2 chụp được cửa sổ glow kể cả khi bị che). Màn hình 150 % nên ảnh ra 1920×1200 với `QTZALO_DEMO_SIZE=1280x800`.
- **(0.1.3)** Sắp chạm giới hạn sử dụng thì dừng agent đang chạy nền (`TaskStop`), nếu không nó tiêu hết phần còn lại mà vẫn có thể không kịp báo cáo.
- **(0.1.1) Mẫu chia việc hiệu quả:** coordinator tự viết toàn bộ backend + biến thể enum + xử lý sự kiện trong `gui.rs`, agent chỉ làm UI theo vùng file (messages/modals-viewer vs modals-settings vs chat/deps). Chỉ có 1 xung đột nhỏ ở khối biến demo trong `src/gui.rs`.

## 8. Tồn đọng đã biết

- **Chưa thử trên Windows thật và tài khoản Zalo thật** cho các tính năng 0.1.1 (và các sửa lỗi lõi #21/#22 của 0.1.0).
- **Chỉ báo đang nhập** chỉ có ở demo. Từ 0.1.2 đã có bằng chứng giao thức (socket `602`, `lp_control` `act = "typing"`), chưa làm.
- ~~Không tự bắt kịp tin offline~~ → đã làm trong 0.1.2. Còn lại: TUI/CLI chưa dùng con trỏ bền; chưa ACK hàng đợi; lịch sử trước lần đăng nhập đầu tiên chỉ lấy được qua "đồng bộ từ điện thoại" (Phần B). Danh sách đầy đủ ở `KE_HOACH_TIEP_THEO.md` §4.
- **Số thành viên nhóm** dựa trên `totalMember` suy ra từ entity `Group` của web bundle cũ (đã xoá ở `54314e0`); chưa thấy trong payload thật. Thiếu thì header vẫn hiện "Nhóm".
- **S6:** media cuộc gọi chưa lọc nguồn gói UDP; client khai `srtpMode: 0`.
- **#19:** chưa đồng bộ toàn bộ lịch sử.
- **Cảnh báo `cargo audit`:** `audiopus_sys`, `ttf-parser` hết bảo trì (không có lỗ hổng).
- **Tải về:** chưa có nút huỷ; macOS chưa gắn quarantine; dark theme nút đỏ (`pal.danger`) chữ trắng tương phản hơi thấp.
- **Emoji màu:** egui không vẽ được emoji màu.
- **Bộ cài chưa ký số**, nên SmartScreen cảnh báo.
- **(0.1.3) Reaction:**
  - Tin cũ do chính mình gửi trong hội thoại 1-1 (lưu trước 0.1.3) chưa có `cliMsgId`, nên reaction từ điện thoại lên những tin đó không khớp. QTzalo không tải lại lịch sử 1-1 để bổ sung trường này.
  - Reaction cho tin nằm ngoài 200 tin gần nhất trong bộ nhớ của mỗi hội thoại bị bỏ qua.
  - Gửi lỗi thì không hoàn tác trên máy.
  - TUI không nhận reaction theo thời gian thực.
  - Mã cảm xúc lạ (ngoài 6 mã) hiện thành ❤️.
- **(0.1.3) Các điểm nhỏ còn để lại sau review:**
  - phần xử lý rê chuột chạy cho mọi bong bóng đang tải ở mỗi khung hình, và tính phần tóm tắt reaction ba lần;
  - tin đã thu hồi vẫn có nút cảm xúc;
  - thiếu test cho `content` dạng object, `gMsgID` dạng chuỗi, `uidFrom` dạng số;
  - quay lại 0.1.2 thì cache hội thoại bị bỏ (đã ghi trong release notes).
- **(0.1.3)** `msgType` trong `rMsg` mới ánh xạ ảnh = 32 (đã quan sát); nhãn dán vẫn gửi 5, tệp/video/voice gửi 1 (theo zca-js có lẽ là 36/46/44/31, chưa kiểm chứng).
