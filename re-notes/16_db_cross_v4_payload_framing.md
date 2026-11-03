# 16 — `db-cross-v4` (tiếp 15): đuôi header, framing payload, MD5 hex, lớp XOR của V1

> **Ngày:** 2026-11-04. **Phạm vi:** phân tích **tĩnh** `db-cross-v4-native.node` (Zalo 26.10.20, `electron_x86`),
> SHA-256 `0BAE75BB…28CFA27`. **Không chạy mã Zalo, không gọi mạng, không sửa file nào ngoài note này.**
> **Dải địa chỉ đã đọc thêm trong đợt này:** `0x1AE00–0x1B8C0` (đuôi header), `0x28470–0x28800` (đầu decode),
> `0x28640–0x28730`, `0x3AC70–0x3B0A0` (phía nén), `0x2A050–0x2A102`, `0x2B010–0x2B0C0`.
> **Nhãn:** `[O]` đọc trực tiếp · `[I]` suy luận có căn cứ · `[U]` chưa chứng minh. Không chứa khoá/dữ liệu người dùng.

## 0. Kết luận nhanh

| Câu hỏi | Kết luận | Độ tin |
|---|---|---|
| (a) Đuôi header sau entries | **Không có field nào thêm**; header kết thúc ngay sau entry cuối, `A` (offset 6) là mốc hết header | `[O]` |
| (b) Payload bắt đầu ở đâu | Ở **offset `A`** (sau header); `0x1AE00` **không** đọc payload, cũng không kiểm `bytes_consumed == A` | `[I]`/`[O]` |
| (b) Framing mỗi file | Mỗi file là **một blob riêng** (V1 `0x28470` mở `"rb"` từ offset 0 của blob): `[u32][32 B hex MD5 đã XOR][32 B][16 B][16 B][16-byte blocks…]` | `[O]`/`[I]` |
| (c) Vị trí MD5 hex | **blob + 4**, vùng 64 B; **32 B đầu** bị XOR với **32 ký tự hex của `MD5(password)`** | `[O]` |
| (c) Khoá XOR | `0x2A050` = `md5_hex_32(password)` → trả con trỏ tới buffer global `0x1012C700` (32 ký tự ASCII) | `[O]` |
| (e) Lớp 16 B của V1 | `0x2B010` XOR khối 16 B (đọc **big-endian**) với keystream 16 B tại `state+0x100`, rồi `+0x10` con trỏ khoá | `[I]`, chi tiết còn `[U]` |

## 1. (a) Đuôi header — không có field nào `[O]`

Toàn bộ các lệnh đọc file trong `0x1AE00` (đã liệt kê hết, không còn chỗ khác):

| Địa chỉ | Đọc gì | Vị trí |
|---|---|---|
| `0x1AE45`/`0x1AE60` | `fseek(6)` + `fread(&A,4,1)` | offset 6 (field `A`) |
| `0x1AEB8` | `fread(&hash,4,1)` | offset `0x0A` |
| `0x1AF1B` | `fread(buf,1,0x10000)` trong vòng băm | vùng `[0x0E, A)` |
| `0x1B09F` | `fread(&nfile,4,1)` | offset `0x0E` |
| `0x1B11C`/`0x1B152` | `fread(&nameLen,4,1)` + `fread(name,nameLen,1)` | entries |
| `0x1B18E` | `fread(&size,4,1)` | entries |

Sau entry cuối (`0x1B3D8: cmp ecx,eax / jae 0x1B62C`) hàm chỉ: log `"crr_no_file == nfile"` (`0x1B610`),
`"start decompress"` (`0x1B62C`), `"list_files.size(): "` (`0x1B641`), tính số entry (`sar eax,5` = `/32`),
tạo/join **thread** (`0x24410`, `0x9994A`, `0x25360`, `0x25EA0`, `0x25A00`) và free vector
(`0x1B83F–0x1B88F`). Kết quả thread ≠ 0 ⇒ `"decompress fail"` (`0x104458`, 15 ký tự).
⇒ **Không còn `fread` nào sau entries** ⇒ header = `magic(6) + A(4) + hash(4) + nfile(4) + entries`.
`[O]` parser **không** kiểm `bytes_consumed == A` ⇒ nếu file thật có byte đệm giữa entry cuối và `A`,
phần đó bị bỏ qua nhưng **vẫn nằm trong vùng băm**. `[I]` payload vì vậy bắt đầu ở `A`.

## 2. (b) Framing payload — mỗi file là một blob riêng `[O]`/`[I]`

