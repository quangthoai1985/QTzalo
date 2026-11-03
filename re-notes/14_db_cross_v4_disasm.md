# 14 — `db-cross-v4` (B3.3): giải mã bằng disassembler thật (PE32 i386)

> **Ngày:** 2026-11-03/04. **Phạm vi:** phân tích **tĩnh** `db-cross-v4-native.node` trong
> `%LOCALAPPDATA%\Programs\Zalo\Zalo-26.10.20\resources\app.asar.unpacked\native\nativelibs\db-cross-v4\prebuilt\window\electron_x86\`.
> **Không chạy mã Zalo, không gọi mạng, không sửa file nào ngoài note này.**
> **Công cụ:** `dumpbin.exe` thật của VS Build Tools 18 (`…\VC\Tools\MSVC\14.51.36231\bin\Hostx64\x86\dumpbin.exe`); script Python **ngoài repo** ở `%TEMP%\qtzalo_re\`. Không dùng Ghidra.
> **Nhãn:** `[O]` đọc trực tiếp từ byte/disasm · `[I]` suy luận có căn cứ · `[U]` chưa chứng minh. **Không chứa** khoá/mật khẩu/nội dung tin nhắn/dữ liệu người dùng.

## 0. Kết quả trong một bảng

| Câu hỏi (a–e) | Trả lời | Nhãn |
|---|---|---|
| (a) 4 hàm JS, địa chỉ thật | `getVersion`=`0x119A`, `parseBinNet`=`0x1C35`, `decompressAndDecryptDb`=`0x6299`, `_V2`=`0x665E`; sau thunk: `0x16210`, `0x156F0`, `0x13D10`, `0x14380` | `[O]` |
| (a) 4 con trỏ ở export `0x103334` | **KHÔNG phải** 4 hàm JS — là bảng khởi tạo CRT (1 registrar + 3 dynamic initializer); không lệnh nào tham chiếu | `[O]`/`[I]` |
| (b) Magic header | **`"ZDB4.0"`** (6 byte): `cmp dword [edi],3442445Ah` + `cmp word [edi+4],302Eh`; sai ⇒ `"invalid signature"` | `[O]` |
| (b) Endianness / field | Số nguyên header **big-endian** (`bswap` sau `fread`); field đầu phải ≥ 14; có chặn `2.000.000.000` | `[O]` |
| (b) Băm header | Băm luồng **64 KiB/lần**; có MD5 IV ở vùng crypto; **không** thấy SHA-256 trên đường header | `[O]`/`[U]` |
| (c) Cipher | **AES-256** (`AES_init(ctx,0x100)`), `Nb/Nk/Nr`, S-box `0x1048D8` + inv `0x1049D8`, **PKCS#7** | `[O]` |
| (c) KDF | **Chưa xác định** — `0x1C0C0` **không** gọi MD5/SHA; chỉ copy `password` vào buffer 256 B rồi init AES-256 | `[U]` |
| (d) Nén | **liblzma tĩnh**; bộ ghi **XZ stream header chuẩn** (`FD 37 7A 58 5A 00` + stream flags) ⇒ `[I]` container **XZ**, không phải raw LZMA2 | `[O]`/`[I]` |
| (e) Thứ tự | **Chưa chứng minh**: V1 đi `0x13D10 → 0x16B80 → 0x298F0 → 0x28470`; hàm giải mã `0x29680` **không có call site trực tiếp** | `[U]` |

## 1. Đầu vào & khối lượng đã đọc `[O]`

- File **1.239.552 B**, SHA-256 `0BAE75BB99946A7A59642C07F77961742F8B9CAF23B2E3CE5930EC21F28CFA27` (khớp `re-notes/13`).
- `dumpbin /headers|/exports|/imports`: exit 0, lần lượt 6.568 / 712 / 10.089 byte; `/disasm` exit 0 sau **21,4 s** → `disasm.txt` **27.673.206 B, 548.740 dòng**, parse được **541.381 lệnh** có địa chỉ trong `.text`.
- **Lưu ý kỹ thuật:** dumpbin **cắt cột byte ở 6 byte** với lệnh dài (`C7 04 24 4F 00 00`), nên **không được** suy độ dài lệnh từ số byte in ra — phải lấy **địa chỉ dòng kế tiếp**; nếu không, duyệt tuần tự sẽ lệch và bỏ sót lệnh.

## 2. Đối chiếu `re-notes/13` — khớp gì, lệch gì

**Khớp `[O]`:** PE32 i386, ImageBase `0x10000000`, EP `0x6C62`, 8 section, export `_register_addon_` @RVA `0x103334`; `KERNEL32.dll` **160** hàm + `node.exe` **39** `napi_*` (delay-load, khớp từng slot IAT); S-box `0x1048D8` (`63 7C 77 7B F2 6B 6F C5…`), invS-box `0x1049D8` (`52 09 6A D5…`), XZ magic `0x10586C`, SHA-256 `0x105878` (H0=`6A09E667`), CRC32 `0x105AF8`, MD5 IV tại `0x29F78/0x2A077/0x2AF04`, Rcon `0x10552D` (`04 04 04 08…`).

**Lệch / cần sửa:** (1) **§3 sai** — 4 con trỏ ở `0x103334` (`0x12880`, `0xE120`, `0xE213`, `0xE1A7`) **không phải** 4 hàm JS: `0x12880` chỉ là `push &napi_module; call [napi_module_register]; ret` (4 lệnh), còn `0xE120/0xE1A7/0xE213` là **dynamic initializer C++** (dựng global `0x124030/0x12413x/0x12418C`, gọi helper CRT `0x45DE`, `0x3666`, `0x57316`) — cả 4 là bảng khởi tạo CRT, **không lệnh nào trong `.text` tham chiếu**. (2) **Cột "Xref (RVA)" của §5 là offset của imm32**, không phải địa chỉ lệnh (vd "read header fail" → lệnh thật `push 101043E4h` @`0x1B4AA`, không phải `0x1B0AB`). (3) **§6 "Rcon không thấy imm32"** đúng: bảng có thật ở `0x10552D` nhưng **0 xref tuyệt đối**. (4) **SHA-256** nay rõ vai trò: của **liblzma** (check XZ), không phải KDF — `movups xmm0,[0x10105878]` @`0x453A6`, `add edx,[eax+0x10105898]` @`0x45D91`.

## 3. Bảng hàm / địa chỉ (dựng lại từ disasm; file không có symbol)

| RVA | Vai trò | Bằng chứng |
|---|---|---|
| `0x124000` | `napi_module` | `nm_version=1`, `nm_filename→"…\src\main.cpp"`, `nm_register_func=0x10004CD7`, `nm_modname→"addon"` |
| `0x12880` | `_register_addon_` thật | `push 10124000h; call [1012F19Ch]` (napi_module_register) |
| `0x14CA0`–`0x14E8x` | Đăng ký 4 property | `napi_create_string_utf8` + `napi_set_property` |
| `0x119A`→`0x16210` / `0x1C35`→`0x156F0` | `getVersion` / `parseBinNet` | 8 lệnh (gọi `0x3017`→`0x13220`) / 339 lệnh, 36 call |
| `0x6299`→`0x13D10` / `0x665E`→`0x14380` | `decompressAndDecryptDb` / `_V2` | 391 lệnh/50 call (V1) · 449 lệnh/58 call |
| `0x1AE00` | **Parse header + băm luồng** | `fread`+`bswap`; `invalid hash data length`, `checksum mismatch`, `crr_no_file`, `found file:`, `start decompress` |
| `0x1BBF0` | Điều phối open header | gọi `0x1AE00` @`0x1BE86`, `0x1C0C0` @`0x1BE5D` |
| `0x1C0C0` | **Kiểm magic + init AES-256** | magic `ZDB4.0`; `push 100h` + `AES_init` @`0x1C11B`; `invalid signature` @`0x1C2A2` |
| `0x16B80` → `0x298F0` | Cầu nối V1 → bộ giải nén | gọi `0x298F0` @`0x16BCC`; `0x298F0` cấp 200 B + 4 B rồi gọi `0x28470` @`0x29920` |
| `0x28470` | **En/decode stream (nén+MD5)** | `Password can not empty`, `MD5 read from encrypted file`, `Res != SZ_OK`, `Z_ERROR_DATA`; gọi MD5 2 lần |
| `0x29680` | **Driver giải mã** | `Decrypt input size`, `Start decrypt file`, `Decryption sucess`, `Md5 not correct !`; gọi MD5 wrapper `0x2EF90` @`0x2979E` |
| `0x267A0` | Chọn tham số AES theo **bit khoá** | 128→Nk=4/Nr=10; 192→Nk=6/Nr=12; 256→Nk=8/Nr=14; khác ⇒ `Incorrect key length` |
| `0x275D0` | Tính độ dài đã **pad** | `div [ecx+0Ch]` (block=16), làm tròn lên ⇒ PKCS#7 |
| `0x26E10` / `0x268B0` / `0x27890` | AES **encrypt** / **decrypt** / **key expansion** | S-box; MixColumns `xor dl,[esp+16h]` @`0x26F88…0x26FC4`; invS-box @`0x26977/0x26A10`; SubBytes ×2 + XOR round-key `0x279AB…0x279B8` |
| `0x3F9F0` / `0x3F550` | **Ghi XZ stream header** / wrapper cấp phát liblzma | `mov eax,[1010586Ch]; mov [esi],eax; mov ax,[10105870h]; mov [esi+4],ax` / `call [eax+28h]` + mảng filter 16 B |

## 4. (a) Bốn hàm JS — bằng chứng asm

Chuỗi trong `.rdata`: `getVersion`@`0x104174` (len 10), `parseBinNet`@`0x104184` (11), `decompressAndDecryptDb`@`0x104194` (22), `…_V2`@`0x1041B0` (25). Mỗi chuỗi được tham chiếu **đúng 1 lần**, trong hàm đăng ký `0x14CA0..0x14E8x`. Mẫu lặp (trích `parseBinNet`):

```
00014D45: 68 35 1C 00 10     push 10001C35h        ; <-- callback
00014D4A: FF 74 24 60        push [esp+60h]        ; env
00014D52: 50                 push eax              ; &result
00014D53: E8 AC 04 FF FF     call 10005204         ; -> napi_create_function (thunk 0x12E60)
00014D65: 6A 0B              push 0Bh              ; len("parseBinNet")
00014D67: 68 84 41 10 10     push 10104184h        ; "parseBinNet"
00014D71: FF 15 38 F1 12 10  call [1012F138h]      ; napi_create_string_utf8
00014D95: FF D6              call esi              ; napi_set_property
```

Áp dụng cho cả 4 block ⇒ **`getVersion=0x119A`, `parseBinNet=0x1C35`, `decompressAndDecryptDb=0x6299`, `_V2=0x665E`**, mỗi cái là thunk `jmp` tới thân thật. **Vì sao `re-notes/13` suy sai:** 4 con trỏ ở `0x103334` trông như "bảng 4 hàm", nhưng `0x12880` là registrar và 3 cái kia là initializer toàn cục (bảng khởi tạo CRT kiểu `__xc_a`); vì sao **export** lại trỏ vào **dữ liệu** `.rdata` thì `[U]` — loader không cần giá trị này vì việc đăng ký đã xảy ra khi CRT init chạy `0x12880`.

## 5. (b) Header — magic `[O]`, endianness `[O]`

Hàm `0x1C0C0` đọc khối đầu file rồi kiểm magic (sai ⇒ `invalid signature`, len 17):

```
0001C208: 81 3F 5A 44 42 34        cmp dword ptr [edi],3442445Ah   ; "ZDB4"
0001C20E: 75 76                    jne 1001C286
0001C210: 66 81 7F 04 2E 30        cmp word ptr [edi+4],302Eh      ; ".0"
0001C216: 75 6E                    jne 1001C286
0001C2A0: 6A 11                    push 11h
0001C2A2: 68 78 43 10 10           push 10104378h                  ; "invalid signature"
```

⇒ **Magic = `"ZDB4.0"` (6 byte ASCII)**, không phải MAC (`re-notes/13` §11 để `[U]` → nay `[O]`). Hàm `0x1AE00` đọc bằng `fread` rồi **đảo byte** (header **big-endian**):

```
0001AE60: E8 80 B3 FE FF        call 100061E5           ; fread(&field0,1,4,file)
0001AE6E: 0F C8                 bswap eax               ; <-- big-endian
0001AE76: 83 C0 F2              add eax,0FFFFFFF2h      ; -14
0001AE79: 79 21                 jns 1001AEA2
0001AE86: B8 90 43 10 10        mov eax,10104390h       ; "invalid hash data length"
0001AF3C: 3B C6                 cmp eax,esi             ; con du lieu trong chunk?
```

Sau đó đọc thêm một dword BE (`0x1AEB8`), dựng 4 word trạng thái băm `24234428h / 85EBCA77h / 0 / 61C8864Fh` (`0x1AED3..0x1AEF1`) **[U] thuật toán băm chưa xác định**, rồi băm theo luồng **64 KiB/lần** (`push 10000h` @`0x1AEC8` → `malloc` @`0x1AF07` → `fread` @`0x1AF1B`). Kiểm tra size: `cmp eax,4` (`0x1B124`, `0x1B196`), `cmp esi,80h` (`0x1B135`), **`cmp esi,77359400h` = 2.000.000.000** (`0x1B1A7`). Chuỗi lỗi: `invalid hash data length`, `checksum mismatch`, `crr_no_file == nfile`, `EOF while restoring`, `found file: `, `list_files.size(): `, `start decompress` ⇒ header = **khối hash + số file + danh sách (tên, kích thước)**; độ rộng từng field `[U]`.

## 6. (c) Crypto — AES-256 + PKCS#7 `[O]`; KDF còn `[U]`

`0x1C0C0` gọi `AES_init(ctx, 256)`; `0x267A0` chọn tham số theo **số bit khoá**:

```
000267A0: 8B 44 24 04        mov eax,[esp+4]      ; key bits
000267AA: 2D 80 00 00 00     sub eax,80h          ; 128
000267AF: 74 3E              je  100267EF         ; -> Nk=4, Nr=0Ah (AES-128)
...        (sub 40h)         192 -> Nk=6, Nr=0Ch;  256 -> Nk=8, Nr=0Eh (AES-256)
00026812: C7 44 24 08 40 4B  mov [esp+8],10104B40h   ; "Incorrect key length"
```

Cấu trúc ngữ cảnh: `[ecx]=Nb=4`, `[ecx+4]=Nk`, `[ecx+8]=Nr`, `[ecx+0Ch]=0x10` (block 16 B). S-box chuẩn, dùng **từng byte** (không phải T-table, đúng kiểu build generic ia32): `movzx eax,byte ptr [eax+ecx+101048D8h]` @`0x27F69` (SubBytes), `mov cl,byte ptr [ecx+edx+101049D8h]` @`0x27850` (InvSubBytes). XOR từng byte với khối trước (`xor byte ptr [edx+eax],cl` @`0x26878`, `0x26948`) ⇒ `[I]` chế độ **CBC**; không thấy AES-NI/CTR. **Padding:** `0x275D0` = `((len+block-1)/block)*block` với `block=[ctx+0Ch]=16` ⇒ **PKCS#7** `[O]`.

**Toàn vẹn:** driver `0x29680` gọi MD5 wrapper `0x2EF90` (`call @0x2979E`) rồi so sánh ⇒ `"Md5 not correct !"`; vùng `0x28470` đọc MD5 từ file (`"MD5 read from encrypted file"` @`0x286A9`) và gọi MD5 2 lần (`@0x285CE`, `@0x2889A`).

**KDF `[U]` — điểm chặn chính:** `0x1C0C0` **không gọi MD5/SHA**; nó `memset` buffer 256 B, `memcpy` nội dung `std::string` mật khẩu (`min(len,0x100)`, `0x1C133..0x1C153`) rồi `AES_init(...,256)`. `[I]` nhiều khả năng **khoá = byte của `password` (zero-pad tới 32 B), không băm** — nhưng chưa chứng minh 32 byte nào được nạp vào key schedule (`0x27890`), và chưa rõ nhánh `_V2` có **hex-decode** `privateKey` trước khi dùng.

## 7. (d) Nén — liblzma tĩnh, `[I]` container XZ

- `0x3F9F0` **ghi** XZ stream header ra buffer: 6 byte magic (từ `0x10586C` + `0x105870`) rồi 2 byte stream flags (`mov [edx+1],al` với `al=[ecx+10h]`) ⇒ đúng `lzma_stream_header_encode`.
- `0x3F550` gọi **con trỏ allocator** `call [eax+28h]` và dựng mảng filter **16 B/filter** + terminator `-1` ⇒ đúng hình dạng `lzma_filter[]` + `lzma_allocator` của liblzma.
- Magic chỉ xuất hiện dạng **dữ liệu**: `mov eax,[1010586Ch]` @`0x3FA02`; `mov ecx,1010586Ch` @`0x43757` ⇒ `[I]` decoder là `lzma_stream_decoder` (XZ), không phải raw LZMA2. Không có magic zstd/LZ4/gzip/bzip2/7z (khớp bằng chứng âm của `re-notes/13`).
- `[U]` chưa đọc hết tham số `lzma_stream` (kích thước struct, memlimit, `LZMA_CONCATENATED`).

## 8. (e) Thứ tự giải mã / giải nén — chưa chứng minh

```
adapter 0x13D10 (decompressAndDecryptDb)          [O]
  ├─ 0x16B80 @0x13F09                             (cầu nối)
  │    └─ 0x298F0 @0x16BCC
  │         └─ 0x28470 @0x29920   <- "en/decode stream": giải nén + đọc MD5
  └─ napi_get_buffer_info ×3, napi_create_object, ...
