# 11 — Sync V1 ("Đồng bộ từ điện thoại"): bản đồ Zalo PC (B3.1)

Ngày 2026-11-02. Phần B của `docs/dev/KE_HOACH_TIEP_THEO.md` §2, bước B3.1: chỉ đọc bản cài trên PC của người dùng. Không chạy mã của Zalo, không gọi máy chủ. Không commit mã/JS/native của Zalo. Kết quả thô (bản đồ asar, các đoạn trích) nằm ở `%USERPROFILE%\zalo-re\`. Đọc trực tiếp từ header của `app.asar` bằng một script Python nhỏ, không giải nén hết. Nhãn: **[O]** đã đọc thấy, **[I]** suy luận.

## Bản cài

- `%LOCALAPPDATA%\Programs\Zalo\Zalo-26.10.20\resources\app.asar` (177.728.582 byte, 13.169 file) cùng `app.asar.unpacked`. Máy còn bản `26.9.10` và `26.10.11`.
- `pc-dist/` (giao diện, worker), `main-dist/` (tiến trình chính), `native/` (72 file).
- **[O]** `db-cross-v4` chỉ có bản `native/nativelibs/db-cross-v4/prebuilt/window/electron_x86/db-cross-v4-native.node` (1.239.552 byte). `dist/binding.js` sẽ chọn `electron_x86_64` khi `process.arch === 'x64'`, nhưng bản đó không có trong gói → **[I]** Zalo PC chạy Electron 32-bit (ia32). Muốn dùng module làm "hộp đen" (B3.3) thì cần Node/Electron ia32 cùng ABI.

## Endpoint [O] (bảng lệnh trong `compact-app-pc.*.js`)

| cmd | Đường dẫn |
|---|---|
| 12000 | `/api/message/pull_mobile_msg` |
| 12700 | `/api/message/cancel_pull_mobile_msg` |
| 12003 | `/api/message/delete_snapshot_mobile_msg` |
| 12096 | `/api/message/get_backupmsginfo` |
| 12412 | `/api/message/get_crossdb` |
| 12888 | `/api/transfer-sync-v2/request-sync` (Sync v2) |

- Domain là **file domain**, hoặc dev domain khi `cross_setting.useDevDomain`. Tất cả đều là GET với `params` = `encodeParams(json)`, giống các API khác.
- `pullMobileMsg` gửi `{pc_name, public_key, from_seq_id, is_retry, min_seq_id, temp_key, imei}`. `from_seq_id` = con trỏ + 1, bằng 0 nếu chưa có. `imei` = Zalo client id.
- `cancel_pull_mobile_msg` gửi `{pc_name, public_key, imei}`.

## Nhận và giải mã bản sao lưu [O] (`lazy/main-startup.*.js`, `shared-worker.*.js`)

- Khi nhận `BACKUP_SUCCESS`, client lưu: `rawUid = data.uid`, `fromSeqId = data.from_seq_id`, `isFull = data.is_full_transfer`, `name = data.file_name`, `url = data.url`, `checksum = data.checksum_code || data.checksum`, `publicKey = data.public_key`, `encryptedKey = data.encrypted_key`, và `format = db_format ?? 0`. Sau đó đặt lại checkpoint `(format, minSeq, maxSeq)`. Hàm kiểm `isBackupFail` / `isMobileR…` nằm trong cùng module (`zb.a`). Kênh đưa sự kiện này tới vẫn chưa xác định (B3.2).
- Tác vụ `DECRYPT_BACKUP` (shared worker) xử lý theo `format`:
  - `0` → `decompressAndDecryptDb(inputPath, outputPath, privateKey)` → `{result, err_message}`;
  - `1` → `decompressAndDecryptDb_V2(inputPath, outputPath, privateKey.toUpperCase(), onProgress)` → `{result, inner_error, error_message}`. Tiến trình tính theo số hội thoại.
  - `result = 0` là thành công.
- **[I]** "Mật khẩu" của module native chính là `privateKey` thu được sau khi giải `encrypted_key` bằng RSA. Với format 1 thì truyền ở dạng hex viết hoa. Khớp tóm tắt trong `KE_HOACH_TIEP_THEO.md` §B1.
- Sau đó là các tác vụ `RESTORE_CONVERSATIONS` / `RESTORE_MESSAGES` có checkpoint. Thư mục tạm có dạng `blob_<id>`.

## Việc tiếp theo

- B3.2: tìm `zb.a.isBackupSuccess/isBackupFail`, xác định cmd/`act_type` của sự kiện transfer.
- B3.3: phân tích `db-cross-v4-native.node` (exports, thư viện nén/mã hoá), có tính đến kiến trúc ia32.
- B3.5: phía APK Android (khi người dùng đồng ý).
