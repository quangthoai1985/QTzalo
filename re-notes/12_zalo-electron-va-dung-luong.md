# 12 — Zalo PC (Electron): vì sao chạy ổn định, vì sao dung lượng phình — đo thực nghiệm

> **Ngày:** 2026-11-03. **Phạm vi:** đo read-only trên chính máy người dùng, Zalo PC `26.10.20`
> (`ProductVersion 26.10.20.3253`, VNG Corp.). **Không** chạy Zalo, **không** mở/đọc nội dung tin nhắn,
> **không** sửa file nào; chỉ đo kích thước, đếm file, đọc metadata file và đọc mã nguồn QTzalo.
> **Nhãn:** `[O]` đo/đọc được trực tiếp · `[I]` suy luận có căn cứ · `[U]` chưa chứng minh.
> **Không ghi dữ liệu cá nhân:** mọi UID được thay bằng `<uid>`, không có tên hội thoại, không có nội dung tin.

## 0. TL;DR

| Câu hỏi | Trả lời ngắn | Bằng chứng |
|---|---|---|
| Vì sao ổn định/trơn? | Kiến trúc nhiều tiến trình + tách riêng tiến trình rủi ro (gọi video, chụp màn hình, OCR) + code splitting/lazy + SQLite WAL chia nhỏ theo hội thoại + cache đĩa lớn + decode media bằng native | §8 |
| Vì sao phình? | **91,7 % dung lượng là cache media không có TTL** (20,5 GB); cộng 1,1 GB DB; 0,66 GB cache Chromium; **0,65 GB gói cập nhật còn sót**; **1,17 GB do 3 bản cài song song** | §7 |
| QTzalo học gì? | WAL + chia nhỏ ghi + cache có **giới hạn kích thước** + decode ngoài luồng chính | §11 |
| QTzalo tránh gì? | Cache media không eviction, ghi lại toàn bộ file mỗi vòng, log không cap, giữ nhiều bản cài | §11 |

## 1. Máy đo `[O]`

Intel Core i7-8700K @3,70 GHz (**6 core / 12 thread**) · RAM **31,9 GB** (còn trống 15,9 GB) ·
GPU **NVIDIA GTX 1660** (4.095 MB) · **Windows 11 Pro build 26200** · ổ C: dùng 288,2 GB / trống 664,7 GB ·
Zalo PC `26.10.20.3253` (còn 2 phiên bản cũ trên đĩa).

## 2. Bản cài: 3 phiên bản song song — 1,75 GB `[O]`

```powershell
Get-ChildItem "$env:LOCALAPPDATA\Programs\Zalo" -Directory |
  ForEach-Object { $s = Get-ChildItem $_.FullName -Recurse -File -Force | Measure-Object Length -Sum
                   "{0} {1:N1} MB {2} file" -f $_.Name, ($s.Sum/1MB), $s.Count }
```

| Phiên bản | Tổng | File | `Zalo.exe` | `app.asar` | `app.asar.unpacked` | `locales` | plugins |
|---|---|---|---|---|---|---|---|
| Zalo-26.10.20 (đang dùng) | **623,8 MB** | 282 | 130,3 | 169,5 | 81,6 | 26,3 | 160,4 |
| Zalo-26.10.11 | 606,4 MB | 274 | 130,3 | 167,6 | 80,6 | 26,3 | — |
| Zalo-26.9.10 | 561,2 MB | 265 | 130,3 | 165,8 | 80,6 | 26,3 | — |
| **Tổng** | **1,75 GB** | 827 | | | | | |

- `[O]` **`Zalo.exe` giống hệt nhau 130,3 MB ở cả 3 bản**; `locales` 26,3 MB và runtime Electron
  (~56 MB DLL/PAK) cũng gần như trùng ⇒ **2 bản cũ chiếm 1.167,6 MB mà gần như chỉ để rollback**.