```

Vấn đề: driver giải mã `0x29680` (chứa `"Start decrypt file"`, `"Decryption sucess"`, `"Md5 not correct !"`) **không có call site trực tiếp** trong toàn `.text` và **không** được tham chiếu như dữ liệu (quét `100029680` và `29680h`: 0 hit) ⇒ `[U]` nó được gọi qua đâu (khả năng: con trỏ hàm trong cấu trúc, hoặc nhánh chỉ dùng cho chiều compress). Vì vậy **chưa khẳng định** được "giải nén trước hay giải mã trước" bằng asm; chỉ có `[I]` từ thứ tự chuỗi lỗi (đọc MD5 → kiểm `SZ_OK` → giải mã) rằng **nén trước, mã hoá sau**.

## 9. KDF / cipher / header: đã biết gì

| Hạng mục | Trạng thái | Chi tiết |
|---|---|---|
| Magic | **Biết** `[O]` | `"ZDB4.0"` (6 B) |
| Endianness + field đầu | **Một phần** | big-endian; dword đầu phải ≥ 14 (nếu < ⇒ `invalid hash data length`); ý nghĩa chính xác `[U]` |
| Hash header | **Một phần** `[O]`/`[U]` | băm luồng 64 KiB/lần; 4 word khởi tạo `24234428/85EBCA77/0/61C8864F` — chưa map được thuật toán |
| Cipher + mode + padding | **Biết** `[O]` (mode `[I]`) | AES-256, Nb=4/Nk=8/Nr=14, block 16, CBC, PKCS#7 |
| Nén | **Một phần** `[O]`/`[I]` | liblzma, XZ stream header; tham số stream `[U]` |
| Toàn vẹn | **Biết** `[O]` | MD5 (IV inline 3 chỗ; so với MD5 trong file) |
| KDF | **Chưa biết** `[U]` | không thấy MD5/SHA trên đường mở file; chưa rõ key = password pad hay dẫn xuất |
| Danh sách file | **Hình dạng** `[O]` | `crr_no_file == nfile`, `found file: `, `list_files.size(): `; layout `[U]` |

## 10. Còn thiếu gì + việc tiếp theo

**Còn thiếu để viết `decrypt()` bằng Rust:** (1) KDF chính xác — 32 byte khoá lấy từ đâu, `_V2` có hex-decode `privateKey` không (hiện **không** thấy băm ở `0x1C0C0`); (2) layout header đầy đủ (độ rộng field, vị trí khối hash, cờ nén/mã hoá); (3) thuật toán khối "hash" header; (4) tham số liblzma (`lzma_stream` size, XZ vs raw, `LZMA_CONCATENATED`, memlimit); (5) thứ tự nén/mã hoá + ranh giới block; (6) ai gọi `0x29680` và nhánh rẽ `format 0` vs `1`.

**Việc tiếp theo, theo thứ tự:** (1) đọc trọn `0x28470` + `0x29680` (`h_028470.txt`, `h_029680.txt`; 1.109 + 287 lệnh) để tìm con trỏ gọi `0x29680` trong `.data` và chốt thứ tự nén↔mã hoá; (2) đọc `0x1AE00` từ `0x1AF30` đến `0x1B8BB` lấy **layout header byte**; (3) map khối hash header (truy 4 word `24234428/85EBCA77/0/61C8864F`); (4) đọc `0x27890` xem 32 byte khoá nạp từ offset nào trong ctx; (5) chỉ khi tĩnh bí: oracle black-box với 1 file backup thật + Node/Electron **ia32** đúng ABI (cần người dùng đồng ý).

## 11. Phụ lục — tái lập

`dumpbin` (VS18) `/headers` `/exports` `/imports` `/disasm` → `%TEMP%\qtzalo_re\*.txt`. Script ngoài repo: `peutil.py` (PE + IAT + parse disasm **theo delta địa chỉ**), `b33_pe.py`, `q3.py`–`q10.py`, `dump.py`, `dumpf.py`, `follow.py`. Lệnh nhanh: `python follow.py` (4 thân hàm), `python q5.py` (xref chuỗi), `python q7.py` (tóm tắt call/cmp hàm lớn).
**Cảnh báo cho phiên sau:** đừng suy độ dài lệnh từ cột byte của dumpbin (bị cắt ở 6 byte).

*Hết ghi chú 14.*
