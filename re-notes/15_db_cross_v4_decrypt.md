# 15 — `db-cross-v4` (B3.3): layout header, KDF, thứ tự nén↔mã hoá

> **Ngày:** 2026-11-04. **Phạm vi:** phân tích **tĩnh** `db-cross-v4-native.node` (Zalo 26.10.20, `electron_x86`),
> SHA-256 `0BAE75BB…28CFA27`. **Không chạy mã Zalo, không gọi mạng, không sửa file nào ngoài note này.**
> **Công cụ:** `dumpbin.exe` VS18 (disasm 27,7 MB / 548.740 dòng) + script Python **ngoài repo** ở `%TEMP%\qtzalo_re\`.
> **Nhãn:** `[O]` đọc trực tiếp · `[I]` suy luận có căn cứ · `[U]` chưa chứng minh. Không chứa khoá/dữ liệu người dùng.

## 0. Kết luận trong một bảng

| Hạng mục | Kết luận | Độ tin |
|---|---|---|
| Layout header | Magic 6 B + `u32 BE A` + `u32 BE xxh32` + `u32 BE nfile` + nfile × (u32 nameLen ≤128, name, u32 size ≤2e9) | `[O]` |
| Hash của header | **XXH32 seed 0** trên `[14, A)` — **không phải MD5**, **không có MAC/chữ ký** | `[O]` |
| KDF (nhánh `_V2`) | **Không biến đổi**: key = 32 byte đầu của `password` đã zero-pad vào buffer 256 B; không MD5/SHA/lặp | `[O]` |
| Cipher + IV | AES-256-CBC, **IV = 16 byte 0**, PKCS#7; giải mã **tại chỗ theo chunk 64 KiB** | `[O]`/`[I]` |
| Thứ tự | **Giải mã → parse header → giải nén**. V2: `0x1C0C0` rồi `0x1AE00`; V1 chỉ giải nén + MD5 (không AES) | `[O]` |
| MD5; `0x29680` | MD5 cho **payload** (đọc 32 B hex, băm, format `%02x`, so sánh), **không** cho header. `0x29680` là **dead code** (0 xref, quét cả VA/RVA trong code + `.data`/`.rdata`) | `[O]`/`[I]` |

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
0001AED3: mov [ebp+FFFFFF70h],24234428h     ; v1 = PRIME32_1 + PRIME32_2
0001AEDD: mov [ebp+FFFFFF74h],85EBCA77h     ; v2 = PRIME32_2
0001AEE7: mov [ebp+FFFFFF78h],0             ; v3 = seed = 0
0001AEF1: mov [ebp+FFFFFF7Ch],61C8864Fh     ; v4 = -PRIME32_1
0001B02B: shr eax,0Fh / xor eax,edx         ; avalanche: h^=h>>15
0001B030: imul ecx,eax,85EBCA77h            ; * PRIME32_2
0001B03D: imul eax,eax,0C2B2AE3Dh           ; * PRIME32_3
0001B045: shr ecx,10h / xor ecx,eax         ; h^=h>>16
0001B04A: cmp dword ptr [ebp+FFFFFF18h],ecx ; so với u32 ở offset 0x0A
```

Băm theo luồng **64 KiB/lần** (`push 10000h` @`0x1AEC8`; `malloc` @`0x1AF07`; `fread` @`0x1AF1B`;
update `0x12DA→0x1A980` @`0x1AF49`/`0x1AF81`) cho tới đủ `A−14` byte. **Hệ quả:** header **không có MAC**;
`"invalid signature"` thực chất là *sai magic*. `re-notes/13` §11 để câu này `[U]` → nay `[O]`.

## 3. KDF — **không biến đổi** `[O]` (nhánh `_V2`)

`0x1C0C0` làm 3 bước: (1) `AES_set_params(ctx, 0x100)` = **AES-256** (`push 100h; lea ecx,[ctx]; call 0x1C5D`
@`0x1C110–0x1C11B`, thunk → `0x267A0`); (2) `memset(buf256,0,0x100)` @`0x1C120–0x1C12E` rồi
`memcpy(buf256, password.data, min(pw_len,0x100))` @`0x1C133–0x1C153`; (3) mỗi chunk giải mã gọi
`0x2608 → 0x26B50`, trong đó key expansion đọc **chính buf256** và copy đúng **Nk·4 = 32 byte**:

```
00026BA8: push dword ptr [esp+28h]          ; key = buf256 (password zero-pad)
00026BAC: call 10002306                     ; -> 0x27890 key expansion
000278A9: mov esi,[ebx+4]                   ; Nk = 8 (AES-256)
000278B7: lea ecx,[esi*4]                   ; 32
000278D0: mov al,[ecx+ebp] / mov [ecx-1],al ; copy từng byte
000278E4: cmp edx,eax / jl 0x278D0          ; lặp đúng 32 lần
0001C161: xorps xmm0,xmm0                   ; IV = 0
0001C164: movups [ebp-0x30],xmm0            ; 16 byte IV = 0, rồi push làm tham số
```

