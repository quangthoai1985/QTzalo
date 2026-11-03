//! Per-user application data locations and private file writes.
//!
//! Earlier builds wrote `session.json`, `zalo_history.db`, caches and logs into whatever the
//! current working directory happened to be, with default (world-readable on Unix) permissions.
//! Everything now lives in one per-user directory:
//!
//! | OS      | Location                                              |
//! |---------|-------------------------------------------------------|
//! | Windows | `%APPDATA%\QTzalo`                                    |
//! | macOS   | `~/Library/Application Support/QTzalo`                |
//! | Linux   | `$XDG_DATA_HOME/qtzalo` or `~/.local/share/qtzalo`    |
//!
//! `QTZALO_DATA_DIR` overrides the location (useful for tests and portable setups).
//! Files holding credentials are written atomically with mode `0600` on Unix; the directory
//! itself is `0700`. On Windows `%APPDATA%` is already private to the user profile.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Folder name used under the platform's application-data root.
pub const APP_DIR_NAME: &str = "QTzalo";

/// Files that older builds left in the working directory and that [`migrate_legacy_files`] moves.
const LEGACY_FILES: &[&str] = &[
    "session.json",
    "zalo_history.db",
    "zalo_history.db-wal",
    "zalo_history.db-shm",
    "threads_cache.json",
    "app_settings.json",
];

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// The per-user data directory. Created (private) on first use.
pub fn data_dir() -> &'static Path {
    DATA_DIR.get_or_init(|| {
        let dir = resolve_data_dir();
        let _ = create_private_dir(&dir);
        dir
    })
}

fn non_empty_env(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn resolve_data_dir() -> PathBuf {
    if let Some(dir) = non_empty_env("QTZALO_DATA_DIR") {
        return dir;
    }
    platform_data_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "windows")]
fn platform_data_dir() -> Option<PathBuf> {
    non_empty_env("APPDATA")
        .or_else(|| non_empty_env("USERPROFILE").map(|p| p.join("AppData").join("Roaming")))
        .map(|root| root.join(APP_DIR_NAME))
}

