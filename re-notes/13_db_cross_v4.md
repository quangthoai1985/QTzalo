# 13 — `db-cross-v4` (B3.3): bản đồ nhị phân từ PE header đến vùng mã

> **⚠️ ĐÍNH CHÍNH (04/11/2026):** `re-notes/14_db_cross_v4_disasm.md` đã disassemble bằng `dumpbin`
> (có sẵn trong VS Build Tools, không cần PATH) và **sửa hai kết luận của file này**: (1) 4 con trỏ ở
> export `0x103334` **không phải** 4 hàm N-API mà là bảng khởi tạo CRT — 4 hàm thật là
> `getVersion=0x119A`, `parseBinNet=0x1C35`, `decompressAndDecryptDb=0x6299`, `_V2=0x665E`;
> (2) header có magic **`"ZDB4.0"`**, đọc **big-endian**, băm luồng 64 KiB/lần. Giữ file này làm
> bản đồ section/import/chuỗi; đọc file 14 cho phần mã.

> **Ngày:** 2026-11-03. **Phạm vi:** phân tích **tĩnh** `db-cross-v4-native.node`
> (`%LOCALAPPDATA%\Programs\Zalo\Zalo-26.10.20\resources\app.asar.unpacked\native\nativelibs\db-cross-v4\prebuilt\window\electron_x86\`).
> **Không chạy mã Zalo, không gọi mạng, không sửa file trong repo.** Công cụ: Python 3.12 thuần stdlib
> (script ngoài repo tại `%TEMP%\qtzalo_re\`). `dumpbin`/`objdump`/`rabin2` **không có** trong PATH
> ⇒ mọi số liệu dưới đây do script tự parse PE; phần dịch ngược mã **chưa làm**.
> **Nhãn:** `[O]` đọc/đo trực tiếp từ byte · `[I]` suy luận có căn cứ · `[U]` chưa chứng minh.
> **Không chứa** nội dung tin nhắn/dữ liệu cá nhân.

## 0. Kết quả trong một bảng

| Câu hỏi | Trả lời | Nhãn |
|---|---|---|
| File là gì | PE32 (i386), DLL `.node`, 8 section, ImageBase `0x10000000`, EP `0x6C62`, không overlay | `[O]` |
| Export | **1 export duy nhất**: `_register_addon_`; 4 hàm JS **không** xuất theo tên | `[O]` |
| 4 hàm JS nằm ở đâu | Bảng 4 con trỏ tại RVA `0x103334` → RVA `0x12880`, `0xE120`, `0xE213`, `0xE1A7` | `[O]` |
| Import | `KERNEL32.dll` 160 hàm + `WSOCK32.dll` 4 hàm; `node.exe` 39 hàm `napi_*` qua **delay-load** | `[O]` |
| Crypto/nén liên kết tĩnh? | **Có.** Không import `Crypt*`/`BCrypt*`/`EVP_*`/`OpenSSL` | `[O]` |
| Nén là gì | **LZMA/XZ**: magic `FD 37 7A 58 5A 00` @RVA `0x10586C` + hằng số `SZ_OK`/`Z_ERROR_DATA` + bảng CRC32 + SHA-256 của liblzma | `[O]`/`[I]` |
| Mã hoá là gì | **AES (bảng S-box + invS-box 512 B @RVA `0x1048D8`)**, tự viết, khoá suy từ `password` | `[O]`/`[I]` |
| Toàn vẹn | **MD5** (hằng số IV inline 3 chỗ trong `.text`) + `checksum mismatch` | `[O]` |
| Đủ để viết Rust? | **Chưa** — thiếu layout header, KDF, mode/padding, cách đóng gói block | `[O]` |

## 1. PE header `[O]`

`e_lfanew=0x120` · Machine `0x014C` (**i386**) · 8 section · TimeDateStamp `0x69AAB4D4` (**2026-03-06 11:04:52 UTC**)
· Characteristics `0x2102` (EXECUTABLE + 32BIT + DLL) · optional magic `0x010B` (**PE32**) · linker **14.38** (VS2022 17.8).

| Trường | Giá trị |
|---|---|
| AddressOfEntryPoint · BaseOfCode · BaseOfData | `0x6C62` · `0x1000` · `0x103000` |
| ImageBase · Section/File alignment | `0x10000000` · `0x1000` / `0x200` |
| SizeOfImage · SizeOfHeaders · CheckSum · Subsystem | `0x13A000` · `0x400` · `0x0` · `2` (GUI) |
| DllCharacteristics | `0x0140` = **ASLR (`0x40`) + NX (`0x100`)**; **không** CFG (`0x4000`=0) dù có section `.00cfg` |
| Overlay sau section cuối | **0 byte** (file = `0x12EA00`) · `.tls`: không có |
| Debug/CodeView | `RSDS`, GUID `d9f35b4e…85ce86`, age 1, PDB `D:\Workspace\nativelibs\db-cross-v4\build\Release\db-cross-v4-native.pdb` |

**Data directories:** EXPORT `0x123350/0x16D` · IMPORT `0x12D368/0x3C` · RESOURCE `0x131000/0x43C` ·
BASERELOC `0x132000/0x677C` · DEBUG `0x118BA8/0x38` · LOAD_CONFIG `0x118AA0/0x40` · IAT `0x12D000/0x368` · DELAY_IMPORT `0x12F000/0x40`.

## 2. Sections `[O]`

| # | Tên | VA | VSize | RawOff | RawSize | Characteristics |
|---|---|---|---|---|---|---|
| 0 | `.text` | `0x00001000` | `0x1018B7` | `0x00000400` | `0x101A00` | `0x60000020` (code, exec, read) |
| 1 | `.rdata` | `0x00103000` | `0x0204BD` | `0x00101E00` | `0x020600` | `0x40000040` (init data, read) |
| 2 | `.data` | `0x00124000` | `0x008774` | `0x00122400` | `0x002600` | `0xC0000040` (init data, RW) |
| 3 | `.idata` | `0x0012D000` | `0x0015D7` | `0x00124A00` | `0x001600` | `0x40000040` |
| 4 | `.didat` | `0x0012F000` | `0x0008CC` | `0x00126000` | `0x000A00` | `0xC0000040` (delay-load) |
| 5 | `.00cfg` | `0x00130000` | `0x00010E` | `0x00126A00` | `0x000200` | `0x40000040` |
| 6 | `.rsrc` | `0x00131000` | `0x00043C` | `0x00126C00` | `0x000600` | `0x40000040` |
| 7 | `.reloc` | `0x00132000` | `0x00768D` | `0x00127200` | `0x007800` | `0x42000040` |

`[I]` `.text` chiếm **1,03 MB / 1,24 MB (83 %)** ⇒ gần như toàn bộ là thư viện liên kết tĩnh
(LZMA + AES + MD5 + C++ runtime), không phải code nghiệp vụ.

## 3. Export: 1 tên, nhưng 4 hàm ẩn `[O]`

`NumberOfFunctions = 1`, `NumberOfNames = 1`, DLL name `db-cross-v4-native.node`, ordinal base 1
⇒ **export duy nhất: `_register_addon_` @RVA `0x103334`**.

RVA `0x103334` nằm trong `.rdata` và **chứa một bảng 4 con trỏ hàm** (4 byte mỗi con trỏ, xen 0):
`0x10012880` → RVA **`0x012880`** · `0x1000E120` → **`0x00E120`** · `0x1000E213` → **`0x00E213`** ·
`0x1000E1A7` → **`0x00E1A7`**.
`[I]` Bốn con trỏ này là 4 adapter N-API, theo thứ tự khai báo chuỗi trong `.rdata` (`getVersion` @`0x104174`,
`parseBinNet` @`0x104184`, `decompressAndDecryptDb` @`0x104194`, `…_V2` @`0x1041B0`) thì lần lượt là
`getVersion`, `parseBinNet`, `decompressAndDecryptDb`, `_V2` — **cần xác nhận** bằng bảng property-descriptor
mà chúng dựng (xem §10 việc 1). ⇒ `require()` **không** trả 4 hàm theo tên export; chúng được
`napi_module_register` (từ `node.exe`) đăng ký động.

## 4. Import: chứng minh crypto/nén liên kết tĩnh `[O]`

`KERNEL32.dll` **160 hàm** (file I/O `CreateFileA/W`/`ReadFile`/`WriteFile`/`SetFilePointer`/`SetEndOfFile`/
`GetFileSize`; bộ nhớ `VirtualAlloc/Free/Query/Protect`; đồng bộ `*CriticalSection`, `TryAcquireSRWLockExclusive`;
heap; debug) · `WSOCK32.dll` **4 hàm** (di sản CRT) · `node.exe` **39 hàm `napi_*`** qua **delay-load**
(`napi_module_register`, `napi_define_properties`, `napi_create_function`, `napi_get_buffer_info`,
`napi_create_string_utf8`, `napi_throw`, …).
**Bằng chứng âm `[O]`:** không có `Crypt*`, `BCrypt*`, `CryptAcquireContext`, `EVP_*`, `OpenSSL`
trong toàn file ⇒ mọi mật mã nằm trong `.text`.

## 5. Chuỗi mốc (RVA · file offset · hàm dùng nó) `[O]`

Xref tìm bằng cách quét `imm32 = ImageBase + RVA` trong `.text` (4062 prologue phát hiện theo heuristic
`55 8B EC`; cột "hàm chứa" là **xấp xỉ**, dùng để khoanh vùng).

| Chuỗi | RVA | file | Xref (RVA) | Vùng hàm |
|---|---|---|---|---|
| `read header fail. error:` | `0x1043E4` | `0x1031E4` | `0x1B0AB, 0x1B0B3, 0x1B0C8, 0x1B0F3, 0x1B128` | **`0x1AA00`–`0x1B2xx`** (header) |
| `invalid signature` | `0x104378` | `0x103178` | `0x1BEA3` | `0x1BCC0` |
| `invalid hash data length` · `checksum mismatch` · `crr_no_file == nfile` | `0x104390/3B0/3C8` | `0x103190/1B0/1C8` | `0x1AA87`, `0x1AC70`, `0x1B211` | **`0x1AA00`** |
| `EOF while restoring` · `found file: ` · `list_files.size(): ` · `start decompress` | `0x104404/41C/440/42C` | `0x103204/21C/240/22C` | `0x1B023, 0x1AE0C, 0x1B242, 0x1B22D` | `0x1AA00` |
| `Incorrect key length` | `0x104B40` | `0x103940` | `0x26416` | **`0x25980`** (crypto) |
| `Password can not empty. FAIL` | `0x104BD0` | `0x1039D0` | `0x28113` | `0x25980` |
| `Input & output,password can not null` | `0x104B64` | `0x103964` | `0x28866` | `0x25980` |
| `MD5 read from encrypted file: %s` | `0x104C4C` | `0x103A4C` | `0x282AA` | `0x25980` |
| `Res != SZ_OK` | `0x104C98` | `0x103A98` | `0x28719` | `0x25980` |
| `Z_ERROR_DATA` | `0x104CA8` | `0x103AA8` | `0x286FE` | `0x25980` |
| `Decrypt input size = %ld` · `Start decrypt file` · `Decryption sucess` | `0x104F1C/3C/6C` | `0x103D1C/3C/6C` | `0x29362, 0x29373, 0x293E4` | `0x25980` |
| `Md5 not correct !` | `0x104F84` | `0x103D84` | `0x29405` | `0x25980` |
| `File size: %lld` | `0x1054B4` | `0x1042B4` | `0x3AB1F` | `0x25980` |
| `MD5 canculate of compress file: %s` | `0x1054C8` | `0x1042C8` | `0x3ABB9` | `0x25980` |
| `Compress database success.` | `0x105038` | `0x103E38` | `0x28E9C` | `0x25980` |
| `encode_stream reval: %d` | `0x10515C` | `0x103F5C` | `0x290CC` | `0x25980` |

## 6. Hằng số & codec `[O]`

| Hạng mục | Vị trí (RVA · file) | Xref | Ý nghĩa |
|---|---|---|---|
| **XZ magic** `FD 37 7A 58 5A 00` | `0x10586C` · `0x10466C` | `0x3F603` (hàm `0x3F5F0`), `0x43358` (hàm `0x43350`) | container **XZ/LZMA2** |
| SHA-256 (`H0..H3` `6a09e667…`; `K[0..3]`) | `0x105878` · `0x104678`; `0x105898` · `0x104698` | `0x44FA9`, `0x45993` | SHA-256 (check của liblzma) |
| Bảng CRC32 (`00000000 77073096`) | `0x105AF8` · `0x1048F8` | `0x44A80, 0x44AF2, 0x44B34, 0x530A9, 0x53248` | CRC (XZ check) |
| **AES S-box + inv S-box** (512 B liền) | `0x1048D8` · `0x1036D8` (inv `0x1049D8` · `0x1037D8`) | `0x27B13…0x27BCF`, `0x27453` | **AES** (mã hoá + giải mã) |
| Rcon `04×3 08×3 10×3 20×3 40×3` · version `5.3.1alpha` · `node.exe` | `0x10552D` · `0x10432D`; `0x10550C` · `0x10430C`; `0x105500` · `0x104300` | Rcon: không thấy imm32; version: `0x3C0C1` (hàm `0x3BF70`) | key expansion; `[I]` **xz Utils 5.3.1alpha / liblzma**; tên DLL delay-load |

**MD5 `[O]`:** IV `01 23 45 67 89 AB CD EF FE DC BA 98 76 54 32 10` xuất hiện **3 lần liên tiếp**
trong `.text` tại RVA `0x29F7B`, `0x2A07A`, `0x2AF07` (mỗi chỗ 20 byte = IV nạp bằng lệnh `mov`).
Không có bảng MD5 K (hằng số nằm rời trong mã) — bình thường với MD5 viết tay.

## 7. Bằng chứng **âm** (rất có giá trị) `[O]`

Không tồn tại trong file: magic `28 B5 2F FD` (**zstd**), `04 22 4D 18` (**LZ4**), `1F 8B 08` (**gzip**),
`BZh` (**bzip2**), `37 7A BC AF` (**7z**), `expand 32-byte k` (**ChaCha20**), `OpenSSL`/`EVP_`,
và **không có** các chuỗi phía Android: `pwd_type`, `pwd_len`, `pwd_md5`, `pwd_sha256`, `header=`,
`md5Checksum=`, `sha256Pass=`, `[ReadHeader]`.
⇒ `[I]` **Định dạng trên PC khác định dạng `com.zing.zdb.backup` của Android** (cùng ý tưởng:
header + MD5 + nén + mã hoá, nhưng **không cùng tên trường**). Không thể lấy layout Android áp thẳng.

## 8. `parseBinNet`: schema protobuf lộ ra trong `.rdata` `[O]`

Chuỗi trường protobuf nằm liên tiếp tại `.rdata 0x103314`–`0x103480` (thứ tự khai báo):
`type, catId, extInfo, childNumber, action, params, title, href, thumb, description, remains,
zinstantData, zinstantMsg, dataSource, data, ext, size, subType, color, property, attach, cliMsgId,
cliMsgType, fromD, globalMsgId, msg, ownerId, ttl, quoteStatus, quote, len, pos, uid, attachs,
reference, mentions`.

`[I]` Đây **trùng khít** schema `Sync2.Message` (`Attach`, `Quote`, `Mention`, `Properties`, `MetaData`)
đã ghi ở `re-notes/15-crypto-sync-protocol.md` §3.4 ⇒ cột `chats.msg`/`attach` trong Cross DB là
**protobuf nhị phân** cùng schema, và `parseBinNet` chính là bộ decode đó. Đây là mảnh còn thiếu của B3.4.

## 9. Layout header — giả thuyết `[I]`, cần disassemble để chốt

Dựa trên thứ tự khai báo chuỗi (trùng thứ tự hàm trong một translation unit `main.cpp`) và các xref:

```
[ header ]
  signature/magic          → "invalid signature"         (0x1BCC0)  độ dài & giá trị [U]
  hash block: len + value  → "invalid hash data length"  (0x1AA00)  [I] MD5 16 B
  nfile (số file)          → "crr_no_file == nfile", "list_files.size(): "
  for each file: nameLen+name ("found file: ") → fileSize int64 ("File size: %lld") → [offset/len: U]
  → "read header fail. error:" / "EOF while restoring"
[ per-file block ] LZMA/XZ decompress ("Res != SZ_OK", "Z_ERROR_DATA") → AES decrypt ("Incorrect key length")
  → so sánh MD5 ("MD5 read from encrypted file", "Md5 not correct !", "checksum mismatch")
[ ghi output ]  V1: 1 file · V2: nhiều file + onProgress(db) → "encode_stream reval: %d"
```

`[I]` `_V2(…, onProgress(db))` + `crr_no_file == nfile` + `found file: ` ⇒ **file backup chứa NHIỀU DB**
(mỗi hội thoại một DB — khớp `Core\Message\<convId>.db` phía PC và `ChatContent` mỗi hội thoại phía Android).
`[U]` Còn thiếu: **magic cụ thể**, kích thước từng field, và thứ tự nén-trước/mã-hoá-sau (thứ tự thông điệp
lỗi cho thấy **nén trước, mã hoá sau**, giống mọi pipeline khác của Zalo).

## 10. Việc tiếp theo (5–8 việc, có địa chỉ cụ thể)

1. **Ghidra/IDA, PE32 i386, base `0x10000000`**: đặt tên 4 hàm theo bảng `0x103334`; đọc bảng
   property-descriptor trong `.data` (`0x124030`, `0x124130`, `0x12418C`, `0x1241B0`, `0x12420C`)
   để **chốt tên ↔ con trỏ** (1–2 h).
2. **Hàm header `0x1AA00`**: đọc `mov`/`cmp` quanh `0x1AA87`, `0x1AC70`, `0x1B0AB`, `0x1B211`
   ⇒ **layout byte chính xác**; lấy hằng magic tại lệnh so sánh của `invalid signature` (`0x1BCC0`) (2–3 h).
3. **Hàm crypto `0x25980`**: tìm nơi dùng S-box `0x1048D8` + Rcon `0x10552D` ⇒ **KDF** từ `password`
   (MD5? SHA-256? bao nhiêu vòng?), **mode** (ECB/CBC/GCM), **padding**, **IV lấy từ đâu** (3–4 h).
4. **Wrapper nén `0x3F5F0` / `0x43350`**: phân biệt `lzma_stream` (liblzma) vs `LzmaDec_DecodeToBuf`
   (7-Zip SDK) bằng hình dạng struct/lời gọi; xác nhận **XZ container** hay **raw LZMA2**; đọc `Props` 5 byte (2 h).
5. **SHA-256 dùng làm gì** (check XZ hay KDF?) — xref `0x44FA0`, `0x45070` (1 h).
6. **Chỉ khi tĩnh bí**: black-box oracle (B4) — cần 1 file backup thật + Node/Electron **ia32** đúng ABI (cần user đồng ý).
7. Viết `decrypt()` bằng Rust khi (2)(3)(4) xong; đơn vị kiểm thử = 1 file backup thật.

## 11. Còn thiếu gì để viết `decrypt()` bằng Rust

1. **Magic + layout header** (độ dài từng field, endianness của `fileSize`).
2. **KDF**: `password` (chuỗi, từ RSA-giải `encrypted_key`) → khoá AES thế nào; có salt/IV trong header không.
3. **Mode + padding** của AES; có phải mỗi file một IV riêng.
4. **Thứ tự chính xác** nén/mã hoá và **ranh giới block** (mỗi file 1 block hay toàn bộ 1 stream).
5. **Cơ chế "signature"** (`invalid signature`) — là magic cố định hay MAC của header.
6. **Nghĩa `format 0` vs `format 1`**: `[I]` khác biệt là 1 file vs nhiều file + biểu diễn khoá (base64/hex),
   nhưng cần xác nhận nhánh rẽ trong `0x00E213` vs `0x00E1A7`.

**Chốt lại B3.3:** đã từ "hợp đồng 4 hàm" lên **bản đồ nhị phân có địa chỉ** — biết chính xác
nén = LZMA/XZ, mã hoá = AES, toàn vẹn = MD5, container nhiều file, và biết **đúng 6 hàm cần đọc**.
Chưa thể viết Rust, nhưng khối lượng còn lại là **8–12 giờ Ghidra có mục tiêu**, không còn là "chưa biết gì".

## 12. Phụ lục — tái lập

Script ngoài repo `%TEMP%\qtzalo_re\`: `pe.py` (header/section/export/import/anchor/hằng số/entropy),
`pe2.py` (delay-import, chuỗi theo thứ tự `.rdata`, hằng số LE, S-box), `pe3.py` (blob hằng số, entropy vùng),
`xref.py` (quét `imm32 = ImageBase+RVA` trong `.text`). **Công cụ:** `dumpbin`/`objdump`/`llvm-objdump`/
`rabin2`/`radare2` **đều không có** trong PATH ⇒ dùng Python thuần (`struct`); **Ghidra chưa cài** — điều kiện
cần cho bước 1–5 của §10. **SHA-256 file:** `0BAE75BB99946A7A59642C07F77961742F8B9CAF23B2E3CE5930EC21F28CFA27`
(giống hệt cả 3 phiên bản Zalo trên máy ⇒ phân tích một lần dùng cho cả ba).

*Hết ghi chú 13.*