Toàn `0x1C0C0` **không** có `call` tới MD5 (`0x29F20/0x2A050/0x2AF00`) hay SHA-256 (`0x453A0/0x45470`),
cũng không có vòng lặp KDF ⇒ **[O] key = 32 byte đầu của password đã zero-pad** (password ngắn ⇒ phần còn lại = 0).
KDF **không băm** — yếu, nhưng đúng asm. `[U]` file > 64 KiB: mỗi chunk dùng **cùng** IV 0 hay `0x26B50`
tự nối chuỗi bên trong (chưa đọc hết vòng CBC `0x26BD0–0x26C0F`).

## 4. Thứ tự nén ↔ mã hoá — **giải mã trước, giải nén sau** `[O]`

```
_V2 (0x665E→0x14380)                                   [O]
  └─ 0x1BBF0 @0x1466E  "open(decrypt + parse)"
       ├─ 0x1C0C0 @0x1BE5D  -> GIẢI MÃ AES-256-CBC TẠI CHỖ
       │     ├─ 0x26B50 @0x1C1D4 -> aes_cbc(key=buf256, IV=0)
       │     └─ kiểm magic "ZDB4.0" TRÊN CHUNK ĐÃ GIẢI MÃ @0x1C208
       │        fseek(file,-n,SEEK_CUR) @0x1C1E8 rồi ghi đè @0x1C1F7  [I] (fwrite theo tham số)
       └─ 0x1AE00 @0x1BE86  -> PARSE HEADER (bảng §1)
decompressAndDecryptDb / V1 (0x6299→0x13D10)           [O]
  └─ 0x16B80 @0x13F09 → 0x298F0 → 0x28470 @0x29920  -> GIẢI NÉN + MD5
```

Bằng chứng thứ tự: `0x1BBF0` gọi `0x1C0C0` (`0x1BE5D`) **trước**, kiểm `test eax,eax; jns` @`0x1BE65`,
rồi mới gọi `0x1AE00` (`0x1BE86`). **AES chỉ có một cửa vào**: `0x26B50` chỉ được gọi từ `0x1C1D4`
(quét toàn `.text`), nên AES **chỉ tồn tại trong nhánh V2** `[O]`. ⇒ **V1 không giải mã**: không có AES
trong chuỗi `0x28470` ở độ sâu 1–2; V1 chỉ mở `"rb"` (`push 10104BF4h` @`0x2853C`), đọc khối MD5, giải nén.
`[U]` vì sao V1 vẫn bắt buộc password (`"Password can not empty. FAIL"` @`0x28510`) mà không dùng cho AES.

**`0x29680` là dead code** `[O]`: 162 lệnh, chứa `"Start decrypt file"`/`"Decryption sucess"`/`"Md5 not correct !"`,
nhưng **0 xref** — đã quét `call` trực tiếp, imm32 dạng VA `0x1029680`, dạng `1029680h`, và `.data`/`.rdata`
cho cả VA lẫn RVA. Vì vậy **không** dùng `0x29680` để suy thứ tự.

## 5. MD5: payload, không phải header `[O]`/`[I]`

Trong `0x28470` (**571 lệnh** — không phải 1.109 như `re-notes/14` ghi):

```
000285CE: call 0x2A050           ; MD5 init (ctx #1)
000285F0: fread 4 byte           ; cmp eax,4  @0x285F8
00028627: fread 32 byte          ; cmp eax,20h @0x2862F  <- 32 ký tự hex MD5
000286A9: push 10104C4Ch         ; "MD5 read from encrypted file: %s\n"
0002889A: call 0x2AF00           ; MD5 init (ctx #2)
000288B6: fread 16 byte          ; cmp eax,10h
00028B41/0x28B9B: call 0x2AF40   ; MD5 update
00028BB0: push 10104CE4h ("%02x"); call 0x28ED0   ; format digest
00028BCB: cmp esi,10h / jl 0x28BB0 ; 16 lần => hex 32 ký tự
00028BD1: call 0x984F4             ; so sánh
```

⇒ `[O]` MD5 **không** tham gia header (header dùng XXH32); MD5 là **toàn vẹn payload**: file chứa sẵn 32 ký tự
hex MD5, chương trình đọc, băm, format `"%02x"`, rồi so sánh. `[I]` dữ liệu băm là **luồng nén** (đọc từng khối
16 B trong lúc giải nén), khớp `"MD5 canculate of compress file"` phía nén; `[U]` chưa chốt băm **nén hay đã giải nén**
— cần đọc `0x286E0–0x28A40` (nơi gọi liblzma `0x2B010`, `0x33990`, `0x33B80`, `0x3B590`).

## 6. Pseudocode Rust (đủ cho nhánh V2; chỗ `[U]` phải kiểm bằng file thật)

