# Báo cáo kiểm toán QTzalo — tiến độ và việc còn tồn đọng (03/11/2026)

> Ngày: 03/11/2026 · Phạm vi: toàn bộ repo `QTzalo/QTzalo` + tài liệu bàn giao + kho nghiên cứu ngoài repo.
> Mục đích: trả lời "dự án đã đi tới đâu, còn những gì tồn đọng".

## 1. Phương pháp và độ phủ

| Việc | Cách làm | Kết quả |
|---|---|---|
| Đọc mã nguồn | 5 nhóm đọc song song, **đọc trọn** 41 file `.rs` (~45.000 dòng, ~2,0 MB) + `Cargo.toml`, `build.rs` | 100% file trong `src/` được đọc, không đọc lướt |
| Đọc tài liệu | `docs/dev/BAN_GIAO.md`, `docs/dev/KE_HOACH_TIEP_THEO.md`, `README`, `CHANGELOG`, `re-notes/03`, `re-notes/11`, `docs/SECURITY_AUDIT.md`, `KIEN_TRUC_HE_THONG`, `CI`, `PROMPT_BAN_GIAO` | Xong |
| Kiểm chứng thực tế | `cargo test --locked`, `cargo check --locked --all-targets`, `cargo +1.88 check`, `cargo audit` | Xong (xem §2) |
| Kiểm chứng chéo phát hiện nặng | Tự đọc lại `store.rs`, `session.rs`, `worker.rs`, `main.rs`; grep toàn repo cho Sync V1 / ACK / transfer / cancel / quarantine | Đã xác nhận |
| Kho nghiên cứu ngoài repo | `docs/ZaloPC-*.md` (175 KB), `notes/00…43`, `apk/`, `artifacts/`, `tools/`; đọc byte tĩnh `db-cross-v4-native.node` | Xong, có phát hiện mới |

Lưu ý: repo **sạch**, `main` = `9fb414d`, không có worktree, không có nhánh chưa merge, `Cargo.toml` = `0.1.3`.

## 2. Tình trạng kỹ thuật hiện tại (kiểm chứng lại, không tin tài liệu)

| Hạng mục | Kết quả thật |
|---|---|
| `cargo test --locked` | **PASS** — 225 pass, 0 fail, **1 ignored** (`call::tests::live_call_dtc_trace`, cần cuộc gọi thật) |
| Số test trong mã | 227 attribute (`#[test]` 221 + `#[tokio::test]` 6); trên Windows biên dịch 226 vì `paths.rs:362` nằm trong `#[cfg(unix)]` |
| `cargo check --locked --all-targets` | **PASS, 0 warning** |
| `cargo +1.88 check --locked --all-targets` | **PASS** (MSRV `rust-version = "1.88"` đúng) |
| `cargo audit` | **0 lỗ hổng**; 2 cảnh báo *unmaintained*: `audiopus_sys 0.2.2`, `ttf-parser 0.25.1` |
| Phát hành | v0.1.0 → v0.1.3 đều đã release; v0.1.3 có 5 file (**thiếu `.dmg`**), workflow báo `failure` vì job macOS bị huỷ |
| Nợ kỹ thuật cấu trúc | `src/main.rs:7-31` khai lại toàn bộ 24 `mod` thay vì dùng `src/lib.rs` ⇒ mọi test chạy **2 lần** (lib + bin); `main.rs:5` có `#![allow(dead_code)]` che cảnh báo code chưa nối |

Kết luận: **trạng thái build/test tốt, trung thực với tài liệu**. Các con số trong `BAN_GIAO.md` §0 và `SECURITY_AUDIT.md` đều kiểm chứng được.

## 3. Tiến độ theo vùng

