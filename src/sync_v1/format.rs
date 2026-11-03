//! `db-cross-v4` container format (Part B / Sync V1, step B3.3).
//!
//! Everything here is derived from static disassembly of Zalo PC's native addon
//! `db-cross-v4-native.node` (PE32 ia32). Evidence, addresses and asm excerpts:
//! `re-notes/15_db_cross_v4_decrypt.md` (04/11/2026). Summary:
//!
//! ```text
//! 0x00  6   magic "ZDB4.0"
//! 0x06  4   u32 BE  header_len (payload starts here; must be >= 14)
//! 0x0A  4   u32 BE  XXH32(bytes[0x0E .. header_len], seed 0)
//! 0x0E  4   u32 BE  nfile
//! 0x12  ..  nfile x { u32 BE name_len (<= 0x80), name bytes, u32 BE size (<= 2_000_000_000) }
//! ```
//!
//! Decryption (per the same note): AES-256-CBC, **16 zero bytes as IV**, PKCS#7 padding, the whole
//! stream decrypted in place in 64 KiB chunks; the key is the **first 32 bytes of the password
//! zero-padded to 256 bytes** (no hash, no KDF). V1 (`decompressAndDecryptDb`) only decompresses and
//! verifies MD5; V2 (`_V2`) is the encrypted one.
//!
//! **Status:** header parsing, key derivation and the AES-CBC primitive are implemented and tested;
//! XZ decompression and the restore pipeline are **not** implemented — they need a real backup sample
//! to validate against (`re-notes/15` §7). This module is deliberately not wired into the client yet.

/// Magic of a decrypted cross-DB container.
pub const MAGIC: &[u8; 6] = b"ZDB4.0";

/// Longest accepted entry name, from `cmp esi, 80h` in the header reader.
pub const MAX_NAME_LEN: usize = 0x80;

/// Largest accepted entry size, from `cmp esi, 77359400h` (2_000_000_000).
pub const MAX_FILE_SIZE: u32 = 2_000_000_000;

/// One row of the container's file table (one database per conversation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossDbFile {
    pub name: String,
    pub size: u32,
}

/// Parsed header of a decrypted cross-DB container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossDbHeader {
    /// Value of the `header_len` field: also the offset where the payload begins.
    pub header_len: usize,
    /// Stored XXH32 of `bytes[0x0E..header_len]`.
    pub hash: u32,
    pub files: Vec<CrossDbFile>,
}

/// XXH32 with seed `0` — the checksum Zalo stores in the header.
pub fn xxh32(data: &[u8]) -> u32 {
    xxh32_seed(data, 0)
}