```rust
const MAGIC: &[u8; 6] = b"ZDB4.0";
fn parse_header(buf: &[u8]) -> Result<(Vec<Entry>, usize)> {          // [O] §1
    if &buf[0..6] != MAGIC { return Err(Error::InvalidSignature); }
    let a = u32::from_be_bytes(buf[6..10].try_into()?);              // [I] tổng dài header
    let expect = u32::from_be_bytes(buf[10..14].try_into()?);
    if xxh32(&buf[14..a as usize], 0) != expect { return Err(Error::ChecksumMismatch); }  // [O] §2
    let nfile = u32::from_be_bytes(buf[14..18].try_into()?);         // [I] số file
    let (mut off, mut out) = (18usize, Vec::new());
    for _ in 0..nfile {
        let n = u32::from_be_bytes(buf[off..off + 4].try_into()?) as usize; off += 4;
        if n > 128 { return Err(Error::NameTooLong); }               // [O] cmp 80h
        let name = String::from_utf8_lossy(&buf[off..off + n]).into_owned(); off += n;
        let size = u32::from_be_bytes(buf[off..off + 4].try_into()?); off += 4;
        if size > 2_000_000_000 { return Err(Error::FileTooLarge); } // [O] 77359400h
        out.push(Entry { name, size });
    }
    Ok((out, off))                                                   // [U] đuôi header sau entries
}
fn derive_key(password: &[u8]) -> [u8; 32] {                         // [O] §3: KHÔNG băm
    let mut b = [0u8; 256];                                          // memset 0x100
    let n = password.len().min(256);                                 // memcpy min(len,0x100)
    b[..n].copy_from_slice(&password[..n]);
    b[..32].try_into().unwrap()                                      // 32 byte đầu = AES-256 key
}
fn decrypt_in_place(file: &mut [u8], password: &[u8]) -> Result<()> { // [O] §4
    let (key, iv) = (derive_key(password), [0u8; 16]);               // IV = 0   [U] multi-chunk
    for chunk in file.chunks_mut(64 * 1024) {                        // 64 KiB/chunk
        aes_256_cbc_decrypt_inplace(chunk, &key, &iv);               // AES-256-CBC, PKCS#7 unpad
    }
    if &file[..6] != MAGIC { return Err(Error::InvalidSignature); }  // @0x1C208
    Ok(())
}
// V2: decrypt_in_place -> parse_header -> (từng file) xz_decompress + verify_md5
// V1: (không AES) read [u32][32-hex-md5][16B] -> xz_decompress -> MD5 so hex
```

`[U]` còn lại: (a) đuôi header; (b) CBC nhiều chunk có nối IV; (c) framing payload từng file; (d) MD5 băm nén hay đã giải nén.

## 7. Cần sửa trong `re-notes/14` + cách xác minh `[U]`

**Sửa `re-notes/14`:** (1) `0x28470` = **571 lệnh** (không phải 1.109), `0x29680` = **162 lệnh** và là **dead code**
⇒ bỏ khỏi sơ đồ; (2) kết luận (e) "chưa chứng minh" nay **đã chốt**: giải mã → parse → giải nén; (3) hash header là
**XXH32 seed 0**, header **không có MD5**; (4) KDF: **key = 32 byte đầu của password zero-pad, không băm**;
(5) `0x1C0C0` còn **giải mã tại chỗ** toàn file; (6) `0x3F9F0` là phía **ghi** header XZ (nén) — khớp `lzma_stream_header_encode`.

**Xác minh bằng file backup thật (khi người dùng đồng ý):** (1) lấy 1 file `format 1` + `privateKey` (64 hex) từ
`syncmsg_info`, giải mã với `key = password.as_bytes()[..32]`; 6 byte đầu ra `"ZDB4.0"` ⇒ KDF đúng. (2) Nếu sai, thử
`hex::decode(privateKey)[..32]`, `md5(password)`, `password.to_uppercase()[..32]`. (3) So `nfile`/entries với
`list_files.size()` trong log Zalo để chốt `A`; (4) so 32 ký tự hex MD5 với `md5(compressed)` và `md5(decompressed)`
để chốt `[U]` (d); (5) file > 64 KiB: thử chunk 2 với IV = 0 và với 16 byte cuối chunk 1.

## 8. Phụ lục — tái lập

`%TEMP%\qtzalo_re\`: `peutil.py` (PE + disasm theo **delta địa chỉ**), `follow.py`, `s15_events.py`, `s15_seq.py` (dump tuần tự, tự snap ranh giới lệnh), `s15_kdf.py`, `s15_callers.py`, `s15_aesmap.py`, `s15_flow.py`. Lệnh chính: `python s15_seq.py 0x1AE00 0x1B8BB` (header), `… 0x1C0C0 0x1C2D8` (decrypt), `… 0x27890 0x27A80` (key expansion), `python s15_callers.py` (đồ thị gọi). **Không** suy độ dài lệnh từ cột byte của dumpbin (bị cắt ở 6 byte).

*Hết ghi chú 15.*