| Vùng | Hoàn thiện | Đánh giá |
|---|---|---|
| Giao thức: ParamCipher v2, crypto, login QR/mật khẩu, session bền | **Rất cao** | `chat.rs` 5.767 dòng / 39 test, `session.rs` 12 test, `param_cipher.rs` 8 test có cả blob thật. Mọi hằng số có dẫn chứng `[O]` |
| WebSocket + long-poll + con trỏ bền (A1) | **Cao** | `socket.rs` 7 test (có test chống bom nén 32 MiB), `sync_state.rs` 10 test, ghi nguyên tử 0600, chống traversal |
| Bắt kịp tin / banner thiếu tin / sắp xếp hội thoại (A2–A4, A9) | **Cao** | Đã nghiệm thu tài khoản thật ở v0.1.2 |
| Reaction hover + realtime (v0.1.3) | **Cao** | `reaction.rs` 22 test + 6 test chống nhấp nháy ở `messages.rs`; hai chiều 1-1/Cloud đã nghiệm thu thật |
| GUI egui (3 cột, 7 tab Cài đặt, sticker, tìm kiếm, viewer ảnh, player video) | **Cao về bề mặt** | 19.685 dòng nhưng chỉ 22 test; **0 test** cho `gui.rs`, `modals.rs`, `workspace.rs`, `sidebar.rs`, `stickers.rs` |
| Tải tệp / cache / MotW | **Cao về chính sách** | `download.rs` 17 test, chống traversal/bidi/tên thiết bị Windows, không ghi đè, không mở file thực thi |
| Cuộc gọi | **Thoại 1-1 thật; video "thật một nửa"** | Signaling + Opus/RTP/RTCP/H.264 đã chạy; còn 3 lỗ hổng nặng (§4.2 P1) |
| TUI | **Trung bình** | Không cuộc gọi, không WebSocket, không reaction realtime, không dùng con trỏ bền |
| **Phần B — Đồng bộ từ điện thoại (Sync V1)** | **0 dòng mã** | Chỉ có tài liệu/nghiên cứu. Đây là khoảng trống lớn nhất |

## 4. Việc còn tồn đọng

### 4.1 Theo kế hoạch Phần B (Sync V1) — chưa có mã nào

Không tồn tại (`grep` 0 kết quả): `pull_mobile_msg`, `from_seq_id`, `cancel_pull_mobile_msg`, `get_backupmsginfo`, `get_crossdb`, `syncmsg_info`, `user_confirm`, `transfer_error`, `transfer_after_login`, `DELETE_QUEUE_ACK`, `RESTORE_*`, `db-cross`.

| Bước | Trạng thái | Ghi chú |
|---|---|---|
| B3.1 bản đồ asar | **Xong** | `re-notes/11` |
| B3.2 kênh `BACKUP_SUCCESS` | **Xong tài liệu**, `act_type` còn `[U]` | Cần bộ lọc `act` cạnh `call::extract_voip_controls` (`call.rs:933` đang lọc cứng `act_type != "voip"`) |
| **B3.3 định dạng `db-cross-v4`** | **Chưa bắt đầu, nhưng khoảng trống đã hẹp hơn tài liệu** | Xem dưới |
| B3.4 schema DB điện thoại | **Gần đủ** | Đã có DDL đầy đủ `ChatContent`, `chats`, `threads` từ chuỗi `libzalosqlite.so` |
| B3.5 phía APK | **Một phần, lệch mục tiêu** | Thiếu đúng phần quyết định: `xe1.g`/`ve1.g` (transfer v1) và thân native `decodeFile`/`convertAndroidDbtoCross` |
| B4 probe thật | **Chưa** | Chưa có `src/bin/sync_v1_probe.rs` |
| B5 tích hợp | **Chưa** | Phụ thuộc B3.3 + B4 |
| B6.1/B6.5/B6.7 | **Chưa có bằng chứng** | Endpoint có nhận client lạ? rủi ro khoá tài khoản? snapshot có tin nhóm/lịch sử cũ? |

**Phát hiện mới về B3.3** (đọc byte tĩnh `db-cross-v4-native.node`, 1.239.552 B, SHA-256 giống hệt cả 3 bản Zalo 26.9.10 / 26.10.11 / 26.10.20):