#[cfg(target_os = "macos")]
fn platform_data_dir() -> Option<PathBuf> {
    non_empty_env("HOME").map(|home| {
        home.join("Library")
            .join("Application Support")
            .join(APP_DIR_NAME)
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_data_dir() -> Option<PathBuf> {
    non_empty_env("XDG_DATA_HOME")
        .or_else(|| non_empty_env("HOME").map(|home| home.join(".local").join("share")))
        .map(|root| root.join("qtzalo"))
}

#[cfg(not(any(unix, target_os = "windows")))]
fn platform_data_dir() -> Option<PathBuf> {
    None
}

/// `session.json` — live credentials. Write it only through [`write_private`].
pub fn session_file() -> PathBuf {
    data_dir().join("session.json")
}

/// Local SQLite message history.
pub fn history_db() -> PathBuf {
    data_dir().join("zalo_history.db")
}

/// Cached thread list (`threads_cache.json`).
pub fn threads_cache_file() -> PathBuf {
    data_dir().join("threads_cache.json")
}

/// GUI settings (`app_settings.json`).
pub fn settings_file() -> PathBuf {
    data_dir().join("app_settings.json")
}

/// Image / sticker / thumbnail cache directory (`stickers_cache/`). Created on demand.
pub fn cache_dir() -> PathBuf {
    let dir = data_dir().join("stickers_cache");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Per-account sync cursors (`sync_state_<uin>.json`, see `crate::sync_state`). `None` when
/// `uin` is not a plain digit string, so it can never escape the data directory.
pub fn sync_state_file(uin: &str) -> Option<PathBuf> {
    let uin = crate::sync_state::sanitize_uin(uin)?;
    Some(data_dir().join(format!("sync_state_{uin}.json")))
}

/// Videos fetched for in-app playback (`stickers_cache/videos/`, one `<md5(url)>.mp4` per
/// video). Lives under [`cache_dir`] so "Xoá cache" and the cache size include it. Created on demand.
pub fn video_cache_dir() -> PathBuf {
    let dir = cache_dir().join("videos");
    let _ = create_private_dir(&dir);
    dir
}

/// Delete `clipboard_*.png` files in [`cache_dir`] older than `max_age`; returns how many.
///
/// Pasting an image from the clipboard writes a temporary PNG into the cache before upload.
/// Those files are never needed again, so they are swept once at GUI startup.
pub fn cleanup_old_clipboard_images(max_age: std::time::Duration) -> usize {
    cleanup_clipboard_images_in(&cache_dir(), max_age, std::time::SystemTime::now())
}

/// Testable core of [`cleanup_old_clipboard_images`]: only regular files (never symlinks or
/// directories) named `clipboard_*.png` whose modification time is more than `max_age` before
/// `now` are removed. Files with a future or unreadable mtime are kept.
fn cleanup_clipboard_images_in(
    dir: &Path,
    max_age: std::time::Duration,
    now: std::time::SystemTime,
) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !(name.starts_with("clipboard_") && name.ends_with(".png")) {
            continue;
        }
        // `DirEntry::metadata` does not follow symlinks, so a link is never treated as a file.
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.file_type().is_file() {
            continue;
        }
        let old_enough = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > max_age);
        if old_enough && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Upper bound for the on-disk image/video cache ([`cache_dir`]).
///
/// Zalo PC never trims its media cache: on this machine `%APPDATA%\ZaloData` reached 22 GB after
/// ~3 months, 91 % of it cached media (`re-notes/12_zalo-electron-va-dung-luong.md`). QTzalo keeps
/// the same speed benefit from caching but bounds it at startup.
pub const CACHE_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Anything in the cache older than this is swept at startup.
pub const CACHE_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);

/// Trim [`cache_dir`] to [`CACHE_MAX_BYTES`]: first delete regular files older than
/// [`CACHE_MAX_AGE`], then delete least-recently-modified files until the rest fits.
///
/// Returns `(bytes_freed, files_removed)`. Symlinks and directories are never followed or removed;
/// the session, history DB and settings live one level up and are never touched.
pub fn trim_cache_dir() -> (u64, usize) {
    trim_cache_in(
        &cache_dir(),
        CACHE_MAX_AGE,
        CACHE_MAX_BYTES,
        std::time::SystemTime::now(),
    )
}

/// Testable core of [`trim_cache_dir`].
fn trim_cache_in(
    dir: &Path,
    max_age: std::time::Duration,
    max_bytes: u64,
    now: std::time::SystemTime,
) -> (u64, usize) {
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    collect_cache_files(dir, &mut files, 0);

    let mut freed = 0u64;
    let mut removed = 0usize;
    let mut kept: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    for (path, size, mtime) in files {
        let too_old = now
            .duration_since(mtime)
            .map(|age| age > max_age)
            .unwrap_or(false);
        if too_old && std::fs::remove_file(&path).is_ok() {
            freed += size;
            removed += 1;
        } else {
            kept.push((path, size, mtime));
        }
    }

    let mut total: u64 = kept.iter().map(|(_, size, _)| *size).sum();
    if total > max_bytes {
        kept.sort_by_key(|(_, _, mtime)| *mtime);
        for (path, size, _) in kept {
            if total <= max_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
                freed += size;
                removed += 1;
            }
        }
    }
    (freed, removed)
}

/// Collect regular files below `dir` (bounded depth, no symlink following).
fn collect_cache_files(
    dir: &Path,
    out: &mut Vec<(PathBuf, u64, std::time::SystemTime)>,
    depth: usize,
) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // `DirEntry::metadata` does not follow symlinks, so links are neither files nor dirs here.
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_file() {
            out.push((
                entry.path(),
                meta.len(),
                meta.modified().unwrap_or(std::time::UNIX_EPOCH),
            ));
        } else if meta.is_dir() {
            collect_cache_files(&entry.path(), out, depth + 1);
        }
    }
}

/// Diagnostic logs (`logs/`). Created on demand. Only written in dev mode.
pub fn logs_dir() -> PathBuf {
    let dir = data_dir().join("logs");
    let _ = create_private_dir(&dir);
    dir
}

/// Protocol dumps (`dump/`, `--dump`). Created on demand.
pub fn dump_dir() -> PathBuf {
    let dir = data_dir().join("dump");
    let _ = create_private_dir(&dir);
    dir
}

/// Create `dir` (and parents); on Unix restrict it to the owner (`0700`).
pub fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Restrict an existing file to the owner (`0600` on Unix; no-op elsewhere).
pub fn restrict_file(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Atomically write `contents` to `path`, owner-only on Unix.
///
/// Writes to a sibling `*.tmp` file first and renames it over the target, so a crash never
/// leaves a half-written `session.json` behind.
pub fn write_private(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        create_private_dir(parent)?;
    }
    let mut tmp_name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(contents.as_ref())?;
        f.sync_all()?;
    }
    restrict_file(&tmp);
    std::fs::rename(&tmp, path)?;
    restrict_file(path);
    Ok(())
}