`0x28470` (đường V1) mở đúng **một** file bằng `"rb"` rồi đọc **tuần tự từ offset 0**:

```
0002853C: push 10104BF4h ("rb") / push <path>
0002854C: call 0x29D70 (fopen)          ; mở file input
0002858B: push 4 / call malloc          ; buffer 4 B
000285C6/0x285CE: push <local> / push <local> / call 0x2A050   ; md5_hex_32(password)
000285D3: push 41h / call malloc        ; buffer 65 B cho digest hex
000285F0: fread(buf4, 1, 4, file)       ; +0 : u32
00028627: fread(hex, 2, 20h, file)      ; +4 : 64 byte (32 ký tự hex lưu dạng 2-byte)
000286C0: fread(buf16a, 1, 10h, file)   ; +0x44 : 16 byte
00028700: fread(buf16b, 1, 10h, file)   ; +0x54 : 16 byte
000288B6/0x28B64: fread(blk, 1, 10h)    ; +0x64 … : các khối 16 byte của luồng nén
```

| Offset blob | Rộng | Ý nghĩa | Độ tin |
|---|---|---|---|
| `+0x00` | u32 | cờ/độ dài (đọc vào `out`-param của caller; nếu âm ⇒ **bỏ qua** bước MD5, `cmp [ebp],0 / jl` @`0x285E1`) | `[I]` |
| `+0x04` | **32 B** | **hex MD5 (obfuscated)** — 32 byte đầu của vùng 64 B | `[O]` |
| `+0x24` | 32 B | phần còn lại của vùng 64 B, **không** bị XOR, chưa rõ dùng gì | `[U]` |
| `+0x44` | 16 B | khối 16 B đầu (đệm/staging cho lớp XOR) | `[O]` |
| `+0x54` | 16 B | khối 16 B thứ hai (nạp staging ban đầu) | `[O]` |
| `+0x64` | 16 B × n | các khối dữ liệu nén, đọc 16 B mỗi vòng | `[O]` |

`[I]` đây **không** phải "một luồng XZ nhiều member": mỗi lần gọi chỉ xử lý **một** blob; việc tách blob
theo entry (dùng `size` ở header) nằm ngoài hàm này (phía JS hoặc bước trước). `size` không được đối chiếu
trong `0x1AE00` lẫn `0x28470` (`[O]`: không có `cmp` nào so `size` với vị trí file).

## 3. (c) MD5 hex: vị trí + cách de-obfuscate `[O]`

Ngay sau khi đọc 64 B, `0x28470` de-obfuscate **32 byte đầu** bằng XOR với khoá 32 byte:

```
0002864E: mov byte ptr [esi+40h],0      ; NUL-terminate chuỗi 64 ký tự
0002865D: mov edi,ebx / sub edi,esi     ; src = ebx (khoá), dst = esi (hex)
00028670: mov al,byte ptr [ecx+edi]     ; al = key[i]
00028676: xor byte ptr [ecx-1],al       ; hex[i] ^= key[i]
0002867C: jne 0x28670                   ; đúng 32 lần (edx = 20h)
; nhánh SSE cho vùng chồng lấn: xorps xmm1,xmm0 @0x2869C, 2 lần × 16 B
```

**Khoá là gì** `[O]`: `ebx` = giá trị trả về của `0x2A050`, và hàm này **trả về `0x1012C700`**:

```
0002A065: push 98h / call malloc        ; ctx MD5 (0x98 byte)
0002A077: mov [ebx+8],67452301h …       ; IV MD5
0002A0A0/0x2A0AB: MD5 update + final    ; băm chuỗi password (2 tham số)
0002A0C0: movzx eax,[esp+edi+0Ch] / push 10104CE4h ("%02x") / call sprintf
0002A0DA: cmp esi,1012C720h / jl 0x2A0C0   ; ghi 16 byte → 32 ký tự hex
0002A0EF: mov eax,1012C700h             ; <-- trả về buffer hex toàn cục
```

⇒ **khoá XOR = 32 ký tự hex (chữ thường) của `MD5(password)`**; vùng nhớ `0x1012C700–0x1012C720`.
Phía nén đối xứng `[O]`: sau khi format MD5 của luồng nén thành 32 ký tự hex, `0x3AC70` XOR **0x21 byte**
(32 ký tự + NUL) với cùng khoá (`xorps`/`xor byte` @`0x3AFE0–0x3B022`), rồi ghi **0x40 = 64 byte** bằng
`0x3B590` (`mov dword [esp+20h],40h` @`0x3B04C`, `cmp eax,40h` @`0x3B06D`).
`[U]` 32 byte thứ hai của vùng 64 B (`+0x24`) — không bị XOR ở cả hai phía, có thể là đệm hoặc digest phụ.