- Có **magic XZ** `FD 37 7A 58 5A 00` tại offset 1.066.604 ⇒ nén là **XZ/LZMA**, không phải zstd/lz4/zlib. Phía Android có đối chứng: `Decompress LZMA fail`, `Init lzma encoder fail`.
- Header có `[ReadHeader]` đọc `file name`, `name length`, `file size`; có `MD5 canculate of compress file` / `MD5 read from encrypted file` / `Md5 not correct !` ⇒ **header chứa MD5 của phần nén**.
- Có `Password can not empty. FAIL`, `Incorrect key length` ⇒ khoá là **dẫn xuất cố định từ password** (độ dài cố định 16/24/32), không dùng thẳng.
- Module làm **cả hai chiều** (compress+encrypt và decompress+decrypt), xử lý theo stream, có buffer tuần tự hoá `[độ dài][dữ liệu]`, **không** chứa SQLite (`không có "SQLite format 3"`) ⇒ nó chỉ ghi ra file `.db` nguyên vẹn; lớp mở SQLCipher (`password: uin`) là tầng khác.
- Nguồn: `D:\Workspace\nativelibs\db-cross-v4\src\main.cpp` — **một translation unit duy nhất**, MSVC; chỉ import `KERNEL32.dll`, `libnode.dll`, `WSOCK32.dll` ⇒ crypto/nén liên kết tĩnh, đã strip symbol (chỉ 2.921 chuỗi ASCII) ⇒ `strings` không đủ, phải disassemble.
- Chỉ có bản `electron_x86` (ia32) ⇒ muốn chạy "hộp đen" cần Electron/Node ia32 cùng ABI.

Còn thiếu chính xác 5 thứ để viết lại bằng Rust: (1) layout byte của header; (2) KDF `password → AES key`; (3) cipher & mode (AES-CBC? IV ở đâu? padding?); (4) đơn vị nén (mỗi hội thoại một XZ stream? format 1 có `onProgress(db)` ⇒ nhiều DB); (5) xác nhận lớp trong đúng là SQLCipher chuẩn.

**Rủi ro tư liệu:** `apk/jadx-src/`, `apk/lib-arm64/`, `apk/resources-dump.txt`, `notes/tmp-crypto/`, `artifacts/asar/root/*.js` **không còn trên đĩa** dù `notes/40–43` còn trích dẫn `file:dòng`. Bản gốc vẫn còn (app.asar 177 MB, `.node` 1,24 MB, APKM 120 MB) nên tái lập được, nhưng hiện **không thể kiểm chứng lại** phần lớn bằng chứng B3.5. Cần giải nén lại **ra ngoài repo** trước khi làm tiếp.

### 4.2 Lỗi / khoảng trống trong mã — đã kiểm chứng

**P0 — sai chức năng, nên sửa trước khi làm tính năng mới**

| # | Vị trí | Vấn đề |
|---|---|---|
| 1 | `store.rs:257-264` | `messages_for` dùng `ORDER BY ts ASC LIMIT` ⇒ hội thoại > 200 tin chỉ nạp **200 tin cũ nhất** từ SQLite; sau khi khởi động lại, lịch sử hiển thị sai (thiếu tin mới). Đã tự đọc xác nhận |
| 2 | `session.rs:326` + `worker.rs:2111` | Tải tệp/video dùng client của `Session` có `timeout(35s)` **cho cả body** ⇒ tệp lớn/đường chậm gần như chắc lỗi. Đã tự đọc xác nhận |
| 3 | `main.rs:256`, `tui.rs:205` | TUI dùng `PollState::default()`, **không** đọc/ghi `sync_state_<uin>.json` ⇒ mỗi lần mở TUI tải lại từ `lastActionId = 1`; TUI không có banner thiếu tin, không reaction realtime |
| 4 | `gui/worker.rs:2295` | Nhánh đăng nhập mật khẩu reset `PollState::default()` và **không** nạp lại `SyncState` (nhánh QR ở `:853` thì có) |
| 5 | `call.rs:467` vs `call.rs:1975` | Caller gửi khoá `typeRequest` nhưng nhánh legacy đọc `type` và mặc định `1` ⇒ **video call phát từ GUI có thể bị đăng ký thành gọi thoại** |
| 6 | `gui.rs:427` (+ mọi nơi gán `status_msg`) | `status_msg` **không được render ở đâu** ⇒ mọi `StatusUpdate` (lỗi gửi, làm mới thất bại, mất mạng, thu hồi lỗi) vô hình với người dùng |
| 7 | `reaction.rs:233-249` + `workspace.rs:1448-1487` | Sự kiện cảm xúc cho tin ngoài 200 tin đã nạp bị **bỏ im lặng**; `add_mine` chạy trước khi gửi và **không rollback** khi gửi lỗi ⇒ cảm xúc "ma" `ts=0` |
| 8 | `call.rs:2690`, `:2424`, `:2264` | `eprintln!` in **payload signaling đã giải mã** (sessId, uid, IP) vô điều kiện, không phụ thuộc `--dev` |