/// XXH32 with an explicit seed (only seed `0` is used by the container).
pub fn xxh32_seed(data: &[u8], seed: u32) -> u32 {
    const P1: u32 = 0x9E37_79B1;
    const P2: u32 = 0x85EB_CA77;
    const P3: u32 = 0xC2B2_AE3D;
    const P4: u32 = 0x27D4_EB2F;
    const P5: u32 = 0x1656_67B1;

    fn round(acc: u32, input: u32) -> u32 {
        const P1: u32 = 0x9E37_79B1;
        const P2: u32 = 0x85EB_CA77;
        acc.wrapping_add(input.wrapping_mul(P2))
            .rotate_left(13)
            .wrapping_mul(P1)
    }

    let len = data.len();
    let mut idx = 0usize;
    let mut h: u32;
    if len >= 16 {
        let mut v1 = seed.wrapping_add(P1).wrapping_add(P2);
        let mut v2 = seed.wrapping_add(P2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(P1);
        while idx + 16 <= len {
            v1 = round(v1, le_u32(&data[idx..]));
            v2 = round(v2, le_u32(&data[idx + 4..]));
            v3 = round(v3, le_u32(&data[idx + 8..]));
            v4 = round(v4, le_u32(&data[idx + 12..]));
            idx += 16;
        }
        h = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
    } else {
        h = seed.wrapping_add(P5);
    }
    h = h.wrapping_add(len as u32);
    while idx + 4 <= len {
        h = h.wrapping_add(le_u32(&data[idx..]).wrapping_mul(P3));
        h = h.rotate_left(17).wrapping_mul(P4);
        idx += 4;
    }
    while idx < len {
        h = h.wrapping_add((data[idx] as u32).wrapping_mul(P5));
        h = h.rotate_left(11).wrapping_mul(P1);
        idx += 1;
    }
    h ^= h >> 15;
    h = h.wrapping_mul(P2);
    h ^= h >> 13;
    h = h.wrapping_mul(P3);
    h ^= h >> 16;
    h
}

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn be_u32(bytes: &[u8], off: usize) -> Result<u32, String> {
    let end = off.checked_add(4).ok_or("độ dài header tràn")?;
    let slice = bytes
        .get(off..end)
        .ok_or("header cross DB bị cắt (thiếu trường u32)")?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Parse and validate the header of an already-decrypted container.
///
/// Errors are user-facing Vietnamese strings. The parser is strict about the magic, the header
/// length field, the XXH32 checksum, the entry-name limit and the entry-size limit, and it never
/// reads outside `bytes`.
pub fn parse_header(bytes: &[u8]) -> Result<CrossDbHeader, String> {
    if bytes.len() < 14 {
        return Err("File cross DB quá ngắn để chứa header.".into());
    }
    if &bytes[0..6] != MAGIC {
        return Err("Không phải file cross DB (sai chữ ký ZDB4.0).".into());
    }
    let header_len = be_u32(bytes, 6)? as usize;
    let hash = be_u32(bytes, 10)?;
    if header_len < 14 || header_len > bytes.len() {
        return Err(format!(
            "Độ dài header cross DB không hợp lệ ({header_len} trên {} byte).",
            bytes.len()
        ));
    }
    let computed = xxh32(&bytes[14..header_len]);
    if computed != hash {
        return Err(format!(
            "Header cross DB hỏng (XXH32 {computed:08x} khác {hash:08x})."
        ));
    }
    let nfile = be_u32(bytes, 14)? as usize;
    let mut off = 18usize;
    let mut files = Vec::with_capacity(nfile.min(1024));
    for _ in 0..nfile {
        let name_len = be_u32(bytes, off)? as usize;
        if name_len > MAX_NAME_LEN {
            return Err(format!("Tên tệp trong cross DB quá dài ({name_len} byte)."));
        }
        off += 4;
        let name_end = off
            .checked_add(name_len)
            .ok_or("độ dài tên tệp tràn")?;
        let name_bytes = bytes
            .get(off..name_end)
            .ok_or("header cross DB bị cắt (thiếu tên tệp)")?;
        if name_end > header_len {
            return Err("Tên tệp vượt quá độ dài header.".into());
        }
        // [I] The container is written by a C++ addon on Windows; names are UTF-8 in practice.
        // Decode lossily so a single bad byte cannot make the whole backup unreadable.
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        off = name_end;
        let size = be_u32(bytes, off)?;
        if size > MAX_FILE_SIZE {
            return Err(format!("Tệp trong cross DB quá lớn ({size} byte)."));
        }
        off += 4;
        if off > header_len {
            return Err("Header cross DB bị cắt (vượt quá độ dài header).".into());
        }
        files.push(CrossDbFile { name, size });
    }
    Ok(CrossDbHeader {
        header_len,
        hash,
        files,
    })
}

/// AES-256 key from the container password.
///
/// [O] `0x1C0C0` zeroes a 256-byte buffer, copies `min(len(password), 256)` bytes into it and hands
/// the first 32 bytes to the AES-256 key schedule: no hash, no salt, no iteration count.
pub fn derive_key(password: &[u8]) -> [u8; 32] {
    let mut buf = [0u8; 256];
    let n = password.len().min(buf.len());
    buf[..n].copy_from_slice(&password[..n]);
    let mut key = [0u8; 32];
    key.copy_from_slice(&buf[..32]);
    key
}

/// Decrypt one AES-256-CBC buffer **without** removing any padding.
///
/// [O] The native reader rounds the length up to a multiple of 16 only to size the buffer and then
/// writes exactly the byte count it was given (`fwrite` at `0x1C1F7`); no PKCS#7 strip loop was
/// found in `0x26B50`. Use the header's `header_len` (and, later, the payload framing) to know where
/// the plaintext ends. See `re-notes/15` §4.
pub fn decrypt_cbc_blocks(
    key: &[u8; 32],
    iv: &[u8; 16],
    ciphertext: &[u8],
) -> Result<Vec<u8>, String> {
    use aes::cipher::block_padding::NoPadding;
    use aes::cipher::{BlockDecryptMut, KeyIvInit};

    if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return Err(format!(
            "Độ dài dữ liệu mã hoá không hợp lệ ({} byte, phải bội số của 16).",
            ciphertext.len()
        ));
    }
    let mut buf = ciphertext.to_vec();
    let dec = cbc::Decryptor::<aes::Aes256>::new(key.into(), iv.into());
    let plain = dec
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .map_err(|e| format!("Giải mã cross DB thất bại: {e}"))?;
    Ok(plain.to_vec())
}

/// Decrypt one AES-256-CBC buffer and strip PKCS#7 padding.
///
/// Kept for the case where a real sample turns out to use padding after all ([I] — see
/// `re-notes/15` §4); the native code path does not strip padding itself.
pub fn decrypt_cbc_pkcs7(
    key: &[u8; 32],
    iv: &[u8; 16],
    ciphertext: &[u8],
) -> Result<Vec<u8>, String> {
    use aes::cipher::block_padding::Pkcs7;
    use aes::cipher::{BlockDecryptMut, KeyIvInit};

    if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return Err(format!(
            "Độ dài dữ liệu mã hoá không hợp lệ ({} byte, phải bội số của 16).",
            ciphertext.len()
        ));
    }
    let mut buf = ciphertext.to_vec();
    let dec = cbc::Decryptor::<aes::Aes256>::new(key.into(), iv.into());
    let plain = dec
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| format!("Giải mã cross DB thất bại (đệm không hợp lệ): {e}"))?;
    Ok(plain.to_vec())
}