- `[O]` Đóng gói kèm **25,09 MB file `.pdb`** (ký hiệu gỡ lỗi) trong `plugins\capture\pdbs\`.
- `[O]` `app.asar` 250,3 MB / 13.169 file, `headerSize` 769.188 B, `dataOffset` 769.196 B
  (bảng offset tập trung ⇒ mở app không cần I/O từng file — xem §8).
- `[O]` Trong asar có **cả 3 kiến trúc**: ia32 31 file / 70,6 MB, **x64 8 file / 10,3 MB**, darwin 2 file / 0,1 MB.
  App chạy ia32 ⇒ **~10,4 MB native x64/darwin là hàng chết** `[I]`.

## 3. Dữ liệu người dùng: 22,37 GB / 35.423 file `[O]`

Thư mục thật: `%APPDATA%\ZaloData` (không phải `%APPDATA%\Zalo`, không phải `%LOCALAPPDATA%\Zalo`).
Ngoài ra `%LOCALAPPDATA%\zalo-updater` = 169,1 MB.

| Nhóm | Dung lượng | % ZaloData | File |
|---|---|---|---|
| `media\` (cache media + gói cập nhật) | **21.011,5 MB** | **91,7 %** | 27.585 |
| `Database\` | 1.096,2 MB | 4,8 % | 2.711 |
| `Partitions\` (profile Chromium) | 363,6 MB | 1,6 % | 4.648 |
| `Cache\` (HTTP cache) | 304,1 MB | 1,3 % | 368 |
| `ftsm_<uid>.db` (+ `-wal`, `-shm`) | 76,3 MB | 0,3 % | 3 |
| `cal\` (log nén `.calf`) | 56,4 MB | 0,2 % | 13 |
| còn lại (`tf-models`, `resp_cache`, GPUCache, DawnCache, log) | ~8 MB | 0,03 % | — |
| **Tổng** | **22.911 MB (22,37 GB)** | 100 % | 35.423 |

Thư mục được tạo `2026-05-12` ⇒ **bản cài hiện tại mới 175 ngày**.

## 4. Media 20,5 GB — nguyên nhân số 1, và không có TTL ≤ 90 ngày `[O]`

| Thư mục con (`media\<uid>\ZaloDownloads\`) | MB | File | Tạo ngày |
|---|---|---|---|
| `resource\` (path structure v2) | **12.501,1** | 16.561 | **2026-07-29** |
| `fileNoise\` | 2.839,7 | 1.670 | 2026-05-12 |
| `video\` | 2.596,7 | 337 | 2026-05-12 |
| `picture\` | 1.453,0 | 4.469 | 2026-05-12 |
| `file\` | 555,7 | 90 | 2026-05-12 |
| `Cache\` (cache media riêng của Zalo) | 509,7 | 2.874 | 2026-05-12 |
| `richThumb\` / `voice\` / `zinstant\` | 30,1 / 6,1 / 2,5 | 276 / 79 / 198 | 2026-05-12 |
| `sticker\` (ngoài ZaloDownloads) | 40,1 | 1.026 | — |

**Phân bố tuổi file media** (27.581 file, tổng 20,05 GB):

| Mốc | Số file | % |
|---|---|---|
| cũ nhất | **175 ngày** (2026-05-12) | — |
| trung vị | **122 ngày** | — |
| cũ hơn 7 ngày | 26.558 | 96,3 % |
| cũ hơn 30 ngày | 24.084 | 87,3 % |
| cũ hơn 90 ngày | 19.279 | **69,9 %** |
| cũ hơn 180 ngày | 0 | 0 % |

- `[O]` **Không có TTL ≤ 90 ngày**: nếu có, 70 % số file đã bị xoá.
- `[U]` Có TTL 180/365 ngày hay không **không phân biệt được**, vì thư mục chỉ mới 175 ngày.
- `[I]` `resource\` được tạo **2026-07-29**, muộn hơn 2,5 tháng so với các thư mục khác ⇒ khớp mốc
  migrate "path structure v2" (`notes/30-backup-media.md` §8.4). Đã kiểm tra trùng lặp: trong 400 tên
  mẫu của `fileNoise`, **chỉ 1 tên** cũng có trong `resource` (0,3 %) ⇒ `resource\` là **bổ sung thật**,
  không phải bản sao của `fileNoise`. Đây là **12,5 GB nội dung mới trong ~3 tháng**.

## 5. Database: 1,07 GB, chia rất nhỏ `[O]`

| Hạng mục | MB | File |
|---|---|---|
| `Media.db` | 270,2 | 1 |
| `Core\Index.db` | 106,4 | 1 |
| `Core\Message\<convId>.db` (1 file / hội thoại) | **647,2** | **2.094** |
| `-wal` (write-ahead log) | 49,1 | 301 |
| `-shm` | 9,4 | 301 |
| `Res.db` / `Reaction.db` / `MsgInfo.db` / `Meta.db` / `Sync.db` | 12,2 | 5 |
| `ftsm_<uid>.db` (full-text search) | 72,2 | 1 |

- `[O]` 2.094 DB hội thoại: **1.406 chat 1-1 + 688 nhóm**; **trung vị 0,03 MB**;
  **67,9 % số file < 50 KB nhưng chỉ chiếm 7,1 % dung lượng**; file lớn nhất 86,8 MB.
- `[I]` Mỗi hội thoại một file ⇒ ghi/đọc chỉ khoá đúng một file, không khoá toàn bộ lịch sử.
  Đây là lý do kỹ thuật chính khiến thao tác chat không giật dù có 2.094 hội thoại.

## 6. Cache, log và gói cập nhật còn sót `[O]`

| Mục | MB | File | Ghi chú |
|---|---|---|---|
| `ZaloData\Cache\Cache_Data` | 304,1 | 368 | file cũ nhất **137 ngày** |
| `Partitions\zalo\Cache` | 355,9 | 4.500 | file cũ nhất **175 ngày** (từ ngày cài) |
| `Code Cache` + `GPUCache` + `DawnCache` | 4,4 | 38 | JIT cache của V8 + GPU |
| `cal\main.calf` / `render.calf` / `shared-worker.calf` / `u-process-sqlite.calf` | 20 / 20 / 10 / 4,5 | 4 | log nén, có cap ~20 MB/tiến trình ⇒ **tổng log bị chặn** |
| **`media\update\ZaloModule-26.10.20_Full-RC4.zip`** | **290,2** | 1 | gói cập nhật, `2026-10-21` |
| **`media\temp\` (2 file)** | **186,5** | 2 | `2026-06-19`, không bị dọn sau 4,5 tháng |
| **`%LOCALAPPDATA%\zalo-updater`** | **169,1** | 1 | bộ cài/updater tải về |
| **Tổng rác cập nhật** | **645,8** | 4 | |

- `[O]` Cache Chromium cộng lại ≈ **664 MB** và **không được trim** (có file từ ngày cài).
- `[O]` `log.log` 25,4 KB, `startup.log` 5,4 KB, `update.log` 578 KB ⇒ **log text bị chặn tốt**;
  chỉ log nén `.calf` lớn nhưng có cap theo tiến trình. Kết luận: **log không phải nguyên nhân phình**.

## 7. Kiểm toán dung lượng — cộng lại `[O]`

| Nguyên nhân | Dung lượng | % tổng 24,3 GB | Có dọn được? |
|---|---|---|---|
| Cache media (`ZaloDownloads` + `sticker`) | 20,53 GB | 84,5 % | một phần (không có UI dọn) |
| Database (+ WAL/SHM + ftsm) | 1,15 GB | 4,7 % | không (dữ liệu thật) |
| Profile Chromium (`Partitions`) | 0,36 GB | 1,5 % | có |
| HTTP cache (`Cache`) | 0,30 GB | 1,2 % | có |
| **3 bản cài song song** (`Programs\Zalo`) | **1,75 GB** | 7,2 % | **có — giữ 1 bản tiết kiệm 1,17 GB** |
| **Gói cập nhật/temp còn sót** | **0,65 GB** | 2,7 % | **có** |
| log `.calf` | 0,06 GB | 0,2 % | có (cap sẵn) |
| **TỔNG dấu chân** | **~24,3 GB** | 100 % | |

## 8. Vì sao Electron ở đây ổn định và trơn `[O]/[I]`

1. **Tách tiến trình thật, không phải danh nghĩa.** `[O]` Đang chạy **10 tiến trình**, tổng working set
   **999 MB**: 8 × `Zalo` (300,8 / 147,5 / 140,4 / 119,5 / 71,8 / 68,7 / 52,7 / 45,1 MB) + `ZaloCall` 31,7 MB
   + `ZaloCap` 20,6 MB. Renderer treo không kéo main process; GPU process riêng.
2. **Đẩy rủi ro ra tiến trình riêng bằng file `.exe` độc lập.** `[O]` `plugins\capture\` (117,1 MB) chứa
   `ZaloCall.exe` 18,68 MB, `ZaviMeet.exe` 18,90 MB, `ZaloCap.exe` 5,23 MB; `plugins\ocr\` (43,3 MB) chứa
   `zocr-host.exe` 4,84 MB. `[I]` Gọi video / chụp màn hình / OCR — ba thứ dễ crash và dễ ngốn CPU nhất —
   chạy ngoài renderer, chết thì chỉ chết tiến trình con.
3. **Code splitting + lazy load.** `[O]` 116 file JS; **55 file trong `pc-dist/lazy/` (26,3 MB)** chỉ nạp khi
   vào màn hình tương ứng; **31 file `*worker*` (74,7 MB)** mỗi worker một luồng riêng. Nhưng
   **5 file > 5 MB chiếm 84,4 MB** (sync-v2-sub-worker 20,21, search-worker 20,18, compact-app-pc 19,96) ⇒
   `[I]` phần lõi vẫn là 3 bundle khổng lồ, lazy chỉ cứu phần UI phụ.
4. **SQLite WAL + chia nhỏ theo hội thoại** (§5): ghi không khoá toàn cục, `-wal` chỉ 49 MB cho 2.094 DB.
5. **Decode media bằng native, ngoài JS.** `[O]` `native\nativelibs\` có `zimage` (libvips 3,31 MB),
   `zjxl` (jxl.dll 6,25 MB + opencv_imgproc 15,84 MB + opencv_core 9,03 MB + brotlienc 4,33 MB),
   `mp4thumb` (x64 3,42 MB + ia32 2,57 MB) ⇒ `[I]` ảnh/video/JPEG-XL được decode trong C++, không chặn UI thread.
6. **Cache đĩa lớn có chủ đích** (§6): 664 MB cache Chromium + 510 MB cache media riêng của Zalo
   ⇒ mở lại ảnh/video cũ gần như tức thì (đây chính là cái giá của sự "trơn tru").
7. **WASM cho crypto + compositor GPU.** `[O]` 3 file WASM (0,73 MB: `libzproto_wasm`, `libzproto_backup_wasm`,
   `trusted_device_protocol`) ⇒ lỗi bộ nhớ trong crypto không làm sập renderer; `GPUCache`/`DawnCache`/
   `vk_swiftshader.dll` (4,28 MB) ⇒ hiệu ứng do compositor lo, không phải JS vẽ lại.
8. `[U]` **Không xác định được phiên bản Electron** chỉ bằng file: có `icudt57.dll` (ICU rất cũ, Chromium ~57)
   **đồng thời** `icudtl.dat` 9,98 MB, `vk_swiftshader.dll`, `DawnCache`, `napi-v6-win32-ia32` (N-API 6 ⇒ Node ≥ 10).
   Dấu hiệu mâu thuẫn ⇒ cần chạy `Zalo.exe --version` (ngoài phạm vi được phép).

## 9. Cái giá của Electron ở đây `[O]`

| Khoản | Số đo |
|---|---|
| RAM thường trực | **999 MB** / 10 tiến trình (khi đang mở chat) |
| Đĩa 1 phiên bản | 623,8 MB = electron ~56 MB DLL/PAK + exe 130,3 + asar 250,3 + plugins 160,4 + locales 26,3 |
| Đĩa do giữ 3 phiên bản | +1.167,6 MB |
| Cache không TTL | 20,5 GB media + 0,66 GB Chromium |
| Rác cập nhật | 645,8 MB |

## 10. QTzalo hiện tại — đo được `[O]`

| Hạng mục | Số đo |
|---|---|
| Dữ liệu `%APPDATA%\QTzalo` | **64,69 MB / 5.855 file** |
| trong đó `stickers_cache\` | **63,64 MB / 5.850 file (98,4 %)** |
| `threads_cache.json` | 0,65 MB |
| `zalo_history.db` | 0,4 MB |
| `sync_state_<uid>.json` | ~0 MB |
| Binary debug (`target\debug\zalo-rust.exe`) | 30,1 MB |
| **`target\` (artefact build)** | **13.437 MB / 14.432 file** |
| Toàn repo | 13.449,8 MB |

`[I]` QTzalo **đang lặp lại đúng lỗi của Zalo**: cache sticker tăng đơn điệu, chiếm 98 % dữ liệu, chưa có
eviction. Khác biệt duy nhất: QTzalo còn nhỏ (64 MB so với 20,5 GB) vì mới dùng ít.

## 11. Khuyến nghị cho QTzalo (ưu tiên giảm dần)

| # | Việc | Ảnh hưởng | Chi phí | Vị trí |
|---|---|---|---|---|
| 1 | **Giới hạn + TTL cho `stickers_cache`** (LRU theo tổng byte, ví dụ 64 MB / 30 ngày) | **Đĩa** (98 % dữ liệu hiện tại) | nhỏ — đã có mẫu `cleanup_clipboard_images` | `src\paths.rs:128-167`, `src\gui\stickers.rs` (`sticker_textures` chỉ `contains_key`/`get`, không evict: `:955`, `:995`, `:1223`, `:1249`, `:1512`, `:1546`) |
| 2 | **Retention + `VACUUM` cho `messages`**: bỏ/không lưu `raw_json` cho tin cũ, xoá theo tuổi, `PRAGMA auto_vacuum=INCREMENTAL` rồi `VACUUM` định kỳ | Đĩa + độ mượt (DB phình ⇒ query chậm) | vừa | `src\store.rs:76-87` (schema có `raw_json TEXT`, **không** có retention/VACUUM nào trong repo) |
| 3 | **Bỏ ghi toàn bộ `threads_cache.json` mỗi vòng** — chỉ ghi khi dirty, hoặc bỏ hẳn và đọc từ `threads` table | Đĩa (write amplification) + độ mượt lúc refresh | nhỏ | `src\chat.rs:359-362`, gọi ở `src\chat.rs:3612` |
| 4 | **Prune `downloads: HashMap<String, DownloadState>`** theo LRU/giới hạn N mục | **RAM** (phình theo số URL từng xem) | nhỏ | `src\gui\media_view.rs:167-205`, `src\gui\messages.rs:3007` |
| 5 | **Đóng gói giống Zalo: 1 bản cài duy nhất**, updater xoá gói cũ sau khi cập nhật | Đĩa (1,17 GB ở Zalo) | nhỏ (quy trình) | `packaging\`, `scripts\` |
| 6 | **Lệnh "Dọn dẹp + hiện dung lượng"** trong GUI (Zalo **không có** ⇒ cơ hội khác biệt) | Đĩa, minh bạch | vừa | `src\gui\` (Settings), dùng `paths::cache_dir()` |
| 7 | **Log có cap cứng + xoay 1 bản** — giữ nguyên cách làm hiện tại, đừng mở rộng | Đĩa | không | `src\voip_log.rs:28` (`ROTATE_BYTES = 2 MiB`) — **đã đúng, không sửa** |
| 8 | **Không sửa nhịp `sync_state`** — đã có chống ghi trùng | — | không | `src\gui\worker.rs:517-541`: tick 2 s nhưng `if last_saved == Some(&st) { continue }` ⇒ **chỉ ghi khi con trỏ đổi** |

**Đính chính so với giả thiết ban đầu** (để lần sau không sửa nhầm chỗ):
- `store.rs:257` là nhánh **xoá** tin (`DELETE FROM messages WHERE msg_id = ?1`), không phải nhánh nạp.
  Điểm đáng chú ý thật nằm ở `store.rs:352` (`DELETE FROM messages WHERE text = 'sendBubbleMessage'`) và ở việc **không có retention/VACUUM**.
- `sync_state` **không** ghi 2 s/lần — đã có guard so sánh (`worker.rs:534`); `voip_log` đã bị chặn ở 2 MiB / 1 bản sao ⇒ không phải nguồn phình.

**Phân loại ảnh hưởng:** đĩa → #1, #2, #3, #5, #6 · RAM → #4 · độ mượt → #2, #3 · không cần làm → #7, #8.

## 12. Trả lời trực tiếp ba câu

**(a) Vì sao Zalo PC ổn định và trơn?** `[O]/[I]` Vì nó **chia nhỏ mọi thứ và cô lập mọi rủi ro**: 10 tiến trình
riêng (999 MB), 3 subsystem dễ chết (call / capture / OCR) đẩy ra `.exe` riêng, 2.094 file DB theo hội thoại
với WAL, 31 worker + 55 chunk lazy, decode ảnh/video bằng native (vips/JXL/opencv/mp4thumb), crypto trong WASM,
và **664 MB cache đĩa** để không phải tải lại. Cái "trơn" phần lớn **được mua bằng bộ nhớ và đĩa**.

**(b) Vì sao dung lượng ngày càng phình?** `[O]` Vì **cache media không có TTL**: 20,53 GB = 84,5 % dấu chân,
trong đó `resource\` **12,5 GB mới sinh trong ~3 tháng** sau đợt migrate path-v2. Không có bằng chứng của bất kỳ
chính sách xoá nào ≤ 90 ngày (70 % file đã cũ hơn 90 ngày). Cộng thêm ba khoản "quên dọn" thuần tuý:
**1,17 GB do 3 bản cài song song**, **0,65 GB gói cập nhật/temp còn sót**, **0,66 GB cache Chromium không trim**.
Log **không** phải nguyên nhân (đã cap).

**(c) QTzalo nên học gì / tránh gì?** `[I]`
- **Học:** WAL + `busy_timeout` (đã có, `store.rs:71-73`); ghi theo lô nhỏ; cache đĩa có **giới hạn cứng**
  (Zalo *thiếu* điều này — QTzalo phải làm tốt hơn); decode ngoài UI thread; log có cap.
- **Tránh:** (1) cache không eviction — QTzalo đã mắc với sticker (98 % dữ liệu); (2) schema lưu `raw_json`
  toàn bộ mà không retention; (3) ghi lại toàn bộ file cache mỗi vòng; (4) giữ nhiều bản cài;
  (5) không có UI dọn dẹp. Nếu làm 5 điều này, QTzalo sẽ **không** lặp lại "hội chứng 22 GB" của Zalo.

## 13. Lệnh tái lập

```powershell
# A+B. dung lượng từng mục (bản cài & dữ liệu)
function Size($p){ (Get-ChildItem $p -Recurse -File -Force | Measure-Object Length -Sum) }
foreach($v in Get-ChildItem "$env:LOCALAPPDATA\Programs\Zalo" -Directory){ "{0} {1:N1} MB" -f $v.Name,((Size $v.FullName).Sum/1MB) }
$d="$env:APPDATA\ZaloData"; foreach($i in Get-ChildItem $d -Force){ if($i.PSIsContainer){$m=Size $i.FullName; "{0}\ {1:N1} MB {2} file" -f $i.Name,($m.Sum/1MB),$m.Count} }

# C. tuổi file media (dấu hiệu TTL) / D. asar / E. RAM
Get-ChildItem "$d\media" -Recurse -File -Force | ForEach-Object { ((Get-Date)-$_.LastWriteTime).TotalDays } | Measure-Object -Maximum -Minimum -Average
$o=Get-Content '<repo>\artifacts\asar\filelist.json' -Raw | ConvertFrom-Json
$o.files | Sort-Object size -Descending | Select-Object -First 20 | ForEach-Object { "{0,8:N2} MB {1}" -f ($_.size/1MB), $_.path }
($o.files | Where-Object { $_.path -match '\.js$' } | Measure-Object size -Sum).Sum/1MB
Get-Process Zalo,ZaloCall,ZaloCap,zocr-host -ErrorAction SilentlyContinue | Measure-Object WorkingSet64 -Sum
```
*Số đo tại 2026-11-03 23:2x; dung lượng làm tròn 0,1 MB. Hết ghi chú 12.*