**P1 — hỏng một phần / rủi ro cao**

| # | Vị trí | Vấn đề |
|---|---|---|
| 9 | `download.rs` toàn module | **Không có huỷ tải**: không `CancelDownload`, không cancel token. `messages.rs:2767-2823` chỉ ẩn nút; `media_view.rs:376-377` "Huỷ" chỉ ẩn card, worker vẫn tải |
| 10 | `download.rs:245-256` | Chỉ Windows gắn MotW; **macOS không gắn `com.apple.quarantine`** |
| 11 | `media.rs:1415-1445` | Không có frame camera vẫn gửi **H.264 tổng hợp** ⇒ người nhận thấy test pattern mà không biết lỗi |
| 12 | `call.rs:2225`, `:2547` | Media lỗi → fallback `AudioHandle` = **loopback mic→loa của chính mình** ⇒ người dùng nghe lại giọng mình, đối phương không nghe gì |
| 13 | `media.rs:76/798/937/1804` | Spawn `"ffmpeg"` trần, **bỏ qua** `video::find_ffmpeg()` ⇒ máy chỉ có ffmpeg trong WinGet mất cả camera lẫn decode video call |
| 14 | `gui/worker.rs:460` | `CtrlPollState` (cursor `lp_control`) luôn `default()` ⇒ mỗi lần mở app tải lại toàn bộ control event từ lúc login (từng gây 413) |
| 15 | toàn `src/` | **Không ACK hàng đợi** (`DELETE_QUEUE_ACK_*`) ⇒ server có thể `evict` sớm; QTzalo chỉ phát hiện khoảng thiếu, không lấy lại được |
| 16 | `chat.rs:943-945`, `:898-905`, `:1481-1490`, `:2340-2349`, `:3289-3292`, `:2259/2265`, `:2481-2483` | Lỗi bị "làm phẳng" thành thành công: envelope giả `error_code:-1`; upload chunk lỗi → `break` rồi vẫn gửi; link/quote lỗi → âm thầm gửi text thường; timeout long-poll ≡ "không có tin"; quote dùng id bịa `now-1000`; `send_reaction` **không kiểm `error_code`** (khác mọi hàm gửi khác) |
| 17 | `chat.rs:3593-3604` | `poll_new_messages` gọi `getrecentv2` **tuần tự cho từng nhóm** (N+1 request, không rate limit) |
| 18 | `store.rs:61-95`, `:228-233`, `:371` | Không `PRAGMA user_version`/migration thật (chỉ `CREATE TABLE IF NOT EXISTS`); không `journal_mode=WAL`/`busy_timeout` (nhiều nơi mở DB, lỗi "database is locked" bị nuốt bằng `let _ =`); `delete_message` xoá thêm mọi tin cùng `(thread_id, ts)`; `hydrate_threads` nạp **toàn bảng** `msg_id→thread_id` vào RAM |
| 19 | `gui.rs:250-290` + `modals.rs:961-1603` | **~20 setting chỉ là UI**, không được đọc ở đâu: `show_typing_indicator`, `double_click_to_reply`, `suggest_mentions`, `autostart_on_boot`, `remember_login`, `allow_call_from`, … |
| 20 | `workspace.rs:1621-1645` (`demo.rs:264`) | Chỉ báo "đang nhập": UI có, **nguồn dữ liệu chỉ là demo**; socket `602` đã có hằng (`socket.rs:47`) nhưng bị `extract_voip_controls` (`call.rs:933`) vứt bỏ — tính năng rẻ, giá trị cao |
| 21 | `tui.rs:787-796`, `:1425`, `:679-687`, `:850-857` | Gửi lỗi **không rollback tin optimistic** (tin ma) ở text/sticker/reaction |
| 22 | `tui.rs:2413-2414`, `:1111-1150` | Overlay Log clone toàn bộ log mỗi frame 30 ms; một số phím (`q x r f u h l / ?`) bị chặn trước focus ⇒ không gõ được tin bắt đầu bằng các ký tự này |
| 23 | `sidebar.rs:325,331,349`, `modals.rs:1230` | Nav "Khác"/"Trò chuyện ẩn"/zCloud chỉ toast "Sắp có"; Cài đặt ghi rõ "Tải toàn bộ lịch sử từ máy chủ chưa được hỗ trợ" |
| 24 | toàn `src/` | **Không nhận diện E2EE/Signal** (0 kết quả grep) ⇒ khi Zalo bật E2EE, tin sẽ bị bỏ qua hoặc hiện rác thay vì bong bóng giữ chỗ |
| 25 | `src/bin/call_probe.rs`, `test_fetch.rs` | Công cụ dev **thay đổi tài khoản thật**: gọi/trả lời cuộc gọi, gửi lời mời kết bạn, sweep 20 biến thể 402, gửi tin nhắn, ghi dump JSON chứa PII |
| 26 | `paths.rs:55` | Thiếu `APPDATA`/`HOME` → fallback `PathBuf::from(".")` ⇒ quay lại hành vi ghi vào CWD như bản cũ |