Sau khi de-obfuscate, log `"MD5 read from encrypted file: %s"` (`0x286A9`), và digest được so với MD5 tính
trên luồng nén ở cuối (`md5_update` @`0x28B41` + `"%02x"` @`0x28BB9` + so sánh @`0x28BD1`).

## 4. (e) Lớp 16 byte của V1 — một phần `[I]`, chi tiết `[U]`

`0x2B010` nhận một khối 16 B, đọc **big-endian** thành 4 word rồi XOR với 4 word **keystream** tại
`state+0x100`, sau đó tăng con trỏ khoá thêm `0x10`:

```
0002B01F: movzx edx,[ecx] / shl edx,8 / or edx,eax …   ; ghép 4 byte -> u32 BE
0002B048: xor edx,dword ptr [ebp+00000100h]            ; ^ keystream word 0
0002B077: xor edx,dword ptr [esi+4]                    ; ^ word 1
0002B09F: xor edx,dword ptr [esi+8]                    ; ^ word 2
0002B0B4: xor ebx,dword ptr [esi+0Ch]                  ; ^ word 3
0002B0B7: add esi,10h                                  ; keystream += 16 B
```

`[I]` đây là **XOR với keystream 16 B** (không thấy S-box AES `0x1048D8`, không thấy hằng MD5/SHA);
phần còn lại của hàm `0x2B010` (sau `0x2B0C0`) và nguồn keystream tại `state+0x100` **chưa đọc** ⇒ **`[U]`**:
chưa xác định được keystream sinh từ đâu (16 B đọc ở `+0x44`/`+0x54`? từ `MD5(password)`? từ bộ đếm?).

## 5. Cập nhật pseudocode `decrypt()` (bước nào đã rõ)

```rust
// B1. Tách 64 KiB/chunk -> AES-256-CBC, IV = 0 mỗi chunk            [O]  §3/§4 của note 15
// B2. KDF key = password[..32] (zero-pad)                          [O]
// B3. parse_header: ZDB4.0, A (u32 BE), xxh32(seed 0) verify,
//     nfile, entries{nameLen<=128, name, size}                      [O]
// B4. payload_start = A                                             [I]
//     tách blob theo entry: dùng `size` (đơn vị byte của blob)       [I]
// B5. với mỗi blob: đọc [u32][32B hex XOR][32B][16B][16B][blocks 16B]  [O]
//     key = hex_lower(MD5(password))  (32 ASCII)                    [O]
//     hex[0..32] ^= key                                             [O]
//     lớp XOR 16 byte của V1 (0x2B010 + keystream state+0x100)       [U]
//     XZ decompress các khối sau đó                                  [I]
//     MD5(luồng nén) == hex đã de-obfuscate                          [O]/[I]
```

## 6. Còn thiếu (sau đợt này)

(a) 32 byte thứ hai của vùng digest `+0x24`; (b) đơn vị/ý nghĩa của `u32` ở `+0x00` và việc "nếu âm thì
bỏ qua MD5"; (c) keystream của `0x2B010` (đọc tiếp `0x2B0C0–0x2B2xx` + `state+0x100` được khởi tạo ở đâu);
(d) tham số liblzma; (e) ai tách blob (JS hay C++) và `size` có phải kích thước blob không.
**Cách xác minh bằng file thật:** lấy 1 file `format 1`, giải mã AES (B1–B2) → kiểm `ZDB4.0`; đọc `A`,
`nfile`, entries; cắt blob đầu theo `size`; tính `hex_lower(md5(password))` rồi XOR 32 byte tại `+4`;
so với digest tính trên luồng nén; thử XZ-decompress phần sau `+0x64`.

## 7. Phụ lục — tái lập

Script: `s17_frames.py`, `s17_hex.py`, `s17_wr.py`, `s17_b010.py`, `s17_gap.py` (trong `%TEMP%\qtzalo_re\`);
output `d17_frames.txt`, `d17_hex.txt`, `d17_wr.txt`, `d17_b010.txt`, `d17_gap.txt`.
**Không** suy độ dài lệnh từ cột byte của dumpbin (bị cắt ở 6 byte).

*Hết ghi chú 16.*