/// Decrypt with the container rules: key from the password, all-zero IV, **no padding strip**.
///
/// `ciphertext` is only one decryption unit; the real file is decrypted **in place in 64 KiB
/// chunks**, each chunk starting from IV 0 (`re-notes/15` §4).
pub fn decrypt_with_password(password: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String> {
    let key = derive_key(password);
    decrypt_cbc_blocks(&key, &[0u8; 16], ciphertext)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known-answer vectors computed with `python-xxhash` (independent implementation), seed 0.
    #[test]
    fn xxh32_matches_the_reference_vectors() {
        assert_eq!(xxh32(b""), 0x02CC_5D05);
        assert_eq!(xxh32(b"a"), 0x550D_7456);
        assert_eq!(xxh32(b"abc"), 0x32D1_53FF);
        assert_eq!(xxh32(b"Hello, world!"), 0x31B7_405D);
        assert_eq!(xxh32(b"ZDB4.0"), 0xD652_A392);
        assert_eq!(xxh32(b"0123456789abcdef"), 0xC2C4_5B69);
        assert_eq!(xxh32(b"0123456789abcdefg"), 0xCC79_B217);
        let sixteen: Vec<u8> = (0u8..16).collect();
        assert_eq!(xxh32(&sixteen), 0xB728_37F4);
        let sixty_four: Vec<u8> = (0u8..64).collect();
        assert_eq!(xxh32(&sixty_four), 0x3112_0435);
    }

    #[test]
    fn key_is_the_first_32_password_bytes_zero_padded() {
        let short = derive_key(b"abc");
        assert_eq!(&short[..3], b"abc");
        assert!(short[3..].iter().all(|b| *b == 0));

        let exact = [7u8; 32];
        assert_eq!(derive_key(&exact), exact);

        let long: Vec<u8> = (0u8..40).collect();
        assert_eq!(derive_key(&long), {
            let mut k = [0u8; 32];
            k.copy_from_slice(&long[..32]);
            k
        });

        // A password longer than the 256-byte buffer is truncated before the key is taken.
        let huge: Vec<u8> = (0..300u32).map(|i| (i % 251) as u8).collect();
        let mut expect = [0u8; 32];
        expect.copy_from_slice(&huge[..32]);
        assert_eq!(derive_key(&huge), expect);

        assert_eq!(derive_key(b""), [0u8; 32]);
    }

    fn build_header(files: &[(&str, u32)]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&(files.len() as u32).to_be_bytes());
        for (name, size) in files {
            body.extend_from_slice(&(name.len() as u32).to_be_bytes());
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(&size.to_be_bytes());
        }
        let header_len = 14 + body.len();
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(header_len as u32).to_be_bytes());
        out.extend_from_slice(&xxh32(&body).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn header_round_trips_and_exposes_the_file_table() {
        let bytes = build_header(&[("1234567.db", 4096), ("group_89.db", 12)]);
        let h = parse_header(&bytes).unwrap();
        assert_eq!(h.header_len, bytes.len());
        assert_eq!(h.files.len(), 2);
        assert_eq!(h.files[0], CrossDbFile { name: "1234567.db".into(), size: 4096 });
        assert_eq!(h.files[1], CrossDbFile { name: "group_89.db".into(), size: 12 });
        // Empty table is valid too.
        let empty = build_header(&[]);
        let h = parse_header(&empty).unwrap();
        assert!(h.files.is_empty());
        assert_eq!(h.header_len, 18);
    }

    #[test]
    fn header_rejects_bad_magic_length_hash_and_limits() {
        let good = build_header(&[("a.db", 1)]);

        let mut bad_magic = good.clone();
        bad_magic[0] = b'X';
        assert!(parse_header(&bad_magic).unwrap_err().contains("ZDB4.0"));

        let mut bad_len = good.clone();
        bad_len[6..10].copy_from_slice(&(10u32).to_be_bytes());
        assert!(parse_header(&bad_len).unwrap_err().contains("Độ dài header"));

        let mut too_long = good.clone();
        too_long[6..10].copy_from_slice(&(9999u32).to_be_bytes());
        assert!(parse_header(&too_long).unwrap_err().contains("Độ dài header"));

        let mut bad_hash = good.clone();
        bad_hash[10] ^= 0xFF;
        assert!(parse_header(&bad_hash).unwrap_err().contains("XXH32"));

        // Name longer than 0x80 bytes.
        let mut long_name = Vec::new();
        let body: Vec<u8> = {
            let mut b = Vec::new();
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&((MAX_NAME_LEN + 1) as u32).to_be_bytes());
            b.extend_from_slice(&vec![b'a'; MAX_NAME_LEN + 1]);
            b.extend_from_slice(&1u32.to_be_bytes());
            b
        };
        long_name.extend_from_slice(MAGIC);
        long_name.extend_from_slice(&((14 + body.len()) as u32).to_be_bytes());
        long_name.extend_from_slice(&xxh32(&body).to_be_bytes());
        long_name.extend_from_slice(&body);
        assert!(parse_header(&long_name).unwrap_err().contains("quá dài"));

        // Size above 2_000_000_000.
        let mut huge = Vec::new();
        let body: Vec<u8> = {
            let mut b = Vec::new();
            b.extend_from_slice(&1u32.to_be_bytes());
            b.extend_from_slice(&2u32.to_be_bytes());
            b.extend_from_slice(b"a.");
            b.extend_from_slice(&(MAX_FILE_SIZE + 1).to_be_bytes());
            b
        };
        huge.extend_from_slice(MAGIC);
        huge.extend_from_slice(&((14 + body.len()) as u32).to_be_bytes());
        huge.extend_from_slice(&xxh32(&body).to_be_bytes());
        huge.extend_from_slice(&body);
        assert!(parse_header(&huge).unwrap_err().contains("quá lớn"));

        // Truncated files never panic.
        assert!(parse_header(&good[..10]).is_err());
        assert!(parse_header(&good[..good.len() - 1]).is_err());
    }

    /// AES-256-CBC known-answer vector from NIST SP 800-38A F.2.6 (cross-checked with `openssl enc
    /// -aes-256-cbc -nopad` on this machine).
    #[test]
    fn aes_256_cbc_matches_the_nist_vector() {
        use aes::cipher::block_padding::NoPadding;
        use aes::cipher::{BlockDecryptMut, KeyIvInit};

        let key: [u8; 32] = hex_array("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4");
        let iv: [u8; 16] = hex_array("000102030405060708090a0b0c0d0e0f");
        let plaintext = hex_vec(
            "6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51\
             30c81c46a35ce411e5fbc1191a0a52eff69f2445df4f9b17ad2b417be66c3710",
        );
        let ciphertext = hex_vec(
            "f58c4c04d6e5f1ba779eabfb5f7bfbd69cfc4e967edb808d679f777bc6702c7d\
             39f23369a9d9bacfa530e26304231461b2eb05e2c39be9fcda6c19078c6a9d1b",
        );
        let mut buf = ciphertext.clone();
        let dec = cbc::Decryptor::<aes::Aes256>::new((&key).into(), (&iv).into());
        let out = dec.decrypt_padded_mut::<NoPadding>(&mut buf).unwrap();
        assert_eq!(out, plaintext.as_slice());
    }

    /// All-zero key + all-zero IV on one block, cross-checked with `openssl enc -aes-256-cbc`.
    #[test]
    fn zero_iv_and_zero_key_decrypt_one_block() {
        use aes::cipher::block_padding::NoPadding;
        use aes::cipher::{BlockDecryptMut, KeyIvInit};

        let ciphertext = hex_vec("dc95c078a2408989ad48a21492842087");
        let mut buf = ciphertext;
        let dec = cbc::Decryptor::<aes::Aes256>::new((&[0u8; 32]).into(), (&[0u8; 16]).into());
        let out = dec.decrypt_padded_mut::<NoPadding>(&mut buf).unwrap();
        assert_eq!(out, [0u8; 16].as_slice());
    }

    #[test]
    fn password_decrypt_round_trips_with_pkcs7() {
        use aes::cipher::block_padding::Pkcs7;
        use aes::cipher::{BlockEncryptMut, KeyIvInit};

        let password = b"mot-mat-khau-dai-hon-32-byte-de-kiem-tra";
        let key = derive_key(password);
        let plaintext = b"ZDB4.0\x00\x00\x00\x00payload cua cross db".to_vec();
        // Leave room for one padding block: `encrypt_padded_mut` writes the PKCS#7 tail itself.
        let mut buf = plaintext.clone();
        buf.resize(plaintext.len() + 16, 0);
        let enc = cbc::Encryptor::<aes::Aes256>::new((&key).into(), (&[0u8; 16]).into());
        let ciphertext = enc
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .unwrap()
            .to_vec();
        assert_eq!(ciphertext.len() % 16, 0);
        assert_eq!(
            decrypt_cbc_pkcs7(&key, &[0u8; 16], &ciphertext).unwrap(),
            plaintext
        );
        // The native path does not strip padding: the padded tail stays in the plaintext.
        let raw = decrypt_with_password(password, &ciphertext).unwrap();
        assert_eq!(raw.len(), ciphertext.len());
        assert_eq!(&raw[..plaintext.len()], plaintext.as_slice());
        assert!(raw[plaintext.len()..].iter().any(|b| *b != 0));
        // Wrong password yields a padding error (or garbage that never equals the plaintext).
        assert!(decrypt_cbc_pkcs7(&derive_key(b"other-password"), &[0u8; 16], &ciphertext).is_err());
        assert!(decrypt_with_password(password, &ciphertext[..15]).is_err());
        assert!(decrypt_with_password(password, b"").is_err());
    }

    fn hex_vec(s: &str) -> Vec<u8> {
        let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(cleaned.len() % 2, 0, "hex lẻ");
        (0..cleaned.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).unwrap())
            .collect()
    }

    fn hex_array<const N: usize>(s: &str) -> [u8; N] {
        let v = hex_vec(s);
        assert_eq!(v.len(), N);
        let mut out = [0u8; N];
        out.copy_from_slice(&v);
        out
    }
}