**P2 — nhỏ hơn / nên gom vào đợt dọn dẹp**

| # | Vị trí | Vấn đề |
|---|---|---|
| 27 | `media.rs:5, :636` | Không jitter buffer/resample/echo-cancel; jitter RTCP luôn 0; `input_buf.lock().unwrap()` có thể panic thread audio |
| 28 | `voip_log.rs:92-108` | Mở file + mutex **trên luồng media realtime** mỗi dòng ⇒ nguy cơ jitter |
| 29 | `call.rs:32-305`, `:1882-1967`, `:12433-12447` | Dead code: cầu nối `ZaloCall.exe` (pipe + AES key hard-code), toàn bộ nhánh gọi nhóm (bị `call.rs:84` chặn) |
| 30 | `call.rs` ControlEvent | Không timeout đổ chuông, không tự đóng overlay nếu mất event `cancel`; hold/share screen chỉ có wrapper HTTP, không UI |
| 31 | `chat.rs:543,1447,1546,2308,2545,2605` | Host hard-code 6 lần, bỏ qua `endpoints.rs`/`domains` (đã healed từ bằng chứng live) |
| 32 | `chat.rs:3115-3116` | Danh sách queue đặc biệt hard-code `"6"|"15"|"16"`; queue 7/8 server báo thì không theo dõi |
| 33 | `sync_state.rs:245-248` | Gap luôn ghi `conv_id = "*"`, `from = newest_message_ts()` toàn cục ⇒ không quy được khoảng thiếu cho hội thoại nào |
| 34 | `login.rs:102` | `login_by_password_v1` gửi **mật khẩu plaintext**, không có dev-gate |
| 35 | `session.rs:649` | `--dump-raw` ghi URL đầy đủ gồm `params` + `signkey` ra đĩa |
| 36 | `socket.rs:344-348` | `ZCALL_WS_FORCE_VOIP` không bị dev-gate (env chèn queue vào production build) |
| 37 | `main.rs:65-75` | `--logout` không xoá `sync_state_<uin>.json` (GUI `:705-711` thì có) |
| 38 | `sidebar.rs:605-632`, `workspace.rs:347-358`, `messages.rs:1978-1980`, `modals.rs:46-55`, `workspace.rs:143-146` | Dữ liệu không thật hiển thị: thanh dung lượng mẫu số cứng 1 GB, "Tổng số tin nhắn" = số tin đang nạp, "Đã nhận" cứng, "Đang hoạt động"/"Nhóm đang hoạt động" luôn hiện |
| 39 | `Cargo.toml` | Không `[profile.release]` (không LTO/strip); không feature gate cho `src/bin/*` dev tool |
| 40 | `build.rs:54` | Chỉ `zalo-rust` có icon; không nhúng version resource/manifest |

### 4.3 Tồn đọng đã ghi trong tài liệu — xác nhận còn mở

Từ `BAN_GIAO.md` §8 và `KE_HOACH_TIEP_THEO.md` §4, kiểm chứng lại:

