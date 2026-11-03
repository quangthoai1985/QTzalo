# 15 — `db-cross-v4` (B3.3): layout header, KDF, thứ tự nén↔mã hoá, MD5, IV

> **Ngày:** 2026-11-04. **Phạm vi:** phân tích **tĩnh** `db-cross-v4-native.node` (Zalo 26.10.20, `electron_x86`), SHA-256 `0BAE75BB…28CFA27`. **Không chạy mã Zalo, không gọi mạng, không sửa file nào ngoài note này.**
> **Công cụ:** `dumpbin.exe` VS18 (disasm 27,7 MB / 548.740 dòng) + script Python **ngoài repo** ở `%TEMP%\qtzalo_re\`. **Nhãn:** `[O]` đọc trực tiếp · `[I]` suy luận có căn cứ · `[U]` chưa chứng minh. Không chứa khoá/dữ liệu người dùng.

## 0. Kết luận trong một bảng

| Hạng mục | Kết luận | Độ tin |
|---|---|---|
| Layout header | Magic 6 B + `u32 BE A` + `u32 BE xxh32` + `u32 BE nfile` + nfile × (u32 nameLen ≤128, name, u32 size ≤2e9) | `[O]` |
| Hash của header | **XXH32 seed 0** trên `[14, A)` — **không phải MD5**, **không có MAC/chữ ký** | `[O]` |
| KDF (nhánh `_V2`) | **Không biến đổi**: key = 32 byte đầu của `password` đã zero-pad vào buffer 256 B; không MD5/SHA/lặp | `[O]` |
| Cipher + IV | AES-256-CBC; **IV = 16 byte 0 cho TỪNG chunk 64 KiB** (không nối tiếp giữa các chunk; CBC chỉ nối trong chunk) | `[O]` |
| Padding | Chỉ **làm tròn độ dài lên bội số 16** để cấp buffer; **không có bước bỏ padding** ở đường giải mã; ghi ra đúng `inlen` byte | `[O]` |
| Thứ tự | **Giải mã → parse header → giải nén**. V2: `0x1C0C0` rồi `0x1AE00`; V1 chỉ giải nén + MD5 (không AES) | `[O]` |
| MD5 input | **Luồng nén** (INPUT của lzma), băm **sau** decode mỗi vòng; digest lưu ở blob `+4` (vùng 64 B), **32 B đầu XOR với `hex(MD5(password))`** — chi tiết **note 16** | `[O]` |
| Framing payload | Mỗi file là **1 blob**: `[u32][32 B hex XOR][32 B][16 B][16 B][blocks 16 B]`; payload bắt đầu ở offset `A`; header **không** có field đuôi — chi tiết **note 16** | `[O]`/`[I]` |
| `0x29680` | **Dead code** (0 xref: code + `.data`/`.rdata`, cả VA lẫn RVA) | `[O]` |

## 1. Layout header `[O]` (hàm parse `0x1AE00`, hàm mở/giải mã `0x1C0C0`)

Magic nằm trong **plaintext** (sau giải mã), nên `0x1AE00` đọc từ offset 6.

| Offset | Rộng | Endian | Ý nghĩa | Bằng chứng (asm) |
|---|---|---|---|---|
| `0x00` | 6 | — | Magic `"ZDB4.0"` | `cmp dword [edi],3442445Ah` @`0x1C208`; `cmp word [edi+4],302Eh` @`0x1C210`; sai ⇒ `"invalid signature"` @`0x1C2A2` |
| `0x06` | u32 | BE | `A` = tổng dài header `[I]` (bắt buộc ≥ 14) | `fseek(file,6,SEEK_SET)` @`0x1AE45`; `fread(&A,4,1)` @`0x1AE60`; `bswap` @`0x1AE6E`; `add eax,-14` @`0x1AE76`; âm ⇒ `"invalid hash data length"` @`0x1AE86` |
| `0x0A` | u32 | BE | XXH32(seed 0) của `[0x0E, A)` | `fread(&h,4,1)` @`0x1AEB8`; so @`0x1B04A` với hash `0x1AFB6–0x1B048`; khác ⇒ `"checksum mismatch"` @`0x1B075` |
| `0x0E` | u32 | BE | `nfile` = số file `[I]` | `fseek(file,14)` @`0x1B084`; `fread(&nfile,4,1)` @`0x1B09F`; điều kiện dừng @`0x1B3D8` |
| `0x12` | × `nfile` | | bản ghi file: | |
| `+0x00` | u32 | BE | `nameLen` **≤ 0x80** | `fread` @`0x1B11C` (≠4 ⇒ `"EOF while restoring"` @`0x1B422`); `cmp esi,80h` @`0x1B135` |
| `+0x04` | `nameLen` | — | **tên file** (byte thô) | `malloc(nameLen)` @`0x1B142`; `fread` @`0x1B152`; `std::string::assign` @`0x1B16D` |
| `+0x04+nameLen` | u32 | BE | kích thước file `[I]` (≤ **2.000.000.000**) | `fread` @`0x1B18E`; `cmp esi,77359400h` @`0x1B1A7` |

Mỗi bản ghi trong RAM là **struct 32 byte** `{u8 flag=0; u32 size; std::string name(24)}` (push_back vào
`std::vector`: `add [ebp-0x28],20h` @`0x1B2A4`; `mov [eax+4],esi`; `movups [eax+8],xmm0` @`0x1B296–0x1B29D`).
Sau vòng lặp có log `"found file: "` (`0x1B20B`), `"list_files.size(): "` (`0x1B641`), `"start decompress"`
(`0x1B62C`), `"crr_no_file == nfile"` (`0x1B610`) — `[I]` `A` là tổng dài header nên `A−14` byte còn lại vừa được
băm vừa là vùng chứa `nfile` + entries.

## 2. Hash header = XXH32 (seed 0) `[O]`

4 word khởi tạo @`0x1AED3–0x1AEF1` đúng là 4 lane XXH32 với seed = 0:
`v1 = P1+P2 = 0x24234428`, `v2 = P2 = 0x85EBCA77`, `v3 = seed = 0`, `v4 = −P1 = 0x61C8864F`.

```
0001AED3/AEDD: 24234428h, 85EBCA77h         ; v1 = P1+P2, v2 = P2
0001AEE7/AEF1: 0, 61C8864Fh                 ; v3 = seed = 0, v4 = -P1
0001B02B: shr eax,0Fh / xor eax,edx         ; avalanche: h^=h>>15
0001B030: imul ecx,eax,85EBCA77h            ; * PRIME32_2
0001B03D: imul eax,eax,0C2B2AE3Dh           ; * PRIME32_3
0001B045: shr ecx,10h / xor ecx,eax         ; h^=h>>16
0001B04A: cmp dword ptr [ebp+FFFFFF18h],ecx ; so với u32 ở offset 0x0A
```

Băm theo luồng **64 KiB/lần** (`push 10000h` @`0x1AEC8`; `malloc` @`0x1AF07`; `fread` @`0x1AF1B`;
update `0x12DA→0x1A980` @`0x1AF49`/`0x1AF81`) cho tới đủ `A−14` byte. **Hệ quả:** header **không có MAC**;
`"invalid signature"` thực chất là *sai magic*. `re-notes/13` §11 để câu này `[U]` → nay `[O]`.

## 3. KDF + IV `[O]` (nhánh `_V2`)

`0x1C0C0` làm 3 bước: (1) `AES_set_params(ctx, 0x100)` = **AES-256** (`push 100h; lea ecx,[ctx]; call 0x1C5D`
@`0x1C110–0x1C11B`); (2) `memset(buf256,0,0x100)` @`0x1C120–0x1C12E` rồi
`memcpy(buf256, password.data, min(pw_len,0x100))` @`0x1C133–0x1C153`; (3) key expansion đọc **buf256** và copy
đúng **Nk·4 = 32 byte**:

```
00026BA8: push dword ptr [esp+28h]          ; key = buf256 (password zero-pad)
00026BAC: call 10002306                     ; -> 0x27890 key expansion
000278A9: mov esi,[ebx+4]                   ; Nk = 8 (AES-256)
000278B7: lea ecx,[esi*4]                   ; 32
000278E4: cmp edx,eax / jl 0x278D0          ; lặp đúng 32 lần (copy từng byte)
```

Toàn `0x1C0C0` **không** có `call` MD5/SHA, không vòng lặp KDF ⇒ **key = 32 byte đầu của password zero-pad**.
KDF **không băm**. **IV = 0 cho từng chunk** `[O]`: vùng IV `[ebp-0x30]` chỉ bị ghi 2 lần — zero hoá **trước**
vòng lặp — và chỉ được đọc 1 lần mỗi vòng khi push làm tham số:

```
0001C161/0x1C164: xorps xmm0,xmm0 / movups [ebp-30h],xmm0   ; IV = 0 (trước vòng lặp)
0001C1C1:         lea eax,[ebp-30h]                          ; push IV mỗi vòng lặp 0x1C1B0..0x1C24D
```
Quét toàn hàm: **không có lệnh nào khác ghi vào `[ebp-0x30]`** ⇒ chunk 2, 3… **khởi động lại CBC với IV = 0**,
không nối tiếp ciphertext của chunk trước.

## 4. Pad + CBC trong `0x26B50` `[O]`

```
00026B53/B73: div [esi+0Ch] -> paddedLen = ceil(inlen/16)*16; *arg5 = paddedLen
00026BB1: memcpy(ivbuf, *arg4, 16)                 ; ivbuf = IV của caller
00026BD0: push roundKeys / lea edi,[out+off] / add ebx(off) / push edi / push ebx
00026BE1: call aes_decrypt_block(ctx, in+off, out+off, roundKeys)
00026BF2: call xor_block                           ; out[off] ^= ivbuf
00026BFC: call memcpy                              ; ivbuf = in[off]  <- nối ciphertext
00026C01: add ebp,[esi+0Ch] / cmp ebp,[esp+20h] / jb 0x26BD0
```
⇒ CBC **liên tục bên trong** một lần gọi (ivbuf = ciphertext khối trước), nhưng mỗi chunk 64 KiB là một lần gọi
mới với IV = 0. **Không có** bước kiểm/bỏ PKCS#7: vòng lặp chạy tới `paddedLen`, không có nhánh xử lý khối cuối;
caller `0x1C0C0` lại ghi ra đúng `esi` byte (`push esi; push 1; push edi; call fwrite` @`0x1C1F3–0x1C1F7`)
và chỉ kiểm magic ở chunk đầu (`cmp dword [edi],3442445Ah` @`0x1C208`). `[I]` nếu phía mã hoá dùng PKCS#7 thì
chỉ đệm **một lần ở cuối file** (tổng độ dài là bội số 16), các byte đệm bị bỏ qua khi parse header.

## 5. Thứ tự nén ↔ mã hoá — **giải mã trước, giải nén sau** `[O]`

```
_V2 (0x665E→0x14380)                                   [O]
  └─ 0x1BBF0 @0x1466E "open(decrypt+parse)": 0x1C0C0 @0x1BE5D (GIẢI MÃ AES-256-CBC TẠI CHỖ:
       fread→decrypt→fseek(-n)→fwrite) rồi 0x1AE00 @0x1BE86 (PARSE HEADER, bảng §1)
