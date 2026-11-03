//! Part B — "Đồng bộ từ điện thoại" (Sync V1).
//!
//! The phone sends a backup container that Zalo PC opens with the native addon
//! `db-cross-v4-native.node`. [`format`] documents and parses that container. It is **not** wired
//! into the client: the restore pipeline still needs a real backup sample to validate the key
//! derivation and the chunked decryption (`re-notes/15_db_cross_v4_decrypt.md` §7).

pub mod format;