- **Đúng là còn mở:** S6 (UDP cuộc gọi không lọc theo relay; `srtpMode: 0` tại `call.rs:1707/1781`); chỉ báo "đang nhập" mới có ở demo; TUI/CLI chưa dùng con trỏ bền; chưa ACK hàng đợi; `totalMember` chưa thấy trong payload thật; chưa đồng bộ toàn bộ lịch sử (#19); chưa có nút huỷ tải; macOS chưa quarantine; emoji màu egui không vẽ được; bộ cài chưa ký số; 2 cảnh báo `cargo audit`; reaction tin cũ 1-1 thiếu `cliMsgId`; mã cảm xúc lạ hiện thành ❤️; hiệu năng hover (xử lý cho mọi bubble mỗi frame); tin đã thu hồi vẫn có nút cảm xúc; thiếu test cho `content` dạng object / `gMsgID` dạng chuỗi / `uidFrom` dạng số; `msgType` của tệp/video/voice/nhãn dán chưa kiểm chứng.
- **Không còn đúng (đã xong):** tự bắt kịp tin khi mở offline; danh sách hội thoại giống điện thoại; ảnh/video/tệp không còn hiện link.

### 4.4 Tài liệu và quy trình cần sửa (rẻ, nên làm ngay)

| # | Chỗ | Vấn đề |
|---|---|---|
| 1 | `README.md:48` | Quảng cáo `QTzalo-<ver>-macos-arm64.dmg` nhưng **v0.1.3 không có file này** (BAN_GIAO §0 ghi rõ). Người dùng sẽ tải nhầm |
| 2 | `README.md:164` | Quy trình "push, **đợi CI xanh**" **không thực hiện được**: job macOS best-effort bị huỷ ⇒ workflow luôn `failure` dù phát hành thành công |
| 3 | `README.md:62` | Danh sách file trong thư mục dữ liệu thiếu `sync_state_<uin>.json` (có thật ở `paths.rs:113-118`) |
| 4 | `README.md:183-203` | Sơ đồ mã nguồn thiếu 7 module: `download.rs`, `endpoints.rs`, `reaction.rs`, `sync_state.rs`, `trace_shape.rs`, `ui.rs`, `video.rs` |
| 5 | `docs/KIEN_TRUC_HE_THONG.md:46-52`, `:72` | `gui/` có 9 file (tài liệu nói 7, thiếu `media_view.rs`, `demo.rs`); SQLite có **4 bảng** (tài liệu nói 1) |
| 6 | `docs/CAI_DAT.md:12,53,61` | Ví dụ vẫn dùng phiên bản `0.1.0` |
| 7 | `docs/README.md:13-22` | Mục lục thiếu `docs/dev/KE_HOACH_TIEP_THEO.md` (kế hoạch hiện hành) |
| 8 | `docs/HUONG_DAN_SU_DUNG.md:9-19` | Thiếu `--demo`, `--dump-raw`, `--iso`, `--v1`, `-d`, `-v` |
| 9 | `.github/workflows/ci-release.yml` | CI **không** chạy `cargo audit`, **không** kiểm MSRV 1.88, **không** clippy/fmt; cài Inno Setup bằng `choco install` **không ghim phiên bản/checksum** (mọi action khác đều ghim SHA) |
| 10 | `docs/ZaloPC-DongBo-SaoLuu-TinNhan.md` §11 | Không đưa `db-cross-v4` vào "Câu hỏi mở" ⇒ người đọc sau tưởng đã xong B3.3 |
| 11 | Toàn kho tư liệu | Gộp nhầm **3 định dạng file** khác nhau: `backuppc2mb` (PC→phone, zstd + SyncChunk + AEAD), `db-cross-v4` (phone→PC v1, XZ + header/MD5 + AES + SQLCipher `uin`), `transfer-sync-v2` (v2, zstd + Sync2 + session cipher) |
| 12 | `notes/40–43`, `docs/` | Trích dẫn tới file **đã mất** (`apk/jadx-src/`, `apk/lib-arm64/`, `artifacts/asar/root/`) ⇒ không kiểm chứng lại được; cần ghi cảnh báo + giải nén lại |
| 13 | `re-notes/03_progress_audit.md` | Audit ngày 2026-10-21 ghi "`cargo check --all-targets`: FAIL" — đúng lịch sử nhưng dễ bị đọc nhầm là hiện tại (nay PASS) |

## 5. Đề xuất thứ tự công việc

### Đợt 1 — sửa lỗi P0 (1–2 ngày, không mở tính năng mới)

1. `store.rs:257` → `ORDER BY ts DESC LIMIT` rồi đảo lại; thêm `PRAGMA journal_mode=WAL; busy_timeout=5000` trong `MessageStore::open`.
2. Tách client tải không timeout tổng (hoặc timeout theo read) cho `download_to_dir`; thêm `CancelDownload` + nút Huỷ dùng chung cho tải tệp và `PrepareVideo`.
3. Cho TUI + nhánh đăng nhập mật khẩu dùng `SyncState` (bỏ `PollState::default()` ở `main.rs:256`, `tui.rs:205`, `worker.rs:2295`).
4. `call.rs:467` đọc `typeRequest` (fallback `type`).
5. Render `status_msg` (thanh trạng thái/toast) — nếu không, mọi báo lỗi của worker vô hình.
6. Bỏ `eprintln!` payload signaling ở `call.rs:2690/2424/2264` (đưa vào `--dev`).

### Đợt 2 — hoàn thiện hành vi (2–3 ngày)

7. Rollback optimistic khi gửi lỗi (reaction, sticker, text) ở GUI và TUI.
8. `send_reaction` kiểm `error_code`; thống nhất chính sách lỗi của `chat.rs` (bỏ envelope giả, bỏ "âm thầm gửi text thường").
9. Persist `CtrlPollState` vào `SyncState` (thêm field `#[serde(default)]`, nâng `VERSION` = 2 + nhánh migrate).
10. Bộ lọc `act_type == "typing"` cạnh `extract_voip_controls` + `GuiEvent` → bật chỉ báo "đang nhập" (đã có UI + bằng chứng).
11. Ẩn hoặc nối thật ~20 setting chết; bỏ dữ liệu giả ("Đang hoạt động", thanh dung lượng 1 GB, "Đã nhận").
12. `media.rs`: fallback camera → báo lỗi/tắt cam thay vì gửi bitstream tổng hợp; ffmpeg fallback → `video::find_ffmpeg()`; bỏ fallback loopback self-echo.

### Đợt 3 — mở Phần B (Sync V1), theo thứ tự rẻ → đắt

13. Bộ lọc sự kiện transfer (`syncmsg_info`, `user_confirm`, `transfer_error`, `transfer_after_login`) chạy `--dev`, chỉ log **cấu trúc** → trả lời luôn `act_type` [U] và một phần B6.1/B6.2 **không cần gọi** `pull_mobile_msg` (rủi ro tài khoản thấp).
14. Khôi phục tư liệu: giải nén `app.asar` + APKM **ra ngoài repo**, viết script vào `scripts/re/`, ghi cảnh báo vào `BAN_GIAO.md`.
15. Dịch ngược `Java_com_zing_zalo_db_BackupRestoreUtils_decodeFile` + `…_convertAndroidDbtoCross` trong `libzalosqlite.so` (còn symbol + chuỗi, cùng họ mã với `db-cross-v4`) ⇒ suy ra header → LZMA → AES **trước khi** đụng Ghidra ia32.
16. Dump chính thức `db-cross-v4-native.node` (strings ASCII/UTF-16 + `dumpbin /exports` + imports + SHA-256) vào `notes/`, đóng mục "NOT FOUND" ở `notes/sl-cipher.txt:140`.
17. Chỉ khi bước 15–16 chưa chốt: dựng hộp đen (B4) — cần file backup thật + Electron/Node ia32.
18. B5 tích hợp sau khi có định dạng; thêm tab mới (đừng tái dùng `SettingsTab::Sync` đang là "Dữ liệu & làm mới"), `WorkerCmd`/`GuiEvent` riêng, state machine tách khỏi GUI để test được.
19. **Không** làm Sync v2 trước (AEAD/KDF chưa xác định, cần trusted-device).

### Đợt 0 — sửa tài liệu (làm ngay, 1 buổi)

Mục §4.4: dòng `.dmg` trong README, quy trình "CI xanh", `sync_state_<uin>.json`, sơ đồ mã nguồn, `KIEN_TRUC`, `CAI_DAT` version, mục lục docs, bảng tham số CLI, thêm `cargo audit` + MSRV vào CI, ghim phiên bản Inno Setup.

## 6. Phụ lục — lệnh kiểm chứng đã chạy

```powershell
cd C:\Users\quang\OneDrive\Desktop\App\QTzalo\QTzalo
cargo test --locked                          # 225 passed, 0 failed, 1 ignored
cargo check --locked --all-targets           # PASS, 0 warning
cargo +1.88 check --locked --all-targets     # PASS (MSRV)
cargo audit                                  # 0 vulnerabilities, 2 unmaintained
git log -1; git status --short; git worktree list; git branch --no-merged main
```

Không file mã nguồn nào bị sửa trong suốt quá trình kiểm toán.