decompressAndDecryptDb / V1 (0x6299→0x13D10)           [O]
  └─ 0x16B80 @0x13F09 → 0x298F0 → 0x28470 @0x29920  -> GIẢI NÉN + MD5
```
Bằng chứng: `0x1BBF0` gọi `0x1C0C0` **trước** (`0x1BE5D`), kiểm `test eax,eax; jns` @`0x1BE65`, rồi mới gọi
`0x1AE00` (`0x1BE86`). **AES chỉ có một cửa vào**: `0x26B50` chỉ được gọi từ `0x1C1D4` ⇒ AES chỉ ở nhánh V2.
`[U]` vì sao V1 vẫn bắt buộc `password` (`"Password can not empty. FAIL"` @`0x28510`) dù không dùng AES —
V1 dùng một lớp **XOR 16 byte** (staging `[esp+0x2C8]`, `0x2B010` + `xorps` @`0x2896B–0x2899E`) chứ không phải AES.

## 6. MD5 băm cái gì `[O]`/`[I]`

Decode (`0x28470`): trong vòng lặp đọc 16 byte/lần (`fread` @`0x288B6`, `0x28B64`) thứ tự là:

```
0x2893F: movups [esp+edi+34Ch],xmm0     ; ghép 16 byte vào buffer INPUT của lzma
0x28983: call 0x2B010                  ; (1) tiến trạng thái khối
0x28A44: call 0x33B80                  ; (2) decode: in=[esp+34Ch+edi] -> out=[esp+10360h]
0x28A71: call 0x3B590                  ; (3) tiêu thụ output
0x28B31/0x28B35: push len / lea eax,[esp+350h]   ; <- slot E+34Ch = buffer INPUT (nén)
0x28B3D/0x28B41: push ctx / call 0x2AF40         ; (4) MD5Update(ctx, input_buf, len)
```
Buffer `E+0x34Ch` chính là buffer được truyền làm **input** cho decode (`lea eax,[esp+358h]; add eax,edi`
@`0x28A28–0x28A31`); buffer **output** `[esp+10360h]` (`0x28A37`) **không** được băm ở đâu.
⇒ `[O]` MD5 **không** băm luồng đã giải nén; nó băm **luồng nén** (đã qua lớp XOR), cập nhật **sau** lời gọi decode
trong cùng vòng. Corroboration phía nén: chuỗi `"MD5 canculate of compress file: %s"` (`0x1054C8`, xref duy nhất
@`0x3AFB8` trong `0x3AC70`) — nơi đó format digest bằng `"%02x"` (`0x3AF9F`) rồi **XOR 0x21 byte** digest với key
(`xorps`/`xor byte` @`0x3AFE0–0x3B022`) trước khi ghi. `[I]` vì vậy checksum = **MD5 của luồng nén**; `[U]` chưa rõ
cách de-obfuscate 32 ký tự hex khi đọc lại (đọc ở `0x28627`, `cmp eax,20h` @`0x2862F`).

## 7. Pseudocode Rust (nhánh V2; chỗ `[U]` phải kiểm bằng file thật)

```rust
const MAGIC: &[u8; 6] = b"ZDB4.0";
fn parse_header(buf: &[u8]) -> Result<(Vec<Entry>, usize)> {          // [O] §1
    if &buf[0..6] != MAGIC { return Err(Error::InvalidSignature); }
    let a = u32::from_be_bytes(buf[6..10].try_into()?);              // [I] tổng dài header
    let expect = u32::from_be_bytes(buf[10..14].try_into()?);
    if xxh32(&buf[14..a as usize], 0) != expect { return Err(Error::ChecksumMismatch); }  // [O] §2
    let nfile = u32::from_be_bytes(buf[14..18].try_into()?);
    let (mut off, mut out) = (18usize, Vec::new());
    for _ in 0..nfile {
        let n = u32::from_be_bytes(buf[off..off + 4].try_into()?) as usize; off += 4;
        if n > 128 { return Err(Error::NameTooLong); }
        let name = String::from_utf8_lossy(&buf[off..off + n]).into_owned(); off += n;
        let size = u32::from_be_bytes(buf[off..off + 4].try_into()?); off += 4;
        if size > 2_000_000_000 { return Err(Error::FileTooLarge); }
        out.push(Entry { name, size });
    }
    Ok((out, off))                                                   // [U] đuôi header sau entries
}
fn derive_key(password: &[u8]) -> [u8; 32] {                         // [O] §3: KHÔNG băm
    let mut b = [0u8; 256];
    let n = password.len().min(256);
    b[..n].copy_from_slice(&password[..n]);
    b[..32].try_into().unwrap()
}
fn decrypt_in_place(file: &mut [u8], password: &[u8]) -> Result<()> { // [O] §3+§4
    let key = derive_key(password);
    for chunk in file.chunks_mut(64 * 1024) {                        // 64 KiB / chunk
        let iv = [0u8; 16];                                          // [O] IV = 0 MỖI chunk
        aes_256_cbc_decrypt(chunk, &key, &iv);                       // CBC nối trong chunk, KHÔNG unpad
    }
    if &file[..6] != MAGIC { return Err(Error::InvalidSignature); }
    Ok(())
}
// V2: decrypt_in_place -> parse_header -> xz_decompress + md5;  V1: [u32][32B hex][16B][16B][16B-blocks]
```

## 8. Đủ để viết Rust chưa?

| Bước cho `decrypt(bytes, password)` | Trạng thái | Ghi chú |
|---|---|---|
| 1. Cắt 64 KiB/chunk, AES-256-CBC, **IV = 0 mỗi chunk** | **ĐỦ** `[O]` | đã có asm `0x26B50` + `0x1C0C0`; không unpad |
| 2. KDF `key = password[..32]` zero-pad | **ĐỦ** `[O]` | không băm, không lặp |
| 3. Parse header (`ZDB4.0`, u32 BE, xxh32 verify, entries) | **ĐỦ** `[O]` | cần crate `xxhash-rust` (xxh32 seed 0) hoặc tự viết |
| 4. Xác định **payload từng file** (vị trí/độ dài) + framing blob | **Một phần** `[O]`/`[I]` | payload ở offset `A`; mỗi file 1 blob `[u32][32B hex XOR][32B][16B][16B][blocks]` (**note 16**); còn `[U]` 32 B phụ |
| 5. Giải nén XZ (liblzma) | **Một phần** `[I]` | container XZ `[I]`; memlimit/flags chưa đọc |
| 6. MD5 verify (luồng nén) | **ĐỦ** `[O]` | digest ở blob `+4`; khoá XOR = `hex(MD5(password))` (32 ASCII, `0x2A050`); so sau decode |
| 7. (V1) lớp XOR 16 byte trước khi giải nén | **THIẾU** `[U]` | `0x2B010` = XOR khối 16 B (BE) với keystream tại `state+0x100`; nguồn keystream chưa đọc |

**Kết luận cho quyết định của Lead:** module **`sync_v1` (chỉ `parse_header` + KDF)** đã **đủ căn cứ để viết ngay**
(bước 2 và 3 `[O]`), có thể tự sinh test vector (tự dựng header + tự tính XXH32, không cần file thật).
Phần `decrypt`/`decompress` **chưa nên viết** vì thiếu bước 4 và 7 — cần 1 file backup thật để đối chiếu.

## 9. Còn thiếu + cách xác minh

`[U]` còn lại (chi tiết + asm: **note 16**): (a) 32 byte phụ của vùng digest blob `+0x24`; (b) ý nghĩa `u32` blob `+0x00`; (c) keystream của `0x2B010` (V1); (d) tham số liblzma; (e) ai tách blob và `size` có phải kích thước blob. **Xác minh bằng file thật:** (1) giải mã AES `key = password[..32]`, IV = 0 theo chunk — 6 byte đầu phải ra `"ZDB4.0"`; (2) nếu sai thử `hex::decode(privateKey)[..32]`, `md5(password)`, `to_uppercase()[..32]`; (3) so `nfile`/entries với log `list_files.size()`; (4) XOR 32 B tại blob `+4` với `hex_lower(md5(password))` rồi so digest; (5) file > 64 KiB: thử chunk 2 với IV = 0 (kỳ vọng đúng) và với 16 byte cuối chunk 1 (kỳ vọng sai).

## 10. Phụ lục — tái lập

`%TEMP%\qtzalo_re\`: `peutil.py` (PE + disasm theo **delta địa chỉ**), `s16_*.py`, `s17_*.py`, `s15_seq.py`, `s15_callers.py`; output `d16_*.txt`, `d17_*.txt`. **Không** suy độ dài lệnh từ cột byte của dumpbin (bị cắt ở 6 byte).

*Hết ghi chú 15.*