/// Move files that older builds wrote into the current directory into [`data_dir`].
///
/// Only moves a file when the destination does not exist yet, so it never overwrites newer data.
/// Returns the names that were moved.
pub fn migrate_legacy_files() -> Vec<String> {
    let mut moved = Vec::new();
    let Ok(cwd) = std::env::current_dir() else {
        return moved;
    };
    let data = data_dir();
    if same_dir(&cwd, data) {
        return moved;
    }
    for name in LEGACY_FILES {
        let src = cwd.join(name);
        let dst = data.join(name);
        if src.is_file() && !dst.exists() && move_file(&src, &dst).is_ok() {
            restrict_file(&dst);
            moved.push((*name).to_string());
        }
    }
    let src_cache = cwd.join("stickers_cache");
    let dst_cache = data.join("stickers_cache");
    if src_cache.is_dir() && !dst_cache.exists() && std::fs::rename(&src_cache, &dst_cache).is_ok() {
        moved.push("stickers_cache/".to_string());
    }
    moved
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    // Different volume: copy then remove.
    std::fs::copy(src, dst)?;
    std::fs::remove_file(src)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "qtzalo-paths-{tag}-{}-{}",
            std::process::id(),
            crate::timefmt::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_private_replaces_atomically() {
        let dir = temp_dir("write");
        let f = dir.join("session.json");
        write_private(&f, b"first").unwrap();
        write_private(&f, b"second").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"second");
        assert!(!dir.join("session.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_removes_only_old_clipboard_pngs() {
        use std::time::{Duration, SystemTime};
        let dir = temp_dir("clip");
        for name in [
            "clipboard_1.png",
            "clipboard_2.png",
            "clipboard_3.jpg",
            "sticker_1.png",
            "my_clipboard_4.png",
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        std::fs::create_dir(dir.join("clipboard_dir.png")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.join("sticker_1.png"), dir.join("clipboard_link.png"))
            .unwrap();
        let day = Duration::from_secs(24 * 3600);

        // Just written: nothing is old enough yet.
        assert_eq!(cleanup_clipboard_images_in(&dir, day, SystemTime::now()), 0);

        // Two days later: only the two `clipboard_*.png` regular files go.
        let later = SystemTime::now() + 2 * day;
        assert_eq!(cleanup_clipboard_images_in(&dir, day, later), 2);
        assert!(!dir.join("clipboard_1.png").exists());
        assert!(!dir.join("clipboard_2.png").exists());
        for kept in [
            "clipboard_3.jpg",
            "sticker_1.png",
            "my_clipboard_4.png",
            "clipboard_dir.png",
        ] {
            assert!(dir.join(kept).exists(), "{kept} must be kept");
        }
        #[cfg(unix)]
        assert!(dir.join("clipboard_link.png").symlink_metadata().is_ok());

        assert_eq!(
            cleanup_clipboard_images_in(&dir.join("missing"), day, later),
            0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_private_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("perm");
        let f = dir.join("secret.json");
        write_private(&f, b"x").unwrap();
        let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_trim_removes_old_then_largest_oldest() {
        use std::time::{Duration, SystemTime};
        let dir = temp_dir("trim");
        std::fs::create_dir_all(dir.join("videos")).unwrap();
        let day = Duration::from_secs(24 * 3600);
        std::fs::write(dir.join("old.png"), b"0123456789").unwrap();
        std::fs::write(dir.join("videos").join("old.mp4"), b"0123456789").unwrap();
        std::fs::write(dir.join("fresh.png"), b"0123456789").unwrap();

        // Fresh files only: age pass keeps everything, size cap of 1 GiB is not reached.
        let (freed, removed) = trim_cache_in(&dir, day, 1024 * 1024 * 1024, SystemTime::now());
        assert_eq!((freed, removed), (0, 0));
        assert!(dir.join("fresh.png").exists());

        // 100 days later: all three files are older than 30 days and go.
        let later = SystemTime::now() + 100 * day;
        let (freed, removed) = trim_cache_in(&dir, day, 1024 * 1024 * 1024, later);
        assert_eq!((freed, removed), (30, 3));
        assert!(!dir.join("old.png").exists());
        assert!(!dir.join("videos").join("old.mp4").exists());

        // Size cap path: three fresh 10-byte files, cap of 15 bytes -> two are removed.
        for name in ["a.png", "b.png", "c.png"] {
            std::fs::write(dir.join(name), b"0123456789").unwrap();
        }
        let (freed, removed) = trim_cache_in(&dir, day, 15, SystemTime::now());
        assert_eq!(removed, 2);
        assert_eq!(freed, 20);
        let left: u64 = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum();
        assert!(left <= 15, "cache must fit the cap, left {left}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
