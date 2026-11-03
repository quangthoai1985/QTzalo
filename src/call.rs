//! Zalo Call & Video Call (`zcall-v2`) native bridge and signaling protocol.
//!
//! Reverse-engineered from `Zalo-26.10.11`:
//! - Main process bridge (`main-dist/main.js` modules `"3Zc2"`, `"vqv6"`, `"5NUA"`, `"bhjE"`, `"ZQzv"`):
//!   - Verifies `plugins/capture/ZaloCall.exe` MD5 (`a2b920c00115ba11a7fcdcb642147e9c`).
//!   - Creates Windows Named Pipes `\\.\pipe\PipeZCallRecv` and `\\.\pipe\PipeZCallSend`.
//!   - Encrypts/decrypts JSON frames with AES-128-CBC (`key = base64("yjAF9oqMWl6XfXYJn9mA7w==")`,
//!     `iv = [0u8; 16]`), hex-encoded and delimited by `'$'`.
//! - Renderer `[zcall-v2] call-helper` (`default-login-main-startup-shared-worker-znotification.*.js`):
//!   - Dispatches `sendSignal` (`401`–`416`, `12006`–`12447`) over `/api/voicecall/*` and `/api/voicecall/group/*`,
//!     returning decrypted `data` payloads via `recvSignal`.
//!   - Forwards incoming VoIP control events (`act_type == "voip"`) from `lp_control` (`queueIds: [4, 5]`)
//!     to `ZaloCall.exe` via `{ "type": "control", "data": event }`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use aes::Aes128;
use anyhow::{anyhow, bail, Result};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};

use crate::chat::QueueCursor;
use crate::gui::GuiEvent;
use crate::session::Session;

/// Expected MD5 digest of `Zalo-26.10.11/plugins/capture/ZaloCall.exe` (`CALL_NATIVE_HASH` in module `"ZQzv"`).
pub const CALL_NATIVE_HASH: &str = "a2b920c00115ba11a7fcdcb642147e9c";

/// Named Pipe where `zalo-rust` receives frames from `ZaloCall.exe`.
pub const PIPE_ZCALL_RECV: &str = r"\\.\pipe\PipeZCallRecv";

/// Named Pipe where `zalo-rust` sends frames to `ZaloCall.exe`.
pub const PIPE_ZCALL_SEND: &str = r"\\.\pipe\PipeZCallSend";

/// `callV2TransportAesKey = "yjAF9oqMWl6XfXYJn9mA7w=="` decoded to 16 raw bytes.
pub const CALL_V2_AES_KEY: [u8; 16] = [
    0xca, 0x30, 0x05, 0xf6, 0x8a, 0x8c, 0x5a, 0x5e, 0x97, 0x7d, 0x76, 0x09, 0x9f, 0xd9, 0x80, 0xef,
];

const ZERO_IV: [u8; 16] = [0u8; 16];

/// Call type constants matching `[zcall-v2] call-helper::makeCall` (`r = n ? (t ? 6 : 5) : (t ? 3 : 1)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CallType {
    DirectAudio = 1,
    DirectVideo = 3,
    GroupAudio = 5,
    GroupVideo = 6,
}

impl CallType {
    pub fn from_flags(is_video: bool, is_group: bool) -> Self {
        match (is_group, is_video) {
            (false, false) => Self::DirectAudio,
            (false, true) => Self::DirectVideo,
            (true, false) => Self::GroupAudio,
            (true, true) => Self::GroupVideo,
        }
    }

    pub fn label_vi(self) -> &'static str {
        match self {
            Self::DirectAudio => "Gọi thoại",
            Self::DirectVideo => "Gọi video",
            Self::GroupAudio => "Gọi nhóm thoại",
            Self::GroupVideo => "Gọi nhóm video",
        }
    }
}

/// Outbound-call gate.
///
/// Earlier builds shipped a test lock that only allowed dialing three hard-coded test accounts
/// (any contact whose name contained "hoang", "do thanh" or "dtc"). It is gone: every 1-1
/// contact can be called. Group calls stay refused because the group-call signaling
/// (`/api/voicecall/group/*`) is not implemented in this client.
pub fn call_target_allowed(partner_id: &str, _partner_name: &str, is_group: bool) -> bool {
    if is_group {
        return false;
    }
    let id = partner_id.trim();
    !id.is_empty() && !id.to_ascii_lowercase().starts_with("group")
}

/// Same as [`call_target_allowed`], returning the Vietnamese refusal reason for the UI.
pub fn ensure_call_target_allowed(
    partner_id: &str,
    partner_name: &str,
    is_group: bool,
) -> Result<()> {
    if call_target_allowed(partner_id, partner_name, is_group) {
        return Ok(());
    }
    if is_group || partner_id.trim().to_ascii_lowercase().starts_with("group") {
        bail!("Chưa hỗ trợ gọi nhóm trong phiên bản này");
    }
    bail!("Không xác định được người nhận cuộc gọi")
}

/// `typeRequest` of a `/api/voicecall/requestcall` (`401`) body (`1` audio, `3` video, `5`/`6` group).
///
/// The two `401` callers in this crate spell the key differently: `CallBridge` sends
/// `typeRequest` (see `CallEngineCmd::MakeCall`), while `call_probe` — mirroring the captured
/// official payload — sends `type`. Reading only one of them silently pinned every GUI dial to
/// `1`, so a video call was registered as an audio call. Both spellings are accepted here; a
/// missing/invalid value falls back to `1` (plain audio), which is what the old code did.
pub fn call_type_from_payload(payload: &Value) -> i32 {
    for key in ["typeRequest", "type"] {
        let Some(v) = payload.get(key) else { continue };
        if let Some(n) = v.as_i64() {
            return n as i32;
        }
        if let Some(s) = v.as_str() {
            if let Ok(n) = s.trim().parse::<i32>() {
                return n;
            }
        }
    }
    1
}

/// Encrypt a JSON string into a `$`-terminated hex AES-128-CBC frame (`encrypt` in module `"vqv6"`).
pub fn encrypt_pipe_frame(plain_json: &str) -> Result<String> {
    let bytes = plain_json.as_bytes();
    let mut buf = vec![0u8; bytes.len() + 16];
    buf[..bytes.len()].copy_from_slice(bytes);
    let enc = cbc::Encryptor::<Aes128>::new_from_slices(&CALL_V2_AES_KEY, &ZERO_IV)?;
    let ct = enc
        .encrypt_padded_mut::<Pkcs7>(&mut buf, bytes.len())
        .map_err(|_| anyhow!("AES-128-CBC pipe encryption padding failed"))?;

    let mut out = String::with_capacity(ct.len() * 2 + 1);
    for b in ct {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out.push('$');
    Ok(out)
}

/// Decrypt a single hex-encoded AES-128-CBC frame (without the trailing `$`) into UTF-8 JSON (`decrypt` in `"vqv6"`).
pub fn decrypt_pipe_frame(hex_cipher: &str) -> Result<String> {
    let trimmed = hex_cipher.trim().trim_end_matches('$');
    if trimmed.is_empty() {
        bail!("empty pipe frame");
    }
    if trimmed.len() % 2 != 0 {
        bail!("odd hex length in pipe frame: {}", trimmed.len());
    }
    let mut raw = Vec::with_capacity(trimmed.len() / 2);
    let bytes = trimmed.as_bytes();
    for chunk in bytes.chunks_exact(2) {
        let s = std::str::from_utf8(chunk)?;
        let b = u8::from_str_radix(s, 16)?;
        raw.push(b);
    }
    let dec = cbc::Decryptor::<Aes128>::new_from_slices(&CALL_V2_AES_KEY, &ZERO_IV)?;
    let pt = dec
        .decrypt_padded_mut::<Pkcs7>(&mut raw)
        .map_err(|_| anyhow!("AES-128-CBC pipe unpad failed"))?;
    Ok(String::from_utf8(pt.to_vec())?)
}

/// Extract complete `$`-delimited frames from an accumulation buffer, leaving any partial tail in `accum`.
pub fn drain_pipe_frames(accum: &mut String) -> Vec<String> {
    let mut frames = Vec::new();
    while let Some(pos) = accum.find('$') {
        let token: String = accum.drain(..=pos).collect();
        let hex_part = token.trim_end_matches('$').trim();
        if !hex_part.is_empty() {
            frames.push(hex_part.to_string());
        }
    }
    frames
}

/// Compute lowercase hex MD5 of a file on disk (`verifyMd5` in module `"vqv6"`).
pub fn compute_file_md5(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut h = Md5::new();
    h.update(&bytes);
    let out = h.finalize();
    Ok(out.iter().fold(String::with_capacity(32), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    }))
}

/// Locate `ZaloCall.exe` in the standard Zalo PC install directory (`%LOCALAPPDATA%\Programs\Zalo`).
///
/// Working-directory-relative candidates and a developer's hard-coded `F:\…` path were removed:
/// an executable must never be picked up from wherever the app happens to be started.
pub fn find_zalocall_exe() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
        let zalo_root = Path::new(&local_app).join("Programs").join("Zalo");
        if let Ok(entries) = std::fs::read_dir(&zalo_root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let candidate = p.join("plugins").join("capture").join("ZaloCall.exe");
                    candidates.push(candidate);
                }
            }
        }
    }

    candidates.into_iter().find(|p| p.is_file())
}

/// Build the `init` / `updateLocal` payload expected by `ZaloCall.exe` (`getInitialData` in `[zcall-v2] call-helper`).
pub fn build_init_payload(s: &Session, command: &str, should_start_native: bool) -> Value {
    let (uid, dname) = s
        .login
        .as_ref()
        .map(|l| {
            // `ZaloCall.exe` converts `local.id` to a 64-bit integer via `std::stoull`.
            // Prefer the user's decimal `userId` (`l.uin`); fall back to `send2me_id` only if `uin` is a hex `dkey`.
            let numeric_uid = if !l.uin.is_empty() && l.uin.chars().all(|c| c.is_ascii_digit()) {
                l.uin.clone()
            } else if !l.send2me_id.is_empty()
                && l.send2me_id.chars().all(|c| c.is_ascii_digit())
            {
                l.send2me_id.clone()
            } else if !l.uin.is_empty() {
                l.uin.clone()
            } else {
                l.send2me_id.clone()
            };
            (numeric_uid, l.display_name.clone())
        })
        .unwrap_or_else(|| ("0".into(), "Zalo User".into()));

    let log_dir = crate::paths::dump_dir();

    let call_log = log_dir.join("call.log").to_string_lossy().to_string();
    let zrtc_log = log_dir.join("zrtc.log").to_string_lossy().to_string();
    let crash_dmp = log_dir.join("crash.dmp").to_string_lossy().to_string();

    let os_info = format!(
        "Windows_NT_10.0_win32_x64;{} ({})",
        s.computer_name,
        std::env::consts::ARCH
    );

    json!({
        "type": "update",
        "command": command,
        "data": {
            "local": {
                "id": uid,
                "avatar": "",
                "name": if dname.is_empty() { "Zalo User" } else { &dname }
            },
            "osInfo": os_info,
            "mainWindowId": uid,
            "logPath": call_log,
            "zrtcLogPath": zrtc_log,
            "dumpPath": crash_dmp,
            "clientVersion": 694,
            "language": "vi",
            "timeoutMakeCall2ZRtp": 15000,
            "enablePreviewDeviceOutgoing": true,
            "enablePreviewDeviceIncall": true,
            "enableUnstableConnecting": true,
            "receiveCallWhileNoMicrophoneDetected": true,
            "receiveCallWhileNoSpeakerDetected": true,
            "autoAudioInput": true,
            "autoAudioOutput": true,
            "defaultAudInputDevice": null,
            "defaultAudOutputDevice": null,
            "defaultVidDevice": null
        },
        "_optional": !should_start_native
    })
}

/// Build the `makeCall` request frame (`makeCall` in `[zcall-v2] call-helper`).
pub fn build_make_call_payload(
    partner_id: &str,
    partner_name: &str,
    partner_avatar: Option<&str>,
    is_video: bool,
    is_group: bool,
) -> Value {
    let call_type = CallType::from_flags(is_video, is_group) as u8;
    let avatar_url = partner_avatar.unwrap_or("");
    let group_info = if is_group {
        let clean_gid = partner_id
            .trim_start_matches("group_")
            .trim_start_matches('g')
            .trim_start_matches('G');
        json!({
            "id": format!("group_{clean_gid}"),
            "name": partner_name,
            "avatar": avatar_url
        })
    } else {
        Value::Null
    };

    json!({
        "type": "request",
        "command": "makeCall",
        "data": {
            "partner": [
                {
                    "id": partner_id,
                    "avatar": avatar_url,
                    "name": partner_name
                }
            ],
            "type": call_type,
            "groupInfo": group_info,
            "isFirstCall": false
        }
    })
}

// ---------------------------------------------------------------------------
// Outbound Native-to-Server Signaling (`/api/voicecall/*` & `/api/voicecall/group/*`)
// ---------------------------------------------------------------------------

fn voice_call_domain(s: &Session) -> &str {
    if s.domains.voice_call.is_empty() {
        "https://voicecall-wpa.chat.zalo.me"
    } else {
        &s.domains.voice_call
    }
}

/// Normalize a `/api/voicecall/*` response `data` field so `ZaloCall.exe` (`zmessagedeseralizer.cpp`)
/// always receives a non-null JSON object (`Json::Value::isObject() == true`).
pub fn normalize_voicecall_data(raw: Value) -> Value {
    match raw {
        Value::Object(_) => raw,
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                json!({})
            } else if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                match parsed {
                    Value::Object(_) => parsed,
                    other => json!({ "value": other }),
                }
            } else {
                json!({ "value": s })
            }
        }
        Value::Null => json!({}),
        other => json!({ "value": other }),
    }
}

async fn voicecall_get(s: &Session, endpoint_path: &str, payload: &Value) -> Result<Value> {
    if s.login.is_none() {
        bail!("chưa đăng nhập");
    }
    let encoded = s.encoded_params(payload)?;
    let host = voice_call_domain(s).trim_end_matches('/').to_string();
    trace_line("VCALL", format!("{host}{endpoint_path}"));
    // `ZCALL_ZPW=ver:type` overrides `zpw_ver`/`zpw_type` **for voicecall only**. The captured
    // official desktop sends `693&24` on these endpoints while the rest of its API surface uses
    // `694&30`, so this knob tests whether the backend keys the invite/media decision on the pair.
    let zpw = std::env::var("ZCALL_ZPW")
        .ok()
        .filter(|v| v.contains(':'))
        .map(|v| {
            let (ver, ty) = v.split_once(':').unwrap();
            format!("zpw_ver={}&zpw_type={}", ver.trim(), ty.trim())
        })
        .unwrap_or_else(|| s.common_params());
    let url = format!(
        "{}{endpoint_path}?{}&params={encoded}",
        voice_call_domain(s).trim_end_matches('/'),
        zpw
    );
    let resp = s.get(&url).await?;
    let decrypted = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(code) = decrypted.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = decrypted
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            bail!("voicecall error {code}: {msg}");
        }
    }
    let raw_data = decrypted.get("data").cloned().unwrap_or(Value::Null);
    Ok(normalize_voicecall_data(raw_data))
}

/// Is call tracing enabled? Only in dev mode (`--dev`) or with `ZALO_CALL_TRACE=1`.
pub fn call_trace_enabled() -> bool {
    crate::logger::is_dev_mode()
        || std::env::var("ZALO_CALL_TRACE").ok().as_deref() == Some("1")
}

/// Opt-in switch (`ZALO_CALL_LOOPBACK=1`) for the legacy mic→speaker loopback fallback.
///
/// Off by default: the loopback only echoes the local microphone to the local speakers, so the
/// peer hears nothing while the user hears themselves. A failed media start is reported in the
/// call UI instead (see `CallBridge`).
fn loopback_fallback_enabled() -> bool {
    std::env::var("ZALO_CALL_LOOPBACK").ok().as_deref() == Some("1")
}

/// Append a diagnostic line to `logs/call_trace.log` (per-user data dir) and mirror it to stderr.
///
/// The trace contains decrypted signaling payloads (call ids, session ids, RTP addresses), so it
/// is **off by default**: it is only written in dev mode or with `ZALO_CALL_TRACE=1`
/// ([`call_trace_enabled`]). Earlier builds always wrote it into the working directory.
pub fn trace_line(tag: &str, body: impl AsRef<str>) {
    use std::io::Write;
    if !call_trace_enabled() {
        return;
    }
    let body = body.as_ref();
    let ts = crate::login::unix_now();
    let path = crate::paths::logs_dir().join("call_trace.log");
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    if let Ok(mut f) = opts.open(&path) {
        let _ = writeln!(f, "[{ts}] {tag} {body}");
    }
    eprintln!("[call] {tag} {body}");
}

/// Execute the exact Zalo signaling HTTP endpoint for a native `sendSignal` command (`handleSendSignal`).
/// Every request/response pair is mirrored into `call_trace.log`.
pub async fn dispatch_send_signal(s: &Session, command: i64, n: &Value) -> Result<Value> {
    trace_line("SIG>", format!("cmd={command} {}", n));
    let result = dispatch_send_signal_inner(s, command, n).await;
    match &result {
        Ok(v) => trace_line("SIG<", format!("cmd={command} {v}")),
        Err(e) => trace_line("SIG!", format!("cmd={command} err={e}")),
    }
    result
}

async fn dispatch_send_signal_inner(s: &Session, command: i64, n: &Value) -> Result<Value> {
    // Outbound-dial lock: `401`/`416` are the only commands that create a call record server-side,
    // so refusing them here covers the GUI, `call_probe` and any future caller in one place.
    if matches!(command, 401 | 416) {
        let callee = n
            .get("calleeId")
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        ensure_call_target_allowed(&callee, "", false)?;
    }

    match command {
        // 401 -> cmd 11300: `/api/voicecall/requestcall`
        401 => {
            // The server pushes the callee invite at *this* request, so the media description has
            // to ride on the 401: the official client sends `extendData` (with its cached relay in
            // `serverAddr`), `rtpAddress`, `rtcpAddress` and its own `session` here — and nothing
            // else. The desktop body has exactly these seven keys: no `typeRequest`, no `imei`,
            // no `subCommand` (`subCommand:3` belongs to the 416 only). Extra keys made the
            // backend degrade the invite (`params.status=5`, `extendData` stripped).
            let has_media = n
                .get("extendData")
                .filter(|v| !v.is_null() && v.as_str() != Some(""))
                .is_some();
            let body = if has_media {
                json!({
                    "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                    "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                    "codec": n.get("codec").cloned().unwrap_or(json!("[]")),
                    "extendData": n.get("extendData").cloned().unwrap_or(json!("")),
                    "rtcpAddress": n.get("rtcpAddress").cloned().unwrap_or(json!("")),
                    "rtpAddress": n.get("rtpAddress").cloned().unwrap_or(json!("")),
                    "session": n.get("session").cloned().unwrap_or(json!("")),
                })
            } else {
                // Legacy path (no cached relay yet): keep the original minimal dial body.
                json!({
                    "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                    "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                    "codec": n.get("codec").cloned().unwrap_or(json!("[]")),
                    // `typeRequest` first (CallBridge), then `type` (official capture / call_probe).
                    "typeRequest": call_type_from_payload(n),
                    "imei": s.imei,
                })
            };
            voicecall_get(s, "/api/voicecall/requestcall", &body).await
        }
        // 416 -> cmd 11301: `/api/voicecall/request`
        416 => {
            let mut body = json!({
                "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                "rtcpAddress": n.get("rtcpAddress").cloned().unwrap_or(json!("")),
                "rtpAddress": n.get("rtpAddress").cloned().unwrap_or(json!("")),
                "codec": n.get("codec").cloned().unwrap_or(json!("[]")),
                "session": n.get("session").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "imei": s.imei,
            });
            if let Some(ext) = n.get("extendData").filter(|v| !v.is_null()) {
                if let Some(obj) = body.as_object_mut() {
                    obj.insert("extendData".into(), ext.clone());
                    obj.insert("subCommand".into(), json!(3));
                }
            }
            voicecall_get(s, "/api/voicecall/request", &body).await
        }
        // 407 -> cmd 11302: `/api/voicecall/ringring`
        407 => {
            let body = json!({
                "callerId": n.get("callerId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "status": 0,
                "imei": s.imei,
            });
            voicecall_get(s, "/api/voicecall/ringring", &body).await
        }
        // 402 -> cmd 11303: `/api/voicecall/answer`
        402 => {
            // Mirror the official `sendAnswerCall(callerId, callId, status, codec, extendData,
            // rtcpAddress, rtpAddress, session_id)`: keys with empty values are omitted the same
            // way `JSON.stringify` drops `undefined`, because the server answers `114` for an
            // empty `rtpAddress`/`session_id`.
            let mut body = json!({});
            let obj = body.as_object_mut().expect("json object");
            for key in [
                "callerId",
                "callId",
                "status",
                "codec",
                "rtcpAddress",
                "rtpAddress",
            ] {
                if let Some(v) = n.get(key) {
                    if !v.is_null() {
                        obj.insert(key.to_string(), v.clone());
                    }
                }
            }
            let sess = n
                .get("session_id")
                .or_else(|| n.get("session"))
                .filter(|v| v.as_str().map(|x| !x.is_empty()).unwrap_or(true));
            if let Some(v) = sess {
                obj.insert("session_id".to_string(), v.clone());
            }
            if let Some(v) = n.get("imei").filter(|v| !v.is_null()) {
                obj.insert("imei".to_string(), v.clone());
            }
            if let Some(ext) = n.get("extendData").filter(|v| !v.is_null()) {
                obj.insert("extendData".to_string(), ext.clone());
                obj.insert("subCommand".to_string(), json!(3));
            }
            voicecall_get(s, "/api/voicecall/answer", &body).await
        }
        // 408 -> cmd 11304: `/api/voicecall/answerack`
        408 => {
            let body = json!({
                "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "status": 0,
                "imei": s.imei,
            });
            voicecall_get(s, "/api/voicecall/answerack", &body).await
        }
        // 405 -> cmd 11305: `/api/voicecall/cancel`
        405 => {
            let body = json!({
                "callerId": n.get("toId").or_else(|| n.get("callerId")).cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "callType": n.get("callType").cloned().unwrap_or(json!(1)),
                "status": 0,
                "imei": s.imei,
            });
            voicecall_get(s, "/api/voicecall/cancel", &body).await
        }
        // 409 -> cmd 11306: `/api/voicecall/endcall`
        409 => {
            let body = json!({
                "uidTo": n.get("toId").or_else(|| n.get("uidTo")).cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "status": 3,
                "imei": s.imei,
            });
            voicecall_get(s, "/api/voicecall/endcall", &body).await
        }
        // 411 -> cmd 11307: `/api/voicecall/holdreq`
        411 => {
            let body = json!({ "toId": n.get("toId").cloned().unwrap_or(json!("")) });
            voicecall_get(s, "/api/voicecall/holdreq", &body).await
        }
        // 412 -> cmd 11308: `/api/voicecall/holdresp`
        412 => {
            let body = json!({ "toId": n.get("toId").cloned().unwrap_or(json!("")) });
            voicecall_get(s, "/api/voicecall/holdresp", &body).await
        }
        // 413 -> cmd 11309: `/api/voicecall/unholdreq`
        413 => {
            let body = json!({ "toId": n.get("toId").cloned().unwrap_or(json!("")) });
            voicecall_get(s, "/api/voicecall/unholdreq", &body).await
        }
        // 414 -> cmd 11310: `/api/voicecall/unholdresp`
        414 => {
            let body = json!({ "toId": n.get("toId").cloned().unwrap_or(json!("")) });
            voicecall_get(s, "/api/voicecall/unholdresp", &body).await
        }
        // 406 -> cmd 11311: `/api/voicecall/logendcall`
        406 => {
            let body = json!({
                "call_id": n.get("callId").cloned().unwrap_or(json!(0)),
                "partner_id": n.get("partnerId").cloned().unwrap_or(json!("")),
                "status": n.get("status").cloned().unwrap_or(json!(0)),
                "logdata": n.get("data").cloned().unwrap_or(json!("")),
                "duration": n.get("duration").cloned().unwrap_or(json!(0)),
                "role": n.get("role").cloned().unwrap_or(json!(0)),
                "imei": s.imei,
            });
            voicecall_get(s, "/api/voicecall/logendcall", &body).await
        }
        // 12006 -> `/api/voicecall/conf` (cam/mic toggle & audio-to-video upgrade)
        12006 => {
            let body = json!({
                "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "isOnCam": n.get("isOnCam").cloned().unwrap_or(json!(0)),
                "isOnMic": n.get("isOnMic").cloned().unwrap_or(json!(1)),
                "data": n.get("data").cloned().unwrap_or(json!("")),
            });
            voicecall_get(s, "/api/voicecall/conf", &body).await
        }
        // 12064 -> `/api/voicecall/requestsharescreen`
        12064 => {
            let body = json!({
                "uidTo": n.get("uidTo").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "status": n.get("status").cloned().unwrap_or(json!(0)),
            });
            voicecall_get(s, "/api/voicecall/requestsharescreen", &body).await
        }
        // 12433 -> `/api/voicecall/group/requestcall`
        12433 => {
            let body = json!({
                "groupId": n.get("groupId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "typeRequest": n.get("typeRequest").cloned().unwrap_or(json!(5)),
                "data": n.get("data").cloned().unwrap_or(json!("")),
                "partners": n.get("partners").cloned().unwrap_or(json!([])),
            });
            voicecall_get(s, "/api/voicecall/group/requestcall", &body).await
        }
        // 12434 -> `/api/voicecall/group/request`
        12434 => {
            let body = json!({
                "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "callType": n.get("callType").cloned().unwrap_or(json!(5)),
                "data": n.get("data").cloned().unwrap_or(json!("")),
                "session": n.get("session").cloned().unwrap_or(json!("")),
                "partners": n.get("partners").cloned().unwrap_or(json!([])),
                "groupId": n.get("groupId").cloned().unwrap_or(json!("")),
            });
            voicecall_get(s, "/api/voicecall/group/request", &body).await
        }
        // 12435 -> `/api/voicecall/group/adduser`
        12435 => {
            let body = json!({
                "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "callType": n.get("callType").cloned().unwrap_or(json!(5)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "data": n.get("data").cloned().unwrap_or(json!("")),
                "session": n.get("session").cloned().unwrap_or(json!("")),
                "partners": n.get("partners").cloned().unwrap_or(json!([])),
            });
            voicecall_get(s, "/api/voicecall/group/adduser", &body).await
        }
        // 12436 -> `/api/voicecall/group/answer`
        12436 => {
            let body = json!({
                "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "callType": n.get("callType").cloned().unwrap_or(json!(5)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "data": n.get("data").cloned().unwrap_or(json!("")),
                "partners": n.get("partners").cloned().unwrap_or(json!([])),
            });
            voicecall_get(s, "/api/voicecall/group/answer", &body).await
        }
        // 12437 -> `/api/voicecall/group/cancel`
        12437 => {
            let body = json!({
                "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "callType": n.get("callType").cloned().unwrap_or(json!(5)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "data": n.get("data").cloned().unwrap_or(json!("")),
                "partners": n.get("partners").cloned().unwrap_or(json!([])),
            });
            voicecall_get(s, "/api/voicecall/group/cancel", &body).await
        }
        // 12438 -> `/api/voicecall/group/endcall`
        12438 => {
            let body = json!({
                "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "callType": n.get("callType").cloned().unwrap_or(json!(5)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "data": n.get("data").cloned().unwrap_or(json!("")),
                "partners": n.get("partners").cloned().unwrap_or(json!([])),
            });
            voicecall_get(s, "/api/voicecall/group/endcall", &body).await
        }
        // 12439 -> `/api/voicecall/group/ringring`
        12439 => {
            let body = json!({
                "calleeId": n.get("calleeId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "callType": n.get("callType").cloned().unwrap_or(json!(5)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "data": n.get("data").cloned().unwrap_or(json!("")),
                "partners": n.get("partners").cloned().unwrap_or(json!([])),
            });
            voicecall_get(s, "/api/voicecall/group/ringring", &body).await
        }
        // 12445 -> `/api/voicecall/group/state`
        12445 => {
            let body = json!({
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "callType": n.get("callType").cloned().unwrap_or(json!(5)),
            });
            voicecall_get(s, "/api/voicecall/group/state", &body).await
        }
        // 12446 -> `/api/voicecall/group/finish`
        12446 => {
            let body = json!({
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "callType": n.get("callType").cloned().unwrap_or(json!(5)),
                "data": n.get("data").cloned().unwrap_or(json!("")),
                "duration": n.get("duration").cloned().unwrap_or(json!(0)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "partners": n.get("partners").cloned().unwrap_or(json!([])),
                "status": n.get("status").cloned().unwrap_or(json!(0)),
                "joinTime": n.get("joinTime").cloned().unwrap_or(json!(0)),
            });
            voicecall_get(s, "/api/voicecall/group/finish", &body).await
        }
        // 12447 -> `/api/voicecall/group/notify`
        12447 => {
            let body = json!({
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "callType": n.get("callType").cloned().unwrap_or(json!(5)),
                "status": n.get("status").cloned().unwrap_or(json!(0)),
                "data": n.get("data").cloned().unwrap_or(json!("")),
            });
            voicecall_get(s, "/api/voicecall/group/notify", &body).await
        }
        // 12097 -> `/api/voicecall/group/ping`
        12097 => {
            let body = json!({
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "userId": n.get("userId").cloned().unwrap_or(json!("")),
                "callstate": n.get("callState").cloned().unwrap_or(json!(0)),
                "audiostate": n.get("audioState").cloned().unwrap_or(json!(0)),
                "videostate": n.get("videoState").cloned().unwrap_or(json!(0)),
                "params": n.get("params").cloned().unwrap_or(json!("")),
            });
            voicecall_get(s, "/api/voicecall/group/ping", &body).await
        }
        // 12098 -> `/api/voicecall/group/broadcast`
        12098 => {
            let body = json!({
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "userId": n.get("userId").cloned().unwrap_or(json!("")),
                "callstate": n.get("callState").cloned().unwrap_or(json!(0)),
                "audiostate": n.get("audioState").cloned().unwrap_or(json!(0)),
                "videostate": n.get("videoState").cloned().unwrap_or(json!(0)),
                "params": n.get("params").cloned().unwrap_or(json!("")),
            });
            voicecall_get(s, "/api/voicecall/group/broadcast", &body).await
        }
        // 12043 -> `/api/voicecall/group/join`
        12043 => {
            let body = json!({
                "userId": n.get("userId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "status": n.get("status").cloned().unwrap_or(json!(0)),
                "srcJoinCall": n.get("srcJoinCall").cloned().unwrap_or(json!(0)),
                "audiostate": n.get("audioState").cloned().unwrap_or(json!(0)),
                "videostate": n.get("videoState").cloned().unwrap_or(json!(0)),
                "data": n.get("data").cloned().unwrap_or(json!("")),
            });
            voicecall_get(s, "/api/voicecall/group/join", &body).await
        }
        // 12044 -> `/api/voicecall/group/sharescreen`
        12044 => {
            let body = json!({
                "userId": n.get("userId").cloned().unwrap_or(json!("")),
                "callId": n.get("callId").cloned().unwrap_or(json!(0)),
                "hostCall": n.get("hostCall").cloned().unwrap_or(json!("")),
                "status": n.get("status").cloned().unwrap_or(json!(0)),
                "audiostate": n.get("audioState").cloned().unwrap_or(json!(0)),
                "videostate": n.get("videoState").cloned().unwrap_or(json!(0)),
                "data": n.get("data").cloned().unwrap_or(json!("")),
            });
            voicecall_get(s, "/api/voicecall/group/sharescreen", &body).await
        }
        other => bail!("unsupported call sendSignal command: {other}"),
    }
}

// ---------------------------------------------------------------------------
// Incoming VoIP Control Polling (`getControllFromPolling`, cmd `99101`)
// ---------------------------------------------------------------------------

/// State machine for `lp_control` (`p4-ctl-chat.zaloapp.com`, `queueIds: ["4", "5"]`).
///
/// Serialized as-is into the per-account sync file so a later session resumes from the stored
/// cursor instead of replaying the whole control queue. `#[serde(default)]` keeps files written
/// before a field existed loadable (missing fields come from the [`Default`] impl below).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CtrlPollState {
    pub last_action_id: String,
    pub last_action_id_other: String,
    pub queues: BTreeMap<String, QueueCursor>,
}

impl Default for CtrlPollState {
    fn default() -> Self {
        let mut queues = BTreeMap::new();
        // Queue ids the official PC client tracks for `lp_ctrl` (`evict_broadcast_queues.lp_ctrl`).
        //
        // `ZCALL_QUEUES=a,b,c` replaces the set (dev knob). Captured ground truth: the official
        // desktop subscribes to `603_1`, `603_0`, `604_0`, and the full media-carrying
        // `act:"request"` invite only arrives on those; the generic set below yields only the
        // ~716-byte `act:"answer"` stub.
        let ids: Vec<String> = std::env::var("ZCALL_QUEUES")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| {
                ["4", "5", "14", "17", "18", "19", "20"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });
        for qid in ids {
            queues.insert(
                qid,
                QueueCursor {
                    last_id: "1".into(),
                    ids: Vec::new(),
                },
            );
        }
        Self {
            last_action_id: "1".into(),
            last_action_id_other: "1".into(),
            queues,
        }
    }
}

impl CtrlPollState {
    pub fn encode_params(&self) -> String {
        use base64::Engine;
        let q_json = serde_json::to_string(&self.queues).unwrap_or_else(|_| "{}".into());
        let b64 = base64::engine::general_purpose::STANDARD.encode(q_json.as_bytes());
        let tail = crate::session::urlencode(&format!("{b64},1"));
        format!(
            "{},{},{}",
            self.last_action_id, self.last_action_id_other, tail
        )
    }

    pub fn update_from_response(&mut self, data_node: &Value) {
        if let Some(reset_id) = data_node.get("resetLastActionId").and_then(Value::as_str) {
            if reset_id != "0" && reset_id.parse::<u128>().unwrap_or(0) > 0 {
                self.last_action_id = reset_id.to_string();
                self.last_action_id_other = "1".into();
                for q in self.queues.values_mut() {
                    q.last_id = reset_id.to_string();
                    q.ids.clear();
                }
                return;
            }
        }
        if let Some(lai) = data_node.get("lastActionId").and_then(Value::as_str) {
            if lai != "0" && lai.parse::<u128>().unwrap_or(0) > self.last_action_id.parse::<u128>().unwrap_or(0) {
                self.last_action_id = lai.to_string();
            }
        }
        if let Some(qs) = data_node.get("queueStatus").and_then(Value::as_object) {
            for (qid, qval) in qs {
                let entry = self.queues.entry(qid.clone()).or_insert_with(|| QueueCursor {
                    last_id: "1".into(),
                    ids: Vec::new(),
                });
                if let Some(lid) = qval.get("lastId").and_then(Value::as_str) {
                    if lid != "0" && lid.parse::<u128>().unwrap_or(0) > entry.last_id.parse::<u128>().unwrap_or(0) {
                        entry.last_id = lid.to_string();
                    }
                }
                // The server keeps re-delivering a queue entry until the client echoes its
                // action id back in `ids`. `getIdsForParam()` does this upstream, with at most
                // the 10 newest ids: echoing every id of a long first reply made each later
                // poll fail with 413 (`re-notes/10` §18).
                if let Some(ids) = qval.get("ids").and_then(Value::as_array) {
                    entry.ids = crate::chat::newest_action_ids(ids, &entry.last_id);
                }
            }
        }
    }
}

/// Control events carry their media handshake as a JSON **string** inside `data.params`
/// (`sessId`, `rtpIP`, `codec`, `extendData`, ...). Return it parsed for callers that need
/// to answer an invite.
pub fn event_media_params(data: &Value) -> Value {
    let Some(raw) = data.get("params").and_then(Value::as_str) else {
        return Value::Null;
    };
    let trimmed = raw.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return Value::Null;
    }
    serde_json::from_str::<Value>(trimmed).unwrap_or(Value::Null)
}

/// Extract `voip` control events from a decrypted `getControllFromPolling` payload (`_processCtrlMessages`).
pub fn extract_voip_controls(data_node: &Value) -> Vec<Value> {
    let mut voip_events = Vec::new();
    let Some(controls) = data_node.get("controls").and_then(Value::as_array) else {
        return voip_events;
    };
    for item in controls {
        let Some(mut content) = item.get("content").cloned() else {
            continue;
        };
        let act_type = content
            .get("act_type")
            .and_then(Value::as_str)
            .unwrap_or("");
        if act_type != "voip" {
            continue;
        }
        if let Some(data_str) = content.get("data").and_then(Value::as_str) {
            let trimmed = data_str.trim();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                    if let Some(obj) = content.as_object_mut() {
                        obj.insert("data".into(), parsed);
                    }
                }
            }
        }
        voip_events.push(content);
    }
    voip_events
}

/// One "đang nhập" (typing) notice decoded from a `lp_control` / socket-`602` control event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypingEvent {
    pub thread_id: String,
    pub name: String,
    pub is_typing: bool,
}

/// Extract `typing` controls from a decrypted `getControllFromPolling` payload (socket `602`).
///
/// Same input shape as [`extract_voip_controls`]: `data_node["controls"]`, where each item may
/// carry a `content` object or a JSON-encoded string that has to be decoded first. Controls that
/// are not typing (including `act_type == "voip"`, which shares this array) and malformed items
/// are skipped; nothing here can panic on a malformed reply.
///
/// [I] The exact field names are inferred — only one captured shape is available — so several
/// spellings are accepted until a live sample pins them down:
/// - `act` / `act_type` `"typing"` starts a notice, `"typing_end"` stops it.
/// - conversation id: `toid` first for a 1-1 chat, `id` first for a group (aliases `threadId`,
///   `conversationId`, `groupId`). Group ids are normalized by stripping the `group_`/`g` prefix,
///   like the rest of this module does for group conversations.
/// - sender name: `dName`, then `name`, then `fromName`.
/// - `is_typing`: an explicit `typing` (or inverse `expired`) field wins, otherwise the notice is
///   "typing" unless it is a `typing_end`.
pub fn extract_typing_events(data_node: &Value) -> Vec<TypingEvent> {
    let mut out = Vec::new();
    let Some(controls) = data_node.get("controls").and_then(Value::as_array) else {
        return out;
    };
    for item in controls {
        let Some(content) = control_content(item) else {
            continue;
        };
        let act = str_at(&content, &["act"]);
        let data = control_data(&content);
        let act_type = str_at(&data, &["act_type"]).or_else(|| str_at(&content, &["act_type"]));
        // VoIP controls share the same array; they are not typing notices.
        if act.as_deref() == Some("voip") || act_type.as_deref() == Some("voip") {
            continue;
        }
        let typing_end =
            act.as_deref() == Some("typing_end") || act_type.as_deref() == Some("typing_end");
        let typing = act.as_deref() == Some("typing") || act_type.as_deref() == Some("typing");
        if !typing && !typing_end {
            continue;
        }
        let is_group = typing_is_group(&data, act.as_deref(), act_type.as_deref());
        let raw_id = typing_thread_id(&data, is_group);
        if raw_id.is_empty() {
            continue;
        }
        let name = str_at(&data, &["dName", "name", "fromName"])
            .or_else(|| str_at(&content, &["dName", "name", "fromName"]))
            .unwrap_or_default();
        let is_typing = match flag_at(&data, "typing") {
            Some(v) => v,
            None => match flag_at(&data, "expired") {
                Some(v) => !v,
                None => !typing_end,
            },
        };
        out.push(TypingEvent {
            thread_id: normalize_group_id(&raw_id),
            name,
            is_typing,
        });
    }
    out
}

/// `item["content"]`: either the object itself or a JSON string that has to be decoded first.
fn control_content(item: &Value) -> Option<Value> {
    let raw = item.get("content")?;
    match raw {
        Value::Object(_) => Some(raw.clone()),
        Value::String(s) => {
            let trimmed = s.trim();
            if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
                return None;
            }
            serde_json::from_str::<Value>(trimmed)
                .ok()
                .filter(Value::is_object)
        }
        _ => None,
    }
}

/// `content["data"]` as an object; a JSON string is decoded, and a missing/non-object value falls
/// back to the control itself so fields carried at the top level still work.
fn control_data(content: &Value) -> Value {
    match content.get("data") {
        Some(Value::Object(_)) => content.get("data").cloned().unwrap_or(Value::Null),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                    if parsed.is_object() {
                        return parsed;
                    }
                }
            }
            content.clone()
        }
        _ => content.clone(),
    }
}

/// Non-empty string (or numeric id rendered as text) at the first key that has one.
fn str_at(node: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        match node.get(*key) {
            Some(Value::String(s)) if !s.trim().is_empty() => return Some(s.trim().to_string()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            _ => {}
        }
    }
    None
}

/// Boolean-ish JSON value: `true`/`false`, `0`/`1`, `"0"`/`"1"`, `"true"`/`"false"`.
fn flag_at(node: &Value, key: &str) -> Option<bool> {
    bool_of(node.get(key)?)
}

/// Boolean-ish JSON value (see [`flag_at`]).
fn bool_of(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|x| x != 0),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Some(true),
            "0" | "false" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Integer-ish JSON value: a number (signed or unsigned that fits `i64`) or a decimal string.
fn i64_of(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_u64().and_then(|u| i64::try_from(u).ok())),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// [I] A typing control belongs to a group when it says so (`isGroup`, `groupId`) or when its act
/// tag mentions `group`; otherwise it is a 1-1 conversation.
fn typing_is_group(data: &Value, act: Option<&str>, act_type: Option<&str>) -> bool {
    if flag_at(data, "isGroup") == Some(true) {
        return true;
    }
    if str_at(data, &["groupId"]).is_some() {
        return true;
    }
    act.or(act_type)
        .map(|tag| tag.contains("group"))
        .unwrap_or(false)
}

/// [I] `toid` is the 1-1 conversation id and `id` is the group id; when both are present the
/// group/1-1 order decides which one wins. Aliases are accepted as a fallback.
fn typing_thread_id(data: &Value, is_group: bool) -> String {
    let keys: &[&str] = if is_group {
        &["id", "groupId", "toid", "threadId", "conversationId"]
    } else {
        &["toid", "threadId", "conversationId", "id"]
    };
    str_at(data, keys).unwrap_or_default()
}

/// Strip a `group_` / `group` / `g` prefix from a group conversation id, like the rest of this
/// module. Numeric ids (1-1 and already-normalized group ids) are returned unchanged.
fn normalize_group_id(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = trimmed
        .trim_start_matches("group_")
        .trim_start_matches("group")
        .trim_start_matches(['g', 'G']);
    if stripped.is_empty() {
        trimmed.to_string()
    } else {
        stripped.to_string()
    }
}

/// Structural summary of a `syncmsg_info` backup description.
///
/// **Shape only, never values:** `url` may carry a one-shot token in its query string and
/// `encrypted_key`/`checksum` are secret material, so this struct keeps lengths, presence flags
/// and a bare URL host — no token, no query, no key bytes. [I] The field names come from
/// `re-notes/11` §B3.2; they are only a reading aid for `--dev` logs until a real transfer event
/// is observed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackupInfo {
    /// UTF-8 byte length of `file_name`.
    pub file_name_len: usize,
    pub url_present: bool,
    /// Host of `url` only (scheme/path/query/fragment/userinfo/port removed).
    pub url_host: Option<String>,
    /// `checksum_code` or `checksum` present and non-empty.
    pub checksum_present: bool,
    /// Byte length of `encrypted_key` (the value itself is never retained).
    pub encrypted_key_len: usize,
    pub db_format: Option<i64>,
    pub is_full_transfer: Option<bool>,
    pub from_seq_id: Option<i64>,
    pub min_seq_id: Option<i64>,
    /// `Some(true/false)` only when `pc_name` was present; compared raw and trimmed.
    pub pc_name_matches_session: Option<bool>,
    /// `Some(true/false)` only when `public_key` was present; compared raw and trimmed.
    pub public_key_matches_session: Option<bool>,
}

/// One phone→PC data-transfer control event, decoded structurally (nothing is requested or
/// decrypted here).
///
/// `act` is kept verbatim: `user_confirm`, `transfer_error`, `syncmsg_info`,
/// `transfer_after_login`. The control's `act_type` remains **[U]**: it has not been observed on a
/// real transfer event, so filtering goes by `act` only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferEvent {
    pub act: String,
    /// Conversation hint (`toid` for 1-1, `id` for a group, prefix-normalized) when present.
    pub thread_hint: Option<String>,
    /// `user_confirm`: `0` declined, `1`/`3` accepted, `2` restoring.
    pub user_action: Option<i64>,
    /// `transfer_error`: phone state (`1` active, `2` idle).
    pub status: Option<i64>,
    /// `transfer_error`: non-zero means the phone/PC reported a failure.
    pub error_code: Option<i64>,
    /// `transfer_after_login`: whether a `temp_key` was carried (its value is never kept).
    pub temp_key_present: bool,
    /// `syncmsg_info`: structural summary of the backup description.
    pub backup: Option<BackupInfo>,
}

/// How a transfer control's `act` may be spelled; anything else is not a transfer event.
const TRANSFER_ACTS: [&str; 4] = [
    "user_confirm",
    "transfer_error",
    "syncmsg_info",
    "transfer_after_login",
];

/// Extract phone→PC transfer controls from a decrypted `getControllFromPolling` payload.
///
/// Same input shape and helpers as [`extract_typing_events`]/[`extract_voip_controls`]:
/// `data_node["controls"]`, each item's `content` may be an object or a JSON-encoded string.
/// This is a read-only reader for `--dev` logging: it issues no request, decrypts nothing, and
/// keeps only the structural summary in [`BackupInfo`].
///
/// `pc_name`/`public_key` are compared with the current session exactly and after trimming, the
/// way `handleCtrlEvents` of Zalo PC drops events addressed to another PC.
///
/// [I] Field names are taken from `re-notes/11` §B3.2; `act_type` is [U] until a real event is
/// captured, so classification uses `act` only.
pub fn extract_transfer_events(
    data_node: &Value,
    session_pc_name: &str,
    session_public_key: &str,
) -> Vec<TransferEvent> {
    let mut out = Vec::new();
    let Some(controls) = data_node.get("controls").and_then(Value::as_array) else {
        return out;
    };
    for item in controls {
        let Some(content) = control_content(item) else {
            continue;
        };
        let data = control_data(&content);
        let Some(act) = str_at(&content, &["act"]).or_else(|| str_at(&data, &["act"])) else {
            continue;
        };
        if !TRANSFER_ACTS.contains(&act.as_str()) {
            continue;
        }

        let at = |key: &str| data.get(key).or_else(|| content.get(key));
        let is_group = typing_is_group(&data, Some(act.as_str()), None);
        let raw_thread = typing_thread_id(&data, is_group);
        let thread_hint = (!raw_thread.is_empty()).then(|| normalize_group_id(&raw_thread));

        let event = TransferEvent {
            user_action: at("user_action").and_then(i64_of),
            status: at("status").and_then(i64_of),
            error_code: at("error_code").and_then(i64_of),
            temp_key_present: at("temp_key").map(non_empty_value).unwrap_or(false),
            thread_hint,
            backup: (act == "syncmsg_info").then(|| {
                backup_info(&data, &content, session_pc_name, session_public_key)
            }),
            act,
        };
        out.push(event);
    }
    out
}

/// Structural summary of one `syncmsg_info` payload (see [`BackupInfo`]).
fn backup_info(
    data: &Value,
    content: &Value,
    session_pc_name: &str,
    session_public_key: &str,
) -> BackupInfo {
    let at = |key: &str| data.get(key).or_else(|| content.get(key));
    let text = |key: &str| at(key).and_then(Value::as_str).unwrap_or("");
    let url = text("url");
    BackupInfo {
        file_name_len: text("file_name").len(),
        url_present: !url.trim().is_empty(),
        url_host: url_host_only(url),
        checksum_present: ["checksum_code", "checksum"]
            .iter()
            .any(|key| at(key).map(non_empty_value).unwrap_or(false)),
        encrypted_key_len: text("encrypted_key").len(),
        db_format: at("db_format").and_then(i64_of),
        is_full_transfer: at("is_full_transfer").and_then(bool_of),
        from_seq_id: at("from_seq_id").and_then(i64_of),
        min_seq_id: at("min_seq_id").and_then(i64_of),
        pc_name_matches_session: matches_session(at("pc_name"), session_pc_name),
        public_key_matches_session: matches_session(at("public_key"), session_public_key),
    }
}

/// Is a JSON value present and non-empty? (`null` and `""` are not.)
fn non_empty_value(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::String(s) => !s.trim().is_empty(),
        _ => true,
    }
}

/// Compare a session-identifying field with the local session (exact, then trimmed).
///
/// `None` when the field is absent or not a string, so the caller can tell "not advertised" from
/// "does not match".
fn matches_session(field: Option<&Value>, session: &str) -> Option<bool> {
    let raw = field?.as_str()?;
    Some(raw == session || raw.trim() == session.trim())
}

/// Host part of a URL, with scheme, path, query, fragment, userinfo and port removed.
///
/// `syncmsg_info.url` may carry a one-shot token in its query string, so the event log must never
/// retain the URL: only the host leaves this function. Hand-parsed on purpose (the crate has no
/// URL parser); anything without a host yields `None`, which keeps the value out of the log.
fn url_host_only(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let rest = match trimmed.split_once("://") {
        Some((_, rest)) => rest,
        None if trimmed.starts_with("//") => &trimmed[2..],
        None => return None,
    };
    let authority = rest
        .split(|c| matches!(c, '/' | '?' | '#'))
        .next()
        .unwrap_or("");
    // Drop `user[:pass]@` if present.
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    if host_port.is_empty() {
        return None;
    }
    let host = if let Some(v6) = host_port.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    let host = host.trim();
    (!host.is_empty()).then(|| host.to_string())
}

/// One `lp_control` reply: its VoIP control events and the whole `data` node (which also
/// carries `reacts` / `reactGroups`, `re-notes/10` §17).
pub struct CtrlPoll {
    pub voip: Vec<Value>,
    pub data: Value,
}

/// Poll `lp_control` (`getControllFromPolling`, cmd `99101`) once and return any incoming VoIP control events.
pub async fn poll_voip_controls(s: &Session, state: &mut CtrlPollState) -> Result<Vec<Value>> {
    poll_controls(s, state).await.map(|p| p.voip)
}

/// Poll `lp_control` once. The first poll with cursor `"1"` replays history: its VoIP events are
/// dropped, but `data` is still returned (reactions since login are worth applying).
pub async fn poll_controls(s: &Session, state: &mut CtrlPollState) -> Result<CtrlPoll> {
    if s.login.is_none() {
        bail!("chưa đăng nhập");
    }
    let is_initial_cursor = state.last_action_id == "1" || state.last_action_id == "0";
    let host = if s.domains.lp_control.is_empty() {
        "https://p4-ctl-chat.zaloapp.com"
    } else {
        &s.domains.lp_control
    };
    let ts = crate::login::unix_now();
    let url = format!(
        "{}/?{}&params={}&ts={ts}",
        host.trim_end_matches('/'),
        s.common_params(),
        state.encode_params()
    );
    let resp = s.get(&url).await?;
    let decrypted = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if crate::trace_shape::enabled() {
        // Every control action, not only VoIP: the sync investigation looks for transfer /
        // "after login" events here (including the initial-cursor replay that is discarded below).
        crate::trace_shape::line(
            "lpc",
            format!(
                "lastActionId={} initial={is_initial_cursor} <- {}",
                state.last_action_id,
                crate::trace_shape::shape(&decrypted)
            ),
        );
    }
    let mut data_node = decrypted.get("data").unwrap_or(&decrypted);
    if let Some(inner) = data_node.get("data").filter(|v| v.is_object()) {
        data_node = inner;
    }
    state.update_from_response(data_node);
    if is_initial_cursor {
        // First poll with cursor "1" only syncs `lastActionId` to the server's latest cursor;
        // discard historical replay events from previous sessions.
        return Ok(CtrlPoll { voip: Vec::new(), data: data_node.clone() });
    }
    let events = extract_voip_controls(data_node);
    if !events.is_empty() {
        trace_line(
            "CTRL",
            format!(
                "n={} lastActionId={} raw={}",
                events.len(),
                state.last_action_id,
                data_node
            ),
        );
    }
    Ok(CtrlPoll { voip: events, data: data_node.clone() })
}

// ---------------------------------------------------------------------------
// Pure-Rust In-App VoIP Signaling & ZRTP Relay Engine (Zero external `ZaloCall.exe` process)
// ---------------------------------------------------------------------------

struct PureRustCallState {
    call_id: i64,
    partner_id: String,
    partner_name: String,
    is_video: bool,
    is_group: bool,
    is_caller: bool,
    is_connected: bool,
    started_at: std::time::Instant,
    sess_id: String,
    rtp_ip: String,
    rtcp_ip: String,
    extend_data: String,
    from_uid_u32: u32,
    partner_uid_u32: u32,
    relay_token: u32,
    server_tokens: BTreeMap<String, u32>,
    udp_rtp_sock: Option<Arc<tokio::net::UdpSocket>>,
    udp_rtcp_sock: Option<Arc<tokio::net::UdpSocket>>,
    keepalive_abort: Option<tokio::sync::watch::Sender<bool>>,
    /// Phase 1 audio session (mic->speaker pass-through). None until media starts.
    audio: Option<crate::audio::AudioHandle>,
    /// Phase 2 media session (Opus/RTP over UDP). None until connected.
    media: Option<crate::media::MediaHandle>,
}

impl Drop for PureRustCallState {
    fn drop(&mut self) {
        if let Some(tx) = self.keepalive_abort.take() {
            let _ = tx.send(true);
        }
    }
}

/// Convert a decimal 64-bit Zalo user ID string to the lower 32-bit `u32` ID used in ZRTP UDP headers (`std::stoull` cast to `uint32_t`).
pub fn uid_str_to_u32(uid: &str) -> u32 {
    uid.trim().parse::<u64>().unwrap_or(0) as u32
}

/// Build the binary 21-byte common ZRTP UDP request header (`0x00892F43` in `ZaloCall.exe`).
fn write_zrtp_common_header(
    buf: &mut Vec<u8>,
    payload_type: u8,
    seq_or_ts: u32,
    ssrc_or_seq: u32,
    from_id: u32,
    call_or_magic: u32,
    cmd: u16,
    sub_cmd: u8,
) {
    buf.push(0x01); // [esi+0x04] magic = 1
    buf.push(payload_type); // [esi+0x05] 0x7E for InitZRTP, 0x01 for Ping/Echo/End
    buf.extend_from_slice(&seq_or_ts.to_le_bytes()); // [esi+0x08]
    buf.extend_from_slice(&ssrc_or_seq.to_le_bytes()); // [esi+0x0C]
    buf.extend_from_slice(&from_id.to_le_bytes()); // [esi+0x10]
    buf.extend_from_slice(&call_or_magic.to_le_bytes()); // [esi+0x14]
    buf.extend_from_slice(&cmd.to_le_bytes()); // [esi+0x18]
    buf.push(sub_cmd); // [esi+0x1A]
}

/// Build `_sendRequestInitZRTPToServer` UDP packet (`0x0089419C` -> `0x008931D9` in `ZaloCall.exe`).
/// - `is_caller == true`  -> `cmd = 11 = 0x000B` (`0x008941BB: xor al, 1; add ax, 0x0B`)
/// - `is_caller == false` -> `cmd = 12 = 0x000C`
/// - `sub_cmd`: `2` for `protocol = 3` (requests `"public_ip|public_port"` in reply), `1` otherwise.
/// - Body order (`0x008931D9..0x008931EF`): `[esi+0x34] = call_id` FIRST, `[esi+0x38] = partner_uid` SECOND,
///   followed by `u16 LE` length + `sess_id`.
pub fn build_zrtp_init_packet(
    is_caller: bool,
    from_uid: u32,
    partner_uid: u32,
    call_id: u32,
    sub_cmd: u8,
    sess_id: &str,
) -> Vec<u8> {
    let cmd: u16 = if is_caller { 0x000B } else { 0x000C };
    let sess_bytes = sess_id.as_bytes();
    let mut buf = Vec::with_capacity(21 + 10 + sess_bytes.len());
    write_zrtp_common_header(&mut buf, 0x7E, 0, 0, from_uid, 0, cmd, sub_cmd);
    buf.extend_from_slice(&call_id.to_le_bytes()); // [esi+0x34] = callId
    buf.extend_from_slice(&partner_uid.to_le_bytes()); // [esi+0x38] = partnerId
    buf.extend_from_slice(&(sess_bytes.len() as u16).to_le_bytes()); // write_str_u16_le (0x00892D42)
    buf.extend_from_slice(sess_bytes);
    buf
}

/// Build `_sendRequestZRTPPing` UDP packet (`0x00894388` -> `0x00893126` in `ZaloCall.exe`).
/// - `cmd = 0x0002`, `sub_cmd = 1`
pub fn build_zrtp_ping_packet(from_uid: u32, call_id: u32, sess_id: &str) -> Vec<u8> {
    let sess_bytes = sess_id.as_bytes();
    let mut buf = Vec::with_capacity(21 + 2 + sess_bytes.len());
    write_zrtp_common_header(&mut buf, 0x01, 0, 0, from_uid, call_id, 0x0002, 0x01);
    buf.extend_from_slice(&(sess_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(sess_bytes);
    buf
}

/// Build `_sendEchoRequestToServers` UDP packet (`0x00894003` -> `0x008930C5` in `ZaloCall.exe`).
/// - `cmd = 0x0005`, `sub_cmd = 0`, magic `fromId = 0x003E9765`, `callId = 0x012F833C`.
pub fn build_zrtp_echo_packet(seq: u32, ts_str: &str) -> Vec<u8> {
    let ts_bytes = ts_str.as_bytes();
    let mut buf = Vec::with_capacity(21 + 2 + ts_bytes.len());
    write_zrtp_common_header(
        &mut buf,
        0x01,
        0,
        seq,
        0x003E9765,
        0x012F833C,
        0x0005,
        0x00,
    );
    buf.extend_from_slice(&(ts_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(ts_bytes);
    buf
}

/// Build `_sendRequestZRTPEndCall` UDP packet (`0x0089409A` -> `0x00893142` in `ZaloCall.exe`).
/// - `cmd = 0x0020`, `sub_cmd = 0x09`, `sessId` prefixed by `u32 LE` length (`write_str_u32_le`).
pub fn build_zrtp_end_packet(from_uid: u32, call_id: u32, sess_id: &str) -> Vec<u8> {
    let sess_bytes = sess_id.as_bytes();
    let mut buf = Vec::with_capacity(21 + 4 + sess_bytes.len());
    write_zrtp_common_header(&mut buf, 0x01, 0, 0, from_uid, call_id, 0x0020, 0x09);
    buf.extend_from_slice(&(sess_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(sess_bytes);
    buf
}

/// Collect all candidate `(rtp_addr, rtcp_addr)` IPv4 relay server pairs from a `401` or `request` payload.
fn collect_relay_servers(payload: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(arr) = payload.get("servers").and_then(Value::as_array) {
        for srv in arr {
            let rtp = srv
                .get("rtpaddr")
                .or_else(|| srv.get("rtp"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let rtcp = srv
                .get("rtcpaddr")
                .or_else(|| srv.get("rtcp"))
                .and_then(Value::as_str)
                .unwrap_or(&rtp)
                .trim()
                .to_string();
            if !rtp.is_empty() && !out.iter().any(|(r, _)| r == &rtp) {
                out.push((rtp, rtcp));
            }
        }
    }
    if let Some(ext_str) = payload.get("extendData").and_then(Value::as_str) {
        if let Ok(ext_json) = serde_json::from_str::<Value>(ext_str.trim()) {
            if let Some(arr) = ext_json.get("serverAddr").and_then(Value::as_array) {
                for srv in arr {
                    let rtp = srv
                        .get("rtp")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let rtcp = srv
                        .get("rtcp")
                        .and_then(Value::as_str)
                        .unwrap_or(&rtp)
                        .trim()
                        .to_string();
                    if !rtp.is_empty() && !out.iter().any(|(r, _)| r == &rtp) {
                        out.push((rtp, rtcp));
                    }
                }
            }
        }
    }
    let top_rtp = payload
        .get("rtpIP")
        .or_else(|| payload.get("rtpAddress"))
        .or_else(|| payload.get("rtpSerIp"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let top_rtcp = payload
        .get("rtcpIP")
        .or_else(|| payload.get("rtcpAddress"))
        .and_then(Value::as_str)
        .unwrap_or(&top_rtp)
        .trim()
        .to_string();
    if !top_rtp.is_empty() && !out.iter().any(|(r, _)| r == &top_rtp) {
        out.push((top_rtp, top_rtcp));
    }
    out
}

/// Results gathered during the pre-call UDP Echo (`cmd=5`) and ZRTP Init (`cmd=12`/`11`) handshake.
#[derive(Debug, Clone, Default)]
pub struct ZrtpProbeResult {
    pub local_port: u16,
    /// Public `(ip, port)` observed by the relay server in its `InitZRTP` (`0x7E`, `cmd=12`/`11`) reply.
    pub public_p2p: Option<(String, u16)>,
    /// Per-relay `(recv_count, avg_rtt_ms)` keyed by RTP `"ip:port"`.
    pub echo_stats: BTreeMap<String, (i64, i64)>,
    /// Relay RTP `"ip:port"` that first answered `InitZRTP` with `res == 0`.
    pub selected_rtp: Option<String>,
    /// Token returned by the relay server in `InitZRTP`.
    pub token: u32,
    /// Per-relay token map (`"ip:port" -> token`) so the caller can switch to `rtpSerIp` on `answer`.
    pub server_tokens: BTreeMap<String, u32>,
}

/// Parse a relay server's UDP response to `InitZRTP` (`0x008934FA` -> `0x00893810` in `ZaloCall.exe`).
///
/// Layout after the 21-byte common header when `buf[0] == 0x02` (or `0x01`), `buf[1] in [0x7E, 0x7F]`,
/// `cmd in [0x000B, 0x000C]`:
/// - `21..25`: `res: i32 LE` (`0` = success, `-106` = invalid call/role)
/// - if `res == 0` and `buf.len() >= 33`:
///   - `25..29`: `call_id: u32 LE` (`[esi+0x34]`)
///   - `29..33`: `token: u32 LE` (`[esi+0x14]`)
///   - `33..35` (when `sub_cmd == 2`): `addr_len: u16 LE`
///   - `35..35+addr_len`: UTF-8 `"public_ip|public_port"` (or `"public_ip:public_port"`)
pub fn parse_zrtp_init_response(buf: &[u8]) -> Option<(i32, u32, u32, Option<(String, u16)>)> {
    if buf.len() < 25 || !matches!(buf[0], 0x01 | 0x02) || !matches!(buf[1], 0x7E | 0x7F) {
        return None;
    }
    let cmd = u16::from_le_bytes([buf[18], buf[19]]);
    if !matches!(cmd, 0x000B | 0x000C) {
        return None;
    }
    let res = i32::from_le_bytes([buf[21], buf[22], buf[23], buf[24]]);
    if res != 0 || buf.len() < 33 {
        return Some((res, 0, 0, None));
    }
    let ret_call_id = u32::from_le_bytes([buf[25], buf[26], buf[27], buf[28]]);
    let token = u32::from_le_bytes([buf[29], buf[30], buf[31], buf[32]]);
    let public_p2p = if buf.len() >= 35 {
        let addr_len = u16::from_le_bytes([buf[33], buf[34]]) as usize;
        if addr_len > 0 && buf.len() >= 35 + addr_len {
            let raw = std::str::from_utf8(&buf[35..35 + addr_len]).unwrap_or("").trim();
            raw.rsplit_once('|')
                .or_else(|| raw.rsplit_once(':'))
                .and_then(|(ip, p)| {
                    p.trim()
                        .parse::<u16>()
                        .ok()
                        .map(|port| (ip.trim().to_string(), port))
                })
        } else {
            None
        }
    } else {
        None
    };
    Some((res, ret_call_id, token, public_p2p))
}

/// Bind RTP & RTCP UDP sockets, run the Echo (`cmd=5`) + ZRTP Init (`cmd=12`/`11`) handshake
/// against the relay servers, collect public P2P & RTT measurements, and spawn the keepalive loop.
pub async fn start_zrtp_udp_relay_session_with_probe(
    payload: &Value,
    from_uid: u32,
    partner_uid: u32,
    call_id: u32,
    is_caller: bool,
    sess_id: &str,
) -> (
    Option<Arc<tokio::net::UdpSocket>>,
    Option<Arc<tokio::net::UdpSocket>>,
    Option<tokio::sync::watch::Sender<bool>>,
    ZrtpProbeResult,
) {
    let mut probe = ZrtpProbeResult {
        local_port: 60020,
        ..Default::default()
    };
    let rtp_sock = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => Arc::new(s),
        Err(_) => return (None, None, None, probe),
    };
    let rtcp_sock = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => Arc::new(s),
        Err(_) => rtp_sock.clone(),
    };
    let local_port = rtp_sock.local_addr().map(|a| a.port()).unwrap_or(60020);
    probe.local_port = local_port;

    let servers = collect_relay_servers(payload);
    // `protocol == 3` uses `sub_cmd = 2` (`0x008DAF8C`), which instructs the relay server
    // to include `"public_ip|public_port"` in its `0x02 0x7E` response.
    let sub_cmd: u8 = 2;

    let pkt_init =
        build_zrtp_init_packet(is_caller, from_uid, partner_uid, call_id, sub_cmd, sess_id);
    let ts_now = crate::login::unix_now().to_string();

    let t_start = std::time::Instant::now();
    let mut echo_seq: u32 = 1;
    // Send 14 echo probes per server (matching `settings.checkPkgRec = 14`) across 2 quick bursts,
    // plus the RTP (`sub_cmd=2`, `w3=0` in `CallController::_sendRequestInitZRTPToServer @ 0x4c18f8`) InitZRTP packet.
    // Never send `InitZRTP` or `Ping` from `rtcp_sock` (`sub_cmd | 4` is `loopbackMode == 2`, not `isRtcp`).
    for _ in 0..2 {
        for (rtp_addr, _rtcp_addr) in &servers {
            let _ = rtp_sock.send_to(&pkt_init, rtp_addr.as_str()).await;
            for _ in 0..7 {
                let pkt_echo = build_zrtp_echo_packet(echo_seq, &ts_now);
                echo_seq = echo_seq.wrapping_add(1);
                let _ = rtp_sock.send_to(&pkt_echo, rtp_addr.as_str()).await;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let mut recv_buf = [0u8; 1500];
    let mut rtcp_buf = [0u8; 1500];
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(280);
    let mut server_tokens: BTreeMap<String, u32> = BTreeMap::new();
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            res = rtp_sock.recv_from(&mut recv_buf) => {
                let Ok((n, peer)) = res else { continue; };
                let peer_str = peer.to_string();
                let rtt_ms = (t_start.elapsed().as_millis() as i64).max(1);
                if n >= 21 && recv_buf[0] == 0x01 && recv_buf[1] == 0x01 {
                    let cmd = u16::from_le_bytes([recv_buf[18], recv_buf[19]]);
                    if cmd == 0x0005 {
                        let entry = probe.echo_stats.entry(peer_str).or_insert((0, rtt_ms));
                        entry.0 += 1;
                        entry.1 = (entry.1 + rtt_ms) / 2;
                    }
                } else if let Some((code, ret_call, token, pub_addr)) = parse_zrtp_init_response(&recv_buf[..n]) {
                    trace_line(
                        "ZRTP",
                        format!("RTP init resp from {peer_str}: res={code} call={ret_call} token={token} pub={pub_addr:?}"),
                    );
                    if code == 0 {
                        server_tokens.insert(peer_str.clone(), token);
                        if probe.selected_rtp.is_none() {
                            probe.selected_rtp = Some(peer_str.clone());
                            probe.token = token;
                        }
                        if probe.public_p2p.is_none() && pub_addr.is_some() {
                            probe.public_p2p = pub_addr.clone();
                        }
                        let pkt_ping = build_zrtp_ping_packet(from_uid, token, sess_id);
                        let _ = rtp_sock.send_to(&pkt_ping, peer_str.as_str()).await;
                    }
                }
            }
            res = rtcp_sock.recv_from(&mut rtcp_buf) => {
                let Ok((n, peer)) = res else { continue; };
                let peer_str = peer.to_string();
                if let Some((code, ret_call, token, pub_addr)) = parse_zrtp_init_response(&rtcp_buf[..n]) {
                    trace_line(
                        "ZRTP",
                        format!("RTCP init resp from {peer_str}: res={code} call={ret_call} token={token} pub={pub_addr:?}"),
                    );
                    if code == 0 {
                        server_tokens.entry(peer_str.clone()).or_insert(token);
                        if probe.token == 0 {
                            probe.token = token;
                        }
                        // Only send ping from rtcp_sock when rtcp_addr differs from rtp_addr so a single-port
                        // relay does not overwrite the RTP socket's source port mapping with rtcp_sock's port.
                        if servers.iter().any(|(r, c)| c == &peer_str && r != c) {
                            let pkt_ping = build_zrtp_ping_packet(from_uid, token, sess_id);
                            let _ = rtcp_sock.send_to(&pkt_ping, peer_str.as_str()).await;
                        }
                    }
                }
            }
        }
    }
    probe.server_tokens = server_tokens.clone();

    let (abort_tx, mut abort_rx) = tokio::sync::watch::channel(false);
    let rtp_loop = rtp_sock.clone();
    let rtcp_loop = rtcp_sock.clone();
    let sess_owned = sess_id.to_string();
    let active_token = probe.token;

    tokio::spawn(async move {
        let mut seq: u32 = echo_seq;
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(400));
        let mut buf_rtp = [0u8; 1500];
        let mut buf_rtcp = [0u8; 1500];
        let mut cur_token = active_token;
        loop {
            tokio::select! {
                changed = abort_rx.changed() => {
                    if changed.is_err() || *abort_rx.borrow() {
                        let pkt_end = build_zrtp_end_packet(from_uid, call_id, &sess_owned);
                        for (rtp_addr, _rtcp_addr) in &servers {
                            let _ = rtp_loop.send_to(&pkt_end, rtp_addr.as_str()).await;
                        }
                        break;
                    }
                }
                _ = interval.tick() => {
                    let pkt_e = build_zrtp_echo_packet(seq, &crate::login::unix_now().to_string());
                    seq = seq.wrapping_add(1);
                    if cur_token == 0 {
                        let pkt_init_retry = build_zrtp_init_packet(is_caller, from_uid, partner_uid, call_id, sub_cmd, &sess_owned);
                        for (rtp_addr, _rtcp_addr) in &servers {
                            let _ = rtp_loop.send_to(&pkt_init_retry, rtp_addr.as_str()).await;
                            let _ = rtp_loop.send_to(&pkt_e, rtp_addr.as_str()).await;
                        }
                    } else {
                        for (rtp_addr, _rtcp_addr) in &servers {
                            let tok = server_tokens.get(rtp_addr).copied().unwrap_or(cur_token);
                            let pkt_p = build_zrtp_ping_packet(from_uid, tok, &sess_owned);
                            let _ = rtp_loop.send_to(&pkt_p, rtp_addr.as_str()).await;
                        }
                    }
                }
                Ok((n, peer)) = rtp_loop.recv_from(&mut buf_rtp), if cur_token == 0 => {
                    if let Some((0, _, token, pub_addr)) = parse_zrtp_init_response(&buf_rtp[..n]) {
                        server_tokens.insert(peer.to_string(), token);
                        cur_token = token;
                        trace_line("ZRTP", format!("late RTP init ack from {peer}: token={token} pub={pub_addr:?}"));
                    }
                }
                Ok((n, peer)) = rtcp_loop.recv_from(&mut buf_rtcp) => {
                    if let Some((0, _, token, pub_addr)) = parse_zrtp_init_response(&buf_rtcp[..n]) {
                        server_tokens.entry(peer.to_string()).or_insert(token);
                        if cur_token == 0 {
                            cur_token = token;
                            trace_line("ZRTP", format!("late RTCP init ack from {peer}: token={token} pub={pub_addr:?}"));
                        }
                    } else {
                        // Not a ZRTP init reply: hand it to the media loop (PLI/FIR handling).
                        crate::media::push_rtcp_inbox(&buf_rtcp[..n]);
                    }
                }
            }
        }
    });

    (Some(rtp_sock), Some(rtcp_sock), Some(abort_tx), probe)
}

/// Bind RTP & RTCP UDP sockets, send the ZRTP Init (`cmd=12` / `cmd=11`), Echo (`cmd=5`), and Ping (`cmd=2`)
/// registration burst to all relay servers, and spawn the 400ms UDP keepalive loop.
pub async fn start_zrtp_udp_relay_session(
    payload: &Value,
    from_uid: u32,
    partner_uid: u32,
    call_id: u32,
    is_caller: bool,
    sess_id: &str,
) -> (
    Option<Arc<tokio::net::UdpSocket>>,
    Option<Arc<tokio::net::UdpSocket>>,
    Option<tokio::sync::watch::Sender<bool>>,
    u16,
) {
    let (rtp, rtcp, abort, probe) = start_zrtp_udp_relay_session_with_probe(
        payload,
        from_uid,
        partner_uid,
        call_id,
        is_caller,
        sess_id,
    )
    .await;
    (rtp, rtcp, abort, probe.local_port)
}

/// Detect local IPv4 address and allocate an ephemeral UDP port for `extendData.p2p`.
fn detect_local_p2p_endpoint(override_port: Option<u16>) -> (String, u16) {
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
        let port = override_port.unwrap_or_else(|| sock.local_addr().map(|a| a.port()).unwrap_or(60020));
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(local) = sock.local_addr() {
                return (local.ip().to_string(), port);
            }
        }
        return ("127.0.0.1".to_string(), port);
    }
    ("127.0.0.1".to_string(), override_port.unwrap_or(60020))
}

/// Construct the exact `codec` and `extendData` JSON strings expected by `/api/voicecall/request` (`416`)
/// and `/api/voicecall/answer` (`402`) from a `/api/voicecall/requestcall` (`401`) server response.
pub fn build_pure_rust_invite_fields(
    resp_401: &Value,
    is_video: bool,
) -> (String, String, String, String, String) {
    build_pure_rust_invite_fields_with_port(resp_401, is_video, None)
}

/// Official PC `imei` = `uuid_v4()` + `"-"` + `md5(userAgent)` → 69 chars. The desktop sends
/// exactly that (`2820ea6b-8652-403d-b2dd-7fc95213b59f-1e181a64c8e8c99ddc7c7de8cd35931b`), and the
/// voicecall backend reacts to the shape: a 24-hex homegrown value gets a degraded invite
/// (`params.status=5`, `extendData` stripped).
pub fn official_imei() -> String {
    let hex = crate::param_cipher::random_hex(32);
    let variant = ['8', '9', 'a', 'b'][(hex.as_bytes()[16] as usize) % 4];
    let uuid = format!(
        "{}-{}-4{}-{}{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        variant,
        &hex[17..20],
        &hex[20..32]
    );
    let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
              Zalo/26.10.11 Chrome/122.0.6261.95 Electron/29.1.5 Safari/537.36";
    format!("{uuid}-{}", crate::crypto::md5_hex(ua))
}

/// `session` for a voice call: 115 random bytes rendered as 154 url-safe base64 chars, which is
/// exactly the shape the official PC client mints for itself (captured sessions are 154 chars).
/// The client owns this value — it is sent in **both** `requestcall` (401) and `request` (416),
/// and the server hands it to the callee as `params.sessId` so the callee can echo it in
/// `answer` (402).
pub fn generate_call_session() -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    // 115 bytes from the OS CSPRNG → 154 url-safe base64 chars (same shape as before).
    URL_SAFE_NO_PAD.encode(crate::param_cipher::random_bytes(115))
}

pub fn build_pure_rust_invite_fields_with_port(
    resp_401: &Value,
    is_video: bool,
    local_udp_port: Option<u16>,
) -> (String, String, String, String, String) {
    build_pure_rust_invite_fields_with_probe(resp_401, is_video, local_udp_port, None)
}

pub fn build_pure_rust_invite_fields_with_probe(
    resp_401: &Value,
    is_video: bool,
    local_udp_port: Option<u16>,
    probe: Option<&ZrtpProbeResult>,
) -> (String, String, String, String, String) {
    let first_srv = resp_401
        .get("servers")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first());

    let rtp_ip = first_srv
        .and_then(|s| s.get("rtpaddr").or_else(|| s.get("rtp")))
        .and_then(Value::as_str)
        .or_else(|| {
            resp_401
                .get("rtpIP")
                .or_else(|| resp_401.get("rtpSerIp"))
                .or_else(|| resp_401.get("rtpAddress"))
                .and_then(Value::as_str)
        })
        .unwrap_or("")
        .to_string();

    let rtcp_ip = first_srv
        .and_then(|s| s.get("rtcpaddr").or_else(|| s.get("rtcp")))
        .and_then(Value::as_str)
        .or_else(|| {
            resp_401
                .get("rtcpIP")
                .or_else(|| resp_401.get("rtcpAddress"))
                .or_else(|| resp_401.get("rtpSerIp"))
                .and_then(Value::as_str)
        })
        .unwrap_or(&rtp_ip)
        .to_string();

    let rtp_v6 = first_srv
        .and_then(|s| s.get("rtpIPv6"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let rtcp_v6 = first_srv
        .and_then(|s| s.get("rtcpIPv6"))
        .and_then(Value::as_str)
        .unwrap_or(rtp_v6);

    let sess_id = resp_401
        .get("sessId")
        .or_else(|| resp_401.get("session"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let port_hint = local_udp_port.or_else(|| probe.map(|p| p.local_port));
    let (local_ip, local_port) = detect_local_p2p_endpoint(port_hint);

    let mut p2p_entries = vec![json!({
        "ip": local_ip,
        "port": local_port,
        "type": 0
    })];
    if let Some((pub_ip, pub_port)) = probe.and_then(|p| p.public_p2p.as_ref()) {
        if !pub_ip.is_empty() && pub_ip != &local_ip {
            p2p_entries.push(json!({
                "ip": pub_ip,
                "port": *pub_port,
                "type": 1
            }));
        }
    }

    let server_result: Vec<Value> = resp_401
        .get("servers")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|srv| {
                    let rtp_str = srv.get("rtpaddr").and_then(Value::as_str).unwrap_or("");
                    let (recv, rtt) = match probe {
                        Some(p) => p
                            .echo_stats
                            .get(rtp_str)
                            .copied()
                            .unwrap_or((14, 25)),
                        None => (0, 0),
                    };
                    json!({
                        "recv": recv,
                        "rtcp": srv.get("rtcpaddr").cloned().unwrap_or(json!("")),
                        "rtcpIPv6": srv.get("rtcpIPv6").cloned().unwrap_or(json!("")),
                        "rtp": srv.get("rtpaddr").cloned().unwrap_or(json!("")),
                        "rtpIPv6": srv.get("rtpIPv6").cloned().unwrap_or(json!("")),
                        "rtt": rtt,
                        "spTcp": 1,
                        "tpType": srv.get("tpType").cloned().unwrap_or(json!(0)),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let server_addr: Vec<Value> = resp_401
        .get("servers")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|srv| {
                    json!({
                        "rtcp": srv.get("rtcpaddr").cloned().unwrap_or(json!("")),
                        "rtcpIPv6": srv.get("rtcpIPv6").cloned().unwrap_or(json!("")),
                        "rtp": srv.get("rtpaddr").cloned().unwrap_or(json!("")),
                        "rtpIPv6": srv.get("rtpIPv6").cloned().unwrap_or(json!("")),
                        "tpType": srv.get("tpType").cloned().unwrap_or(json!(0)),
                    })
                })
                .collect()
        })
        .unwrap_or_else(|| {
            vec![json!({
                "rtcp": rtcp_ip,
                "rtcpIPv6": rtcp_v6,
                "rtp": rtp_ip,
                "rtpIPv6": rtp_v6,
                "tpType": 0
            })]
        });

    let num_servers = if probe.is_some() {
        server_result.len()
    } else {
        0
    };

    let extend_obj = json!({
        "callType": if is_video { 1 } else { 0 },
        "fecTP": 0,
        "gccAudio": 1,
        "gccEarlyCall": 0,
        "gccMode": 1,
        "gccSVLR": 1,
        "maxFT": 60,
        "newZrtc": 1,
        "numServers": num_servers,
        "p2p": p2p_entries,
        "packetMode": 2,
        "platform": 2,
        "sP2P": 0,
        "serverAddr": server_addr,
        "serverResult": server_result,
        "spTcp": 1,
        "srtcp": 0,
        "srtpMode": 0,
        "supportCallBusy": 1,
        "supportHevcDecode": 0,
        "tpType": 0,
        "video": {
            "codec": [
                {
                    "name": "h264",
                    "payload": 97
                }
            ]
        }
    });

    let codec_str =
        "[{\"dynamicFptime\":0,\"frmPtime\":20,\"name\":\"opus/16000/1\",\"payload\":112}]\n"
            .to_string();
    let extend_str = format!("{extend_obj}\n");

    (rtp_ip, rtcp_ip, sess_id, codec_str, extend_str)
}

/// Build the callee-side `extendData` JSON string for `/api/voicecall/answer` (`402`),
/// matching `CallController::getExtendData` (`0x008e1644` in `ZaloCall.exe` when `!is_caller`).
pub fn build_callee_extend_data(
    rtp_ip: &str,
    rtcp_ip: &str,
    is_video: bool,
    local_udp_port: Option<u16>,
    probe: Option<&ZrtpProbeResult>,
) -> String {
    let port_hint = local_udp_port.or_else(|| probe.map(|p| p.local_port));
    let (local_ip, local_port) = detect_local_p2p_endpoint(port_hint);
    let mut p2p_entries = vec![json!({
        "ip": local_ip,
        "port": local_port,
        "type": 0
    })];
    if let Some((pub_ip, pub_port)) = probe.and_then(|p| p.public_p2p.as_ref()) {
        if !pub_ip.is_empty() && pub_ip != &local_ip {
            p2p_entries.push(json!({
                "ip": pub_ip,
                "port": *pub_port,
                "type": 1
            }));
        }
    }
    let rtcp_val = if rtcp_ip.is_empty() { rtp_ip } else { rtcp_ip };
    let extend_obj = json!({
        "callType": if is_video { 1 } else { 0 },
        "fecTP": 0,
        "gccAudio": 1,
        "gccEarlyCall": 0,
        "gccMode": 1,
        "gccSVLR": 1,
        "maxFT": 60,
        "newZrtc": 1,
        "numServers": 0,
        "p2p": p2p_entries,
        "packetMode": 2,
        "platform": 2,
        "sP2P": 0,
        "select2side": 1,
        "serverAddr": [
            {
                "rtcp": rtcp_val,
                "rtcpIPv6": "",
                "rtp": rtp_ip,
                "rtpIPv6": "",
                "tpType": 0
            }
        ],
        "spTcp": 1,
        "srtcp": 0,
        "srtpMode": 0,
        "supportCallBusy": 1,
        "supportHevcDecode": 0,
        "tpType": 0,
        "video": {
            "codec": [
                {
                    "name": "h264",
                    "payload": 97
                }
            ]
        }
    });
    format!("{extend_obj}\n")
}

enum CallEngineCmd {
    MakeCall {
        partner_id: String,
        partner_name: String,
        is_video: bool,
        is_group: bool,
    },
    AnswerCall {
        accept: bool,
    },
    SetMediaState {
        is_on_mic: bool,
        is_on_cam: bool,
    },
    EndCall,
    ControlEvent(Value),
}

pub struct CallBridge {
    cmd_tx: mpsc::UnboundedSender<CallEngineCmd>,
}

impl CallBridge {
    /// Spawn the 100% Pure-Rust VoIP Call Engine (no external `ZaloCall.exe` process).
    pub fn spawn(
        session: Arc<Mutex<Session>>,
        gui_tx: Sender<GuiEvent>,
        egui_ctx: eframe::egui::Context,
    ) -> Self {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<CallEngineCmd>();
        let active_call: Arc<Mutex<Option<PureRustCallState>>> = Arc::new(Mutex::new(None));
        let cmd_tx_inner = cmd_tx.clone();

        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    CallEngineCmd::MakeCall {
                        partner_id,
                        partner_name,
                        is_video,
                        is_group,
                    } => {
                        // Outbound-dial lock: refuse everything but the three test accounts before
                        // any server contact, and surface it in the call overlay.
                        if let Err(e) =
                            ensure_call_target_allowed(&partner_id, &partner_name, is_group)
                        {
                            let _ = gui_tx.send(GuiEvent::CallRejected {
                                code: -403,
                                reason: e.to_string(),
                            });
                            egui_ctx.request_repaint();
                            continue;
                        }

                        let s_snap = {
                            let mut s_lock = session.lock().await;
                            let needs_uid = s_lock
                                .login
                                .as_ref()
                                .map(|l| {
                                    l.uin.is_empty()
                                        || !l.uin.chars().all(|c| c.is_ascii_digit())
                                })
                                .unwrap_or(false);
                            if needs_uid {
                                if s_lock.sync_profile_me().await.is_some() {
                                    let _ = s_lock.save();
                                }
                            }
                            s_lock.clone()
                        };

                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(1_700_000_000_000);
                        let call_id = ((now_ms % 1_800_000_000) + 100_000_000) as i64;
                        let call_type = CallType::from_flags(is_video, is_group) as u8;

                        let gui_tx_clone = gui_tx.clone();
                        let ctx_clone = egui_ctx.clone();
                        let active_clone = active_call.clone();

                        tokio::spawn(async move {
                            if is_group {
                                let clean_gid = partner_id
                                    .trim_start_matches("group_")
                                    .trim_start_matches('g')
                                    .trim_start_matches('G')
                                    .to_string();
                                let req_12433 = json!({
                                    "groupId": clean_gid,
                                    "callId": call_id,
                                    "typeRequest": call_type,
                                    "data": "",
                                    "partners": []
                                });
                                match dispatch_send_signal(&s_snap, 12433, &req_12433).await {
                                    Ok(resp) => {
                                        let status = resp
                                            .get("status")
                                            .and_then(|v| {
                                                v.as_i64().or_else(|| {
                                                    v.as_str().and_then(|s| s.parse().ok())
                                                })
                                            })
                                            .unwrap_or(0);
                                        if status != 0 {
                                            let reason = resp
                                                .get("msg")
                                                .and_then(Value::as_str)
                                                .filter(|m| !m.is_empty())
                                                .unwrap_or("Không thể khởi tạo cuộc gọi nhóm")
                                                .to_string();
                                            let _ = gui_tx_clone.send(GuiEvent::CallRejected {
                                                code: status,
                                                reason: reason.clone(),
                                            });
                                            let _ = gui_tx_clone.send(GuiEvent::MessageSent {
                                                thread_id: partner_id,
                                                text: format!(
                                                    "📞 Cuộc gọi nhóm (0s) — {reason} (Mã {status})"
                                                ),
                                                msg_id: format!("call_{}", crate::login::unix_now()),
                                            });
                                            ctx_clone.request_repaint();
                                            return;
                                        }
                                        let sess_id = resp
                                            .get("session")
                                            .or_else(|| resp.get("sessId"))
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .to_string();
                                        *active_clone.lock().await = Some(PureRustCallState {
                                            call_id,
                                            partner_id,
                                            partner_name,
                                            is_video,
                                            is_group: true,
                                            is_caller: true,
                                            is_connected: false,
                                            started_at: std::time::Instant::now(),
                                            sess_id,
                                            rtp_ip: String::new(),
                                            rtcp_ip: String::new(),
                                            extend_data: String::new(),
                                            from_uid_u32: 0,
                                            partner_uid_u32: 0,
                                            relay_token: 0,
                                            server_tokens: BTreeMap::new(),
                                            udp_rtp_sock: None,
                                            udp_rtcp_sock: None,
                                            keepalive_abort: None,
                                            audio: None,
    media: None,
                                        });
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::CallStateChanged("calling".into()));
                                        ctx_clone.request_repaint();
                                    }
                                    Err(e) => {
                                        let _ = gui_tx_clone.send(GuiEvent::CallRejected {
                                            code: -1,
                                            reason: e.to_string(),
                                        });
                                        ctx_clone.request_repaint();
                                    }
                                }
                                return;
                            }

                            // Step 1: 401 (`/api/voicecall/requestcall`)
                            let req_401 = json!({
                                "calleeId": partner_id,
                                "callId": call_id,
                                "codec": "[]",
                                "typeRequest": call_type,
                                "imei": s_snap.imei,
                            });
                            let resp_401 = match dispatch_send_signal(&s_snap, 401, &req_401).await
                            {
                                Ok(r) => r,
                                Err(e) => {
                                    let _ = gui_tx_clone.send(GuiEvent::CallRejected {
                                        code: -1,
                                        reason: format!("Lỗi khởi tạo cuộc gọi (401): {e}"),
                                    });
                                    ctx_clone.request_repaint();
                                    return;
                                }
                            };

                            let status_401 = resp_401
                                .get("status")
                                .and_then(|v| {
                                    v.as_i64()
                                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                                })
                                .unwrap_or(0);
                            if status_401 != 0 {
                                let reason = resp_401
                                    .get("msg")
                                    .and_then(Value::as_str)
                                    .filter(|m| !m.is_empty())
                                    .unwrap_or("Người nhận không thể nhận cuộc gọi lúc này")
                                    .to_string();
                                let _ = gui_tx_clone.send(GuiEvent::CallRejected {
                                    code: status_401,
                                    reason: reason.clone(),
                                });
                                let _ = gui_tx_clone.send(GuiEvent::MessageSent {
                                    thread_id: partner_id,
                                    text: format!(
                                        "📞 Cuộc gọi đi (0s) — {reason} (Mã {status_401})"
                                    ),
                                    msg_id: format!("call_{}", crate::login::unix_now()),
                                });
                                ctx_clone.request_repaint();
                                return;
                            }

                            let real_call_id = resp_401
                                .get("id")
                                .and_then(Value::as_i64)
                                .unwrap_or(call_id);

                            let from_uid_u32 = resp_401
                                .get("fromId")
                                .or_else(|| resp_401.get("callerId"))
                                .and_then(|v| {
                                    v.as_u64()
                                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                                })
                                .map(|v| v as u32)
                                .unwrap_or_else(|| {
                                    s_snap
                                        .zpw_sek
                                        .as_deref()
                                        .and_then(|sek| sek.split('.').nth(1))
                                        .and_then(|x| x.parse::<u32>().ok())
                                        .unwrap_or_else(|| {
                                            s_snap
                                                .login
                                                .as_ref()
                                                .map(|l| uid_str_to_u32(&l.uin))
                                                .unwrap_or(0)
                                        })
                                });
                            let partner_uid_u32 = resp_401
                                .get("toId")
                                .or_else(|| resp_401.get("calleeId"))
                                .and_then(|v| {
                                    v.as_u64()
                                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                                })
                                .map(|v| v as u32)
                                .unwrap_or_else(|| uid_str_to_u32(&partner_id));
                            let sess_id_raw = resp_401
                                .get("sessId")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();

                            // Step 1.5: Register ZRTP UDP session with Zalo's RTP/RTCP relay servers BEFORE 416
                            let (udp_rtp_sock, udp_rtcp_sock, keepalive_abort, zrtp_probe) =
                                start_zrtp_udp_relay_session_with_probe(
                                    &resp_401,
                                    from_uid_u32,
                                    partner_uid_u32,
                                    real_call_id as u32,
                                    true,
                                    &sess_id_raw,
                                )
                                .await;

                            let (rtp_ip, rtcp_ip, sess_id, codec_str, extend_str) =
                                build_pure_rust_invite_fields_with_probe(
                                    &resp_401,
                                    is_video,
                                    Some(zrtp_probe.local_port),
                                    Some(&zrtp_probe),
                                );

                            *active_clone.lock().await = Some(PureRustCallState {
                                call_id: real_call_id,
                                partner_id: partner_id.clone(),
                                partner_name: partner_name.clone(),
                                is_video,
                                is_group: false,
                                is_caller: true,
                                is_connected: false,
                                started_at: std::time::Instant::now(),
                                sess_id: sess_id.clone(),
                                rtp_ip: rtp_ip.clone(),
                                rtcp_ip: rtcp_ip.clone(),
                                extend_data: extend_str.clone(),
                                from_uid_u32,
                                partner_uid_u32,
                                relay_token: zrtp_probe.token,
                                server_tokens: zrtp_probe.server_tokens,
                                udp_rtp_sock,
                                udp_rtcp_sock,
                                keepalive_abort,
                                audio: None,
    media: None,
                            });

                            // Step 2: 416 (`/api/voicecall/request` — rings recipient's phone/Zalo)
                            let req_416 = json!({
                                "callId": real_call_id,
                                "calleeId": partner_id,
                                "codec": codec_str,
                                "extendData": extend_str,
                                "rtcpAddress": rtcp_ip,
                                "rtpAddress": rtp_ip,
                                "session": sess_id,
                                "imei": s_snap.imei,
                            });

                            match dispatch_send_signal(&s_snap, 416, &req_416).await {
                                Ok(resp_416) => {
                                    let st = resp_416
                                        .get("status")
                                        .and_then(|v| {
                                            v.as_i64().or_else(|| {
                                                v.as_str().and_then(|s| s.parse().ok())
                                            })
                                        })
                                        .unwrap_or(0);
                                    if st != 0 {
                                        let reason = resp_416
                                            .get("msg")
                                            .and_then(Value::as_str)
                                            .filter(|m| !m.is_empty())
                                            .unwrap_or("Không thể đổ chuông tới người nhận")
                                            .to_string();
                                        *active_clone.lock().await = None;
                                        let _ = gui_tx_clone.send(GuiEvent::CallRejected {
                                            code: st,
                                            reason,
                                        });
                                    } else {
                                        let _ = gui_tx_clone
                                            .send(GuiEvent::CallStateChanged("calling".into()));
                                        let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                            "🔔 Đang gọi tới {partner_name} (ID cuộc gọi #{real_call_id})…"
                                        )));
                                    }
                                    ctx_clone.request_repaint();
                                }
                                Err(e) => {
                                    *active_clone.lock().await = None;
                                    let _ = gui_tx_clone.send(GuiEvent::CallRejected {
                                        code: -1,
                                        reason: format!("Lỗi gửi tín hiệu đổ chuông (416): {e}"),
                                    });
                                    ctx_clone.request_repaint();
                                }
                            }
                        });
                    }

                    CallEngineCmd::AnswerCall { accept } => {
                        let s_snap = session.lock().await.clone();
                        let active_clone = active_call.clone();
                        let gui_tx_clone = gui_tx.clone();
                        let ctx_clone = egui_ctx.clone();
                        tokio::spawn(async move {
                            if accept {
                                // Wait up to 350ms if the incoming ZRTP UDP probe is still finishing
                                for _ in 0..14 {
                                    let ready = active_clone
                                        .lock()
                                        .await
                                        .as_ref()
                                        .map(|s| s.udp_rtp_sock.is_some())
                                        .unwrap_or(true);
                                    if ready {
                                        break;
                                    }
                                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                                }
                            }
                            let mut guard = active_clone.lock().await;
                            if let Some(ref mut st) = *guard {
                                let call_id = st.call_id;
                                let caller_id = st.partner_id.clone();
                                let rtp_ip = st.rtp_ip.clone();
                                let rtcp_ip = st.rtcp_ip.clone();
                                let sess_id = st.sess_id.clone();
                                let ext = st.extend_data.clone();
                                let codec_str = "[{\"dynamicFptime\":0,\"frmPtime\":20,\"name\":\"opus/16000/1\",\"payload\":112}]\n";

                                if accept {
                                    st.is_connected = true;
                                    st.started_at = std::time::Instant::now();
                                    if let (Some(rsock), Ok(target)) = (
                                        &st.udp_rtp_sock,
                                        st.rtp_ip.parse::<std::net::SocketAddr>(),
                                    ) {
                                        let pkt_p = build_zrtp_ping_packet(
                                            st.from_uid_u32,
                                            st.relay_token,
                                            &st.sess_id,
                                        );
                                        let _ = rsock.try_send_to(&pkt_p, target);
                                    }
                                    match crate::media::MediaHandle::start_with_sockets(
                                        st.rtp_ip.clone(),
                                        st.rtcp_ip.clone(),
                                        st.from_uid_u32,
                                        st.partner_uid_u32,
                                        st.is_caller,
                                        st.relay_token,
                                        st.udp_rtp_sock.clone(),
                                        st.udp_rtcp_sock.clone(),
                                        0,
                                        st.is_video,
                                        Some(gui_tx_clone.clone()),
                                        Some(ctx_clone.clone()),
                                    ) {
                                        Ok(m) => {
                                            st.media = Some(m);
                                        }
                                        Err(e) => {
                                            // Loopback (mic -> own speakers) is not call audio:
                                            // the peer hears nothing and the user hears themselves.
                                            // Report the failure in the call UI; keep the old
                                            // fallback only when ZALO_CALL_LOOPBACK=1.
                                            crate::voip_log::line(
                                                "CALL",
                                                format!("answer media start failed: {e}"),
                                            );
                                            let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(
                                                "⚠️ Không mở được âm thanh cuộc gọi (micro/loa). Cuộc gọi vẫn tiếp tục nhưng không có tiếng.".into(),
                                            ));
                                            if loopback_fallback_enabled() {
                                                match crate::audio::AudioHandle::start() {
                                                    Ok(sess) => st.audio = Some(sess),
                                                    Err(e) => {
                                                        let _ = gui_tx_clone.send(
                                                            GuiEvent::StatusUpdate(format!(
                                                                "⚠️ Không mở được micro/loa: {e}"
                                                            )),
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    let _ = gui_tx_clone
                                        .send(GuiEvent::CallStateChanged("connected".into()));
                                    let _ = gui_tx_clone.send(GuiEvent::StatusUpdate(format!(
                                        "🟢 Đang trong cuộc gọi với {}",
                                        st.partner_name
                                    )));
                                } else {
                                    let partner_id = st.partner_id.clone();
                                    *guard = None;
                                    let _ = gui_tx_clone
                                        .send(GuiEvent::CallStateChanged("free".into()));
                                    let _ = gui_tx_clone.send(GuiEvent::MessageSent {
                                        thread_id: partner_id,
                                        text: "📞 Cuộc gọi đến (Đã từ chối)".into(),
                                        msg_id: format!("call_{}", crate::login::unix_now()),
                                    });
                                }
                                drop(guard);
                                ctx_clone.request_repaint();

                                let mut req_402 = json!({
                                    "callerId": caller_id,
                                    "callId": call_id,
                                    "status": if accept { 0 } else { 3 },
                                    "codec": codec_str,
                                    "rtcpAddress": rtcp_ip,
                                    "rtpAddress": rtp_ip,
                                    "session_id": sess_id,
                                });
                                if accept && !ext.is_empty() {
                                    req_402["extendData"] = json!(ext);
                                    req_402["subCommand"] = json!(3);
                                }
                                match dispatch_send_signal(&s_snap, 402, &req_402).await {
                                    Ok(r) => trace_line("ANSWER", format!("402 resp: {r}")),
                                    Err(e) => trace_line("ANSWER", format!("402 err: {e}")),
                                }
                            }
                        });
                    }

                    CallEngineCmd::SetMediaState {
                        is_on_mic,
                        is_on_cam,
                    } => {
                        let s_snap = session.lock().await.clone();
                        let guard = active_call.lock().await;
                        if let Some(ref st) = *guard {
                            if let Some(media) = &st.media {
                                media.set_mic_muted(!is_on_mic);
                                media.set_cam_enabled(is_on_cam);
                            }
                            let call_id = st.call_id;
                            let partner_id = st.partner_id.clone();
                            tokio::spawn(async move {
                                let req_12006 = json!({
                                    "calleeId": partner_id,
                                    "callId": call_id,
                                    "isOnCam": if is_on_cam { 1 } else { 0 },
                                    "isOnMic": if is_on_mic { 1 } else { 0 },
                                    "data": ""
                                });
                                let _ = dispatch_send_signal(&s_snap, 12006, &req_12006).await;
                            });
                        }
                    }

                    CallEngineCmd::EndCall => {
                        let s_snap = session.lock().await.clone();
                        let taken = active_call.lock().await.take();
                        if let Some(st) = taken {
                            let duration_sec = if st.is_connected {
                                st.started_at.elapsed().as_secs()
                            } else {
                                0
                            };
                            let partner_id = st.partner_id.clone();
                            let call_id = st.call_id;
                            let call_type = CallType::from_flags(st.is_video, st.is_group) as u8;
                            let rtp_ip = st.rtp_ip.clone();
                            let rtcp_ip = st.rtcp_ip.clone();
                            let sess_id = st.sess_id.clone();
                            let is_group = st.is_group;
                            let is_caller = st.is_caller;
                            let is_connected = st.is_connected;

                            if !partner_id.is_empty() {
                                let label = if is_caller {
                                    format!("📞 Cuộc gọi đi ({duration_sec}s)")
                                } else {
                                    format!("📞 Cuộc gọi đến ({duration_sec}s)")
                                };
                                let _ = gui_tx.send(GuiEvent::MessageSent {
                                    thread_id: partner_id.clone(),
                                    text: label,
                                    msg_id: format!("call_{}", crate::login::unix_now()),
                                });
                            }
                            let _ = gui_tx.send(GuiEvent::CallStateChanged("free".into()));
                            egui_ctx.request_repaint();

                            tokio::spawn(async move {
                                if is_group {
                                    let clean_gid = partner_id
                                        .trim_start_matches("group_")
                                        .trim_start_matches('g')
                                        .trim_start_matches('G')
                                        .to_string();
                                    let cmd_code = if is_connected { 12438 } else { 12437 };
                                    let body = json!({
                                        "calleeId": clean_gid,
                                        "callId": call_id,
                                        "callType": call_type,
                                        "hostCall": "",
                                        "data": "",
                                        "partners": []
                                    });
                                    let _ = dispatch_send_signal(&s_snap, cmd_code, &body).await;
                                } else if is_caller && !is_connected {
                                    // Cancel ringing before callee picks up (`405`)
                                    let req_405 = json!({
                                        "toId": partner_id,
                                        "callId": call_id,
                                        "callType": call_type
                                    });
                                    let _ = dispatch_send_signal(&s_snap, 405, &req_405).await;
                                } else if !is_caller && !is_connected {
                                    // Decline incoming call (`402` status = 4)
                                    let req_402 = json!({
                                        "callerId": partner_id,
                                        "callId": call_id,
                                        "status": 3,
                                        "codec": "",
                                        "rtcpAddress": rtcp_ip,
                                        "rtpAddress": rtp_ip,
                                        "session_id": sess_id
                                    });
                                    let _ = dispatch_send_signal(&s_snap, 402, &req_402).await;
                                } else {
                                    // Hang up active connected call (`409` + `406`)
                                    let req_409 = json!({
                                        "toId": partner_id,
                                        "callId": call_id
                                    });
                                    let _ = dispatch_send_signal(&s_snap, 409, &req_409).await;
                                    let req_406 = json!({
                                        "callId": call_id,
                                        "partnerId": partner_id,
                                        "status": 0,
                                        "data": "{}",
                                        "duration": duration_sec,
                                        "role": if is_caller { 1 } else { 0 }
                                    });
                                    let _ = dispatch_send_signal(&s_snap, 406, &req_406).await;
                                }
                            });
                        }
                    }

                    CallEngineCmd::ControlEvent(voip_ctrl) => {
                        let act = voip_ctrl
                            .get("act")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let data = voip_ctrl.get("data").cloned().unwrap_or(Value::Null);
                        trace_line("EVT", format!("act={act} ts={}", voip_ctrl.get("data").and_then(|d| d.get("ts")).cloned().unwrap_or(Value::Null)));
                        trace_line("EVT", format!("act={act} data={data}"));
                        crate::voip_log::log_signal_media(act, &event_media_params(&data));
                        let ev_call_id = data
                            .get("callId")
                            .or_else(|| data.get("id"))
                            .and_then(|v| {
                                v.as_i64()
                                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                            })
                            .unwrap_or(0);
                        let s_snap = session.lock().await.clone();

                        match act {
                            "ring_ring" | "group_ring_ring" => {
                                let guard = active_call.lock().await;
                                let Some(ref st) = *guard else {
                                    continue;
                                };
                                if ev_call_id != 0 && ev_call_id != st.call_id {
                                    continue;
                                }
                                let _ = gui_tx.send(GuiEvent::CallStateChanged("ringing".into()));
                                let _ = gui_tx.send(GuiEvent::StatusUpdate(
                                    "🔔 Thiết bị của người nhận đang đổ chuông…".into(),
                                ));
                                egui_ctx.request_repaint();
                            }
                            "answer" | "group_answer" => {
                                trace_line("EVT", format!("ctrl answer raw: {voip_ctrl}"));
                                let status = data
                                    .get("status")
                                    .and_then(|v| {
                                        v.as_i64().or_else(|| {
                                            v.as_str().and_then(|s| s.trim().parse::<i64>().ok())
                                        })
                                    })
                                    .unwrap_or(0);
                                let mut guard = active_call.lock().await;
                                // If `active_call` is None and `status == 5`, check if this is an incoming
                                // call invite stub delivered via `lp_control` (Queue 20) from a peer!
                                if guard.is_none() {
                                    let self_uin = s_snap
                                        .login
                                        .as_ref()
                                        .map(|l| l.uin.clone())
                                        .unwrap_or_default();
                                    let self_uid32 = s_snap
                                        .zpw_sek
                                        .as_deref()
                                        .and_then(|sek| sek.split('.').nth(1))
                                        .unwrap_or("");
                                    let uid_n = data
                                        .get("uidN")
                                        .and_then(Value::as_str)
                                        .unwrap_or("");
                                    let uid_from = data
                                        .get("uidFrom")
                                        .and_then(|v| match v {
                                            Value::String(s) => Some(s.to_string()),
                                            Value::Number(n) => Some(n.to_string()),
                                            _ => None,
                                        })
                                        .unwrap_or_default();
                                    let is_from_peer = (!uid_n.is_empty() && uid_n != self_uin)
                                        || (!uid_from.is_empty() && uid_from != self_uid32);
                                    if status == 5 && ev_call_id != 0 && is_from_peer {
                                        drop(guard);
                                        let mut synth = voip_ctrl.clone();
                                        if let Some(obj) = synth.as_object_mut() {
                                            obj.insert(
                                                "act".into(),
                                                json!(if act == "group_answer" {
                                                    "group_request"
                                                } else {
                                                    "request"
                                                }),
                                            );
                                        }
                                        let _ = cmd_tx_inner.send(CallEngineCmd::ControlEvent(synth));
                                    }
                                    continue;
                                }
                                let Some(st_check) = guard.as_ref() else {
                                    continue;
                                };
                                if ev_call_id != 0 && ev_call_id != st_check.call_id {
                                    continue;
                                }
                                // Skip replayed answer events after already connected
                                if st_check.is_connected {
                                    continue;
                                }
                                if status == 0 {
                                    if let Some(st) = guard.as_mut() {
                                        st.is_connected = true;
                                        st.started_at = std::time::Instant::now();
                                        // Update RTP/RTCP relay and token if callee selected a specific `rtpSerIp`
                                        let ans_media = event_media_params(&data);
                                        if let Some(sel_srv) = ans_media
                                            .get("rtpSerIp")
                                            .or_else(|| ans_media.get("rtpIP"))
                                            .and_then(Value::as_str)
                                            .map(str::trim)
                                            .filter(|s| !s.is_empty())
                                        {
                                            st.rtp_ip = sel_srv.to_string();
                                            st.rtcp_ip = sel_srv.to_string();
                                            if let Some(&tok) = st.server_tokens.get(sel_srv) {
                                                st.relay_token = tok;
                                            }
                                        }
                                        trace_line(
                                            "MEDIA",
                                            format!(
                                                "start caller=true rtp={} token={} video={}",
                                                st.rtp_ip, st.relay_token, st.is_video
                                            ),
                                        );
                                        // Send an immediate ZRTP Ping (cmd=0x0002) to the selected relay
                                        if let (Some(rsock), Ok(target)) = (
                                            &st.udp_rtp_sock,
                                            st.rtp_ip.parse::<std::net::SocketAddr>(),
                                        ) {
                                            let pkt_p = build_zrtp_ping_packet(
                                                st.from_uid_u32,
                                                st.relay_token,
                                                &st.sess_id,
                                            );
                                            let _ = rsock.try_send_to(&pkt_p, target);
                                        }
                                        // Start Opus/RTP media over the registered ZRTP UDP relay sockets
                                        // (falling back to local audio pass-through only if RTP media fails).
                                        match crate::media::MediaHandle::start_with_sockets(
                                            st.rtp_ip.clone(),
                                            st.rtcp_ip.clone(),
                                            st.from_uid_u32,
                                            st.partner_uid_u32,
                                            st.is_caller,
                                            st.relay_token,
                                            st.udp_rtp_sock.clone(),
                                            st.udp_rtcp_sock.clone(),
                                            0,
                                            st.is_video,
                                            Some(gui_tx.clone()),
                                            Some(egui_ctx.clone()),
                                        ) {
                                            Ok(m) => {
                                                st.media = Some(m);
                                            }
                                            Err(e) => {
                                                crate::voip_log::line(
                                                    "CALL",
                                                    format!("caller media start failed: {e}"),
                                                );
                                                let _ = gui_tx.send(GuiEvent::StatusUpdate(
                                                    "⚠️ Không mở được âm thanh cuộc gọi (micro/loa). Cuộc gọi vẫn tiếp tục nhưng không có tiếng.".into(),
                                                ));
                                                if loopback_fallback_enabled() {
                                                    match crate::audio::AudioHandle::start() {
                                                        Ok(sess) => st.audio = Some(sess),
                                                        Err(e) => {
                                                            let _ = gui_tx.send(
                                                                GuiEvent::StatusUpdate(format!(
                                                                    "⚠️ Không mở được micro/loa: {e}"
                                                                )),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        let call_id = st.call_id;
                                        let callee_id = st.partner_id.clone();
                                        tokio::spawn(async move {
                                            let req_408 = json!({
                                                "calleeId": callee_id,
                                                "callId": call_id
                                            });
                                            let _ =
                                                dispatch_send_signal(&s_snap, 408, &req_408).await;
                                        });
                                    }
                                    let _ =
                                        gui_tx.send(GuiEvent::CallStateChanged("connected".into()));
                                    let _ = gui_tx.send(GuiEvent::StatusUpdate(
                                        "🟢 Người nhận đã bắt máy — Đang trong cuộc gọi".into(),
                                    ));
                                } else if !st_check.is_caller && status == 5 {
                                    // Queue 20 (`lp_control`) echoes a `status=5` stub on call creation;
                                    // ignore it on the callee side so ringing is not aborted prematurely.
                                    continue;
                                } else if st_check.is_caller
                                    && status == 5
                                    && st_check.started_at.elapsed()
                                        < std::time::Duration::from_secs(8)
                                    && !event_media_params(&data)
                                        .get("extendData")
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .is_empty()
                                {
                                    // Queue 20 echoes the caller's own 416 invite with `status=5`
                                    // right before `ring_ring` arrives; do not mistake it for a rejection.
                                    continue;
                                } else {
                                    let taken = guard.take();
                                    let reason = match status {
                                        1 | 2 | 3 | 4 => {
                                            "Người nhận đang bận hoặc đã từ chối cuộc gọi"
                                        }
                                        5 => "Kết nối thoại bị gián đoạn hoặc người nhận từ chối",
                                        _ => "Người nhận không trả lời cuộc gọi",
                                    };
                                    let _ = gui_tx.send(GuiEvent::CallRejected {
                                        code: status,
                                        reason: reason.into(),
                                    });
                                    if let Some(st) = taken {
                                        let _ = gui_tx.send(GuiEvent::MessageSent {
                                            thread_id: st.partner_id.clone(),
                                            text: format!("📞 Cuộc gọi đi (0s) — {reason}"),
                                            msg_id: format!("call_{}", crate::login::unix_now()),
                                        });
                                    }
                                }
                                egui_ctx.request_repaint();
                            }
                            "end_call" | "cancel" | "group_end_call" | "group_cancel" => {
                                let mut guard = active_call.lock().await;
                                let Some(ref st_check) = *guard else {
                                    continue;
                                };
                                if ev_call_id != 0 && ev_call_id != st_check.call_id {
                                    continue;
                                }
                                if let Some(st) = guard.take() {
                                    let duration_sec = if st.is_connected {
                                        st.started_at.elapsed().as_secs()
                                    } else {
                                        0
                                    };
                                    let label = if st.is_caller {
                                        format!("📞 Cuộc gọi đi ({duration_sec}s)")
                                    } else if st.is_connected {
                                        format!("📞 Cuộc gọi đến ({duration_sec}s)")
                                    } else {
                                        "📞 Cuộc gọi nhỡ".to_string()
                                    };
                                    let _ = gui_tx.send(GuiEvent::MessageSent {
                                        thread_id: st.partner_id.clone(),
                                        text: label,
                                        msg_id: format!("call_{}", crate::login::unix_now()),
                                    });
                                }
                                let _ = gui_tx.send(GuiEvent::CallStateChanged("free".into()));
                                let _ = gui_tx.send(GuiEvent::StatusUpdate(
                                    "Cuộc gọi đã kết thúc từ phía đối phương".into(),
                                ));
                                egui_ctx.request_repaint();
                            }
                            "request" | "group_request" => {
                                // Skip if already handling this call with a full extendData
                                let mut is_upgrade = false;
                                {
                                    let g = active_call.lock().await;
                                    if let Some(st) = g.as_ref() {
                                        if st.call_id == ev_call_id {
                                            if !st.extend_data.is_empty() || st.is_connected {
                                                continue;
                                            }
                                            is_upgrade = true;
                                        }
                                    }
                                }
                                // Reject historical replays (> 90 seconds old), tolerating PC/phone clock skew
                                if let Some(ev_ts) = data.get("ts").and_then(|v| {
                                    v.as_i64()
                                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                                }) {
                                    let now_ms = (crate::login::unix_now() * 1000) as i64;
                                    let ev_ms = if ev_ts < 10_000_000_000 {
                                        ev_ts * 1000
                                    } else {
                                        ev_ts
                                    };
                                    if now_ms.saturating_sub(ev_ms) > 90_000 {
                                        continue;
                                    }
                                }
                                let call_id = ev_call_id;
                                let media = event_media_params(&data);
                                let ext_parsed: Value = media
                                    .get("extendData")
                                    .or_else(|| data.get("extendData"))
                                    .and_then(Value::as_str)
                                    .and_then(|s| serde_json::from_str::<Value>(s.trim()).ok())
                                    .unwrap_or(Value::Null);
                                // `callerId` in 407/402 must be the 19-digit `uidN` in our id space;
                                // the 32-bit `uidFrom` is rejected with `114`.
                                let caller_id = data
                                    .get("uidN")
                                    .or_else(|| data.get("uidFrom"))
                                    .and_then(|v| match v {
                                        Value::String(s) if !s.is_empty() => Some(s.clone()),
                                        Value::Number(n) => Some(n.to_string()),
                                        _ => None,
                                    })
                                    .unwrap_or_default();
                                // Decrypted signalling (session id, uid, RTP addresses) must not
                                // reach stderr in production; `trace_line` is dev-only.
                                trace_line(
                                    "EVT",
                                    format!(
                                        "incoming request call_id={} caller={} data={}",
                                        ev_call_id, caller_id, data
                                    ),
                                );
                                let caller_name = media
                                    .get("Dname")
                                    .or_else(|| data.get("Dname"))
                                    .or_else(|| data.get("groupName"))
                                    .and_then(Value::as_str)
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or("Người dùng Zalo")
                                    .to_string();
                                let call_type = media
                                    .get("zrtc_config")
                                    .and_then(|z| z.get("callType"))
                                    .or_else(|| data.get("callType"))
                                    .and_then(Value::as_u64)
                                    .unwrap_or(1);
                                let ext_call_type = ext_parsed
                                    .get("callType")
                                    .and_then(Value::as_i64)
                                    .unwrap_or(-1);
                                let is_video = call_type == 3
                                    || call_type == 6
                                    || ext_call_type == 1
                                    || ext_parsed.get("video").is_some()
                                    || media
                                        .get("zrtc_config")
                                        .and_then(|z| z.get("video"))
                                        .and_then(|v| v.get("enable"))
                                        .and_then(Value::as_i64)
                                        == Some(1)
                                    || data
                                        .get("video")
                                        .and_then(|v| v.get("enable"))
                                        .and_then(Value::as_i64)
                                        == Some(1);
                                let is_group = act == "group_request" || call_type >= 5;

                                let from_uid_u32 = data
                                    .get("uidTo")
                                    .or_else(|| data.get("toId"))
                                    .or_else(|| data.get("calleeId"))
                                    .or_else(|| media.get("reveiverId"))
                                    .or_else(|| media.get("receiverId"))
                                    .and_then(|v| {
                                        v.as_u64()
                                            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                                    })
                                    .map(|v| v as u32)
                                    .unwrap_or_else(|| {
                                        s_snap
                                            .zpw_sek
                                            .as_deref()
                                            .and_then(|sek| sek.split('.').nth(1))
                                            .and_then(|x| x.parse::<u32>().ok())
                                            .unwrap_or(0)
                                    });
                                let partner_uid_u32 = data
                                    .get("uidFrom")
                                    .or_else(|| data.get("fromId"))
                                    .or_else(|| data.get("callerId"))
                                    .and_then(|v| {
                                        v.as_u64()
                                            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                                    })
                                    .map(|v| v as u32)
                                    .unwrap_or_else(|| uid_str_to_u32(&caller_id));
                                let sess_id_raw = media
                                    .get("sessId")
                                    .or_else(|| media.get("session"))
                                    .or_else(|| data.get("session"))
                                    .or_else(|| data.get("sessId"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string();

                                let active_clone = active_call.clone();
                                let gui_tx_clone = gui_tx.clone();
                                let ctx_clone = egui_ctx.clone();
                                let relay_source = if media.is_object() {
                                    media.clone()
                                } else {
                                    data.clone()
                                };

                                // Populate `active_call` immediately so clicking Accept/Decline or receiving
                                // `cancel` during the 280ms UDP probe works without racing!
                                let init_rtp = collect_relay_servers(&relay_source)
                                    .into_iter()
                                    .next()
                                    .map(|(r, _)| r)
                                    .unwrap_or_default();
                                {
                                    let mut g = active_call.lock().await;
                                    if g.is_none() {
                                        *g = Some(PureRustCallState {
                                            call_id,
                                            partner_id: caller_id.clone(),
                                            partner_name: caller_name.clone(),
                                            is_video,
                                            is_group,
                                            is_caller: false,
                                            is_connected: false,
                                            started_at: std::time::Instant::now(),
                                            sess_id: sess_id_raw.clone(),
                                            rtp_ip: init_rtp.clone(),
                                            rtcp_ip: init_rtp,
                                            extend_data: String::new(),
                                            from_uid_u32,
                                            partner_uid_u32,
                                            relay_token: 0,
                                            server_tokens: BTreeMap::new(),
                                            udp_rtp_sock: None,
                                            udp_rtcp_sock: None,
                                            keepalive_abort: None,
                                            audio: None,
                                            media: None,
                                        });
                                    }
                                }

                                // Show incoming call window in GUI immediately (before 280ms UDP probe)
                                if !is_upgrade {
                                    let _ = gui_tx_clone.send(GuiEvent::IncomingCall {
                                        call_id,
                                        caller_id: caller_id.clone(),
                                        caller_name: caller_name.clone(),
                                        is_video,
                                        is_group,
                                    });
                                    ctx_clone.request_repaint();
                                }

                                tokio::spawn(async move {
                                    if !is_upgrade {
                                        // Notify caller that our client is ringing (`407`) right away
                                        let req_407 = json!({
                                            "callerId": caller_id,
                                            "callId": call_id,
                                            "status": 0,
                                            "imei": s_snap.imei,
                                        });
                                        let _ = dispatch_send_signal(&s_snap, 407, &req_407).await;
                                    }

                                    let (udp_rtp_sock, udp_rtcp_sock, keepalive_abort, zrtp_probe) =
                                        start_zrtp_udp_relay_session_with_probe(
                                            &relay_source,
                                            from_uid_u32,
                                            partner_uid_u32,
                                            call_id as u32,
                                            false,
                                            &sess_id_raw,
                                        )
                                        .await;

                                    let (mut rtp_ip, mut rtcp_ip, sess_from_inv, _codec, _caller_ext) =
                                        build_pure_rust_invite_fields_with_probe(
                                            &relay_source,
                                            is_video,
                                            Some(zrtp_probe.local_port),
                                            Some(&zrtp_probe),
                                        );
                                    if rtp_ip.is_empty() {
                                        if let Some(sel) = &zrtp_probe.selected_rtp {
                                            rtp_ip = sel.clone();
                                            rtcp_ip = sel.clone();
                                        }
                                    }
                                    let sess_id = if !sess_id_raw.is_empty() {
                                        sess_id_raw
                                    } else {
                                        sess_from_inv
                                    };
                                    let extend_str = build_callee_extend_data(
                                        &rtp_ip,
                                        &rtcp_ip,
                                        is_video,
                                        Some(zrtp_probe.local_port),
                                        Some(&zrtp_probe),
                                    );

                                    let mut g = active_clone.lock().await;
                                    if let Some(st) = g.as_mut() {
                                        if st.call_id == call_id {
                                            st.is_video = is_video;
                                            st.sess_id = sess_id;
                                            if !rtp_ip.is_empty() {
                                                st.rtp_ip = rtp_ip;
                                                st.rtcp_ip = rtcp_ip;
                                            }
                                            st.extend_data = extend_str;
                                            st.relay_token = zrtp_probe.token;
                                            st.server_tokens = zrtp_probe.server_tokens;
                                            st.udp_rtp_sock = udp_rtp_sock;
                                            st.udp_rtcp_sock = udp_rtcp_sock;
                                            st.keepalive_abort = keepalive_abort;
                                        }
                                    }
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Self { cmd_tx }
    }

    /// Initiate an outgoing voice or video call (1-on-1 or Group) 100% in pure Rust.
    pub fn make_call(
        &self,
        partner_id: &str,
        partner_name: &str,
        _partner_avatar: Option<&str>,
        is_video: bool,
        is_group: bool,
    ) {
        let _ = self.cmd_tx.send(CallEngineCmd::MakeCall {
            partner_id: partner_id.to_string(),
            partner_name: partner_name.to_string(),
            is_video,
            is_group,
        });
    }

    /// Accept (`true`) or decline (`false`) an incoming VoIP call.
    pub fn answer_call(&self, accept: bool) {
        let _ = self.cmd_tx.send(CallEngineCmd::AnswerCall { accept });
    }

    /// Update microphone/camera state (`12006` `/api/voicecall/conf`) during an active call.
    pub fn set_media_state(&self, is_on_mic: bool, is_on_cam: bool) {
        let _ = self.cmd_tx.send(CallEngineCmd::SetMediaState {
            is_on_mic,
            is_on_cam,
        });
    }

    /// Process an incoming VoIP control event (`act_type == "voip"`) in pure Rust.
    pub fn handle_control_event(&self, voip_ctrl: Value) {
        let _ = self.cmd_tx.send(CallEngineCmd::ControlEvent(voip_ctrl));
    }

    /// Hang up or cancel the current active call in pure Rust (`405` cancel or `409` endcall).
    pub fn end_call(&self) {
        let _ = self.cmd_tx.send(CallEngineCmd::EndCall);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_poll_echoes_at_most_ten_newest_ids_per_queue() {
        // [O] 03/11: the first lp_control reply listed 50 ids in each of queues 5, 14 and 17;
        // echoing all of them made every later poll fail with 413 Payload Too Large.
        let ids: Vec<Value> = (1001..=1050).map(|n| json!(n.to_string())).collect();
        let mut st = CtrlPollState::default();
        st.update_from_response(&json!({
            "lastActionId": "1050",
            "queueStatus": { "17": { "lastId": "1050", "ids": ids } }
        }));
        let expected: Vec<String> = (1041..=1050).map(|n| n.to_string()).collect();
        assert_eq!(st.queues["17"].ids, expected);
    }

    #[test]
    fn any_one_to_one_contact_can_be_called() {
        // The old test lock (three hard-coded accounts) is gone.
        assert!(call_target_allowed("123456789012345678", "", false));
        assert!(call_target_allowed("100000001", "Nguyễn Văn A", false));
        assert!(ensure_call_target_allowed("123456789", "", false).is_ok());
        // An empty target is still refused.
        assert!(!call_target_allowed("", "Nguyễn Văn A", false));
        assert!(ensure_call_target_allowed("  ", "", false).is_err());
    }

    #[test]
    fn group_calls_are_refused_honestly() {
        assert!(!call_target_allowed("1000000000000000001", "Nhóm", true));
        assert!(!call_target_allowed("group_123", "", false));
        let err = ensure_call_target_allowed("group_123", "", true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("gọi nhóm"), "{err}");
    }

    #[test]
    fn pipe_aes_roundtrip_and_delimiter() {
        let msg = r#"{"type":"update","command":"native-ready"}"#;
        let frame = encrypt_pipe_frame(msg).unwrap();
        assert!(frame.ends_with('$'));
        let decoded = decrypt_pipe_frame(&frame).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn drain_pipe_frames_splits_multiple_and_keeps_partial() {
        let f1 = encrypt_pipe_frame(r#"{"a":1}"#).unwrap();
        let f2 = encrypt_pipe_frame(r#"{"b":2}"#).unwrap();
        let mut buf = format!("{f1}{f2}deadbeef");
        let drained = drain_pipe_frames(&mut buf);
        assert_eq!(drained.len(), 2);
        assert_eq!(decrypt_pipe_frame(&drained[0]).unwrap(), r#"{"a":1}"#);
        assert_eq!(decrypt_pipe_frame(&drained[1]).unwrap(), r#"{"b":2}"#);
        assert_eq!(buf, "deadbeef");
    }

    #[test]
    fn call_type_codes_match_zalo_pc() {
        assert_eq!(CallType::from_flags(false, false) as u8, 1);
        assert_eq!(CallType::from_flags(true, false) as u8, 3);
        assert_eq!(CallType::from_flags(false, true) as u8, 5);
        assert_eq!(CallType::from_flags(true, true) as u8, 6);
    }

    #[test]
    fn call_type_payload_reads_type_request_then_type_then_defaults_to_audio() {
        // `CallBridge` sends `typeRequest`; the official capture and `call_probe` send `type`.
        assert_eq!(call_type_from_payload(&json!({ "typeRequest": 5 })), 5);
        assert_eq!(call_type_from_payload(&json!({ "type": 5 })), 5);
        assert_eq!(call_type_from_payload(&json!({})), 1);
        // `typeRequest` wins when both are present (a video dial must not be pinned to 1).
        assert_eq!(call_type_from_payload(&json!({ "typeRequest": 3, "type": 1 })), 3);
        // Strings are accepted too, matching the loosely-typed official payload.
        assert_eq!(call_type_from_payload(&json!({ "typeRequest": "6" })), 6);
        assert_eq!(call_type_from_payload(&json!({ "typeRequest": "x", "type": 3 })), 3);
    }

    #[test]
    fn ctrl_poll_state_roundtrips_and_accepts_partial_or_empty_json() {
        let mut st = CtrlPollState::default();
        st.last_action_id = "4242".into();
        st.last_action_id_other = "7".into();
        st.queues.insert(
            "603_1".into(),
            QueueCursor {
                last_id: "99".into(),
                ids: vec!["97".into(), "98".into()],
            },
        );
        st.queues.insert(
            "604_0".into(),
            QueueCursor {
                last_id: "5".into(),
                ids: vec!["4".into(), "5".into()],
            },
        );

        let text = serde_json::to_string(&st).expect("serialize CtrlPollState");
        let back: CtrlPollState = serde_json::from_str(&text).expect("deserialize CtrlPollState");
        assert_eq!(back, st);

        // Files written before a field existed must still load: `#[serde(default)]`.
        let partial: CtrlPollState =
            serde_json::from_str(r#"{"last_action_id":"9"}"#).expect("partial CtrlPollState");
        assert_eq!(partial.last_action_id, "9");
        assert_eq!(partial.queues, CtrlPollState::default().queues);

        let empty: CtrlPollState = serde_json::from_str("{}").expect("empty CtrlPollState");
        assert_eq!(empty, CtrlPollState::default());
    }

    #[test]
    fn typing_events_cover_direct_group_voip_garbage_and_typing_end() {
        let data_node = json!({
            "controls": [
                // 1-1 typing: `content` is an object and `toid` wins for a direct chat.
                { "content": {
                    "act": "typing",
                    "data": { "toid": "123456789", "dName": "Nguyễn Văn A" }
                }},
                // Group typing: `content` is a JSON string and the `group_` prefix is stripped.
                { "content": "{\"act\":\"typing\",\"data\":{\"id\":\"group_987654321\",\"dName\":\"Nhóm Thiết kế\",\"isGroup\":1}}" },
                // A VoIP control shares this array and must be ignored.
                { "content": { "act": "typing", "act_type": "voip", "data": { "toid": "111", "dName": "X" } } },
                // Malformed items must be skipped, never panic.
                { "content": "not json" },
                { "nope": 1 },
                // `typing_end` stops typing.
                { "content": {
                    "act": "typing",
                    "act_type": "typing_end",
                    "data": { "toid": "123456789", "dName": "Nguyễn Văn A" }
                }},
                // An explicit `expired` flag also stops typing.
                { "content": {
                    "act": "typing",
                    "data": { "toid": "555", "dName": "Trần B", "expired": 1 }
                }},
            ]
        });

        let got = extract_typing_events(&data_node);
        assert_eq!(got.len(), 4, "{got:?}");
        assert_eq!(
            got[0],
            TypingEvent {
                thread_id: "123456789".into(),
                name: "Nguyễn Văn A".into(),
                is_typing: true,
            }
        );
        assert_eq!(
            got[1],
            TypingEvent {
                thread_id: "987654321".into(),
                name: "Nhóm Thiết kế".into(),
                is_typing: true,
            }
        );
        assert_eq!(
            got[2],
            TypingEvent {
                thread_id: "123456789".into(),
                name: "Nguyễn Văn A".into(),
                is_typing: false,
            }
        );
        assert_eq!(
            got[3],
            TypingEvent {
                thread_id: "555".into(),
                name: "Trần B".into(),
                is_typing: false,
            }
        );

        // No `controls` node at all -> empty, exactly like `extract_voip_controls`.
        assert!(extract_typing_events(&json!({ "data": {} })).is_empty());
        assert!(extract_typing_events(&json!({ "controls": [] })).is_empty());
    }

    #[test]
    fn syncmsg_info_keeps_only_backup_shape_and_never_the_url_token() {
        let data_node = json!({
            "controls": [
                // Full backup description; the URL carries a one-shot token in its query.
                { "content": { "act": "syncmsg_info", "data": {
                    "file_name": "zalo_backup.db",
                    "url": "https://cdn.zdn.vn/backup/abc?token=SECRET",
                    "checksum_code": "abc123",
                    "encrypted_key": "0123456789abcdef0123456789abcdef",
                    "db_format": 1,
                    "is_full_transfer": true,
                    "from_seq_id": "10",
                    "min_seq_id": 3,
                    "pc_name": "PC-A",
                    "public_key": "PUB-KEY"
                }}},
                // A URL without a query string still yields its host.
                { "content": { "act": "syncmsg_info", "data": {
                    "file_name": "b.db",
                    "url": "https://cdn.example/backup.bin",
                    "pc_name": "PC-A"
                }}},
            ]
        });

        let events = extract_transfer_events(&data_node, "PC-A", "PUB-KEY");
        assert_eq!(events.len(), 2, "{events:?}");

        let full = &events[0];
        assert_eq!(full.act, "syncmsg_info");
        assert!(full.user_action.is_none() && full.status.is_none() && full.error_code.is_none());
        assert!(!full.temp_key_present);
        let b = full.backup.as_ref().expect("backup summary");
        assert_eq!(b.file_name_len, "zalo_backup.db".len());
        assert!(b.url_present);
        assert_eq!(b.url_host.as_deref(), Some("cdn.zdn.vn"));
        assert!(b.checksum_present);
        assert_eq!(b.encrypted_key_len, 32);
        assert_eq!(b.db_format, Some(1));
        assert_eq!(b.is_full_transfer, Some(true));
        assert_eq!(b.from_seq_id, Some(10), "numeric strings are accepted");
        assert_eq!(b.min_seq_id, Some(3));
        assert_eq!(b.pc_name_matches_session, Some(true));
        assert_eq!(b.public_key_matches_session, Some(true));

        // Nothing secret may survive anywhere in the event: no token, no query, no path, no key.
        let dump = format!("{events:?}");
        for secret in ["SECRET", "token=", "0123456789abcdef", "backup/abc"] {
            assert!(!dump.contains(secret), "leaked {secret:?} in {dump}");
        }

        let plain = &events[1];
        let b2 = plain.backup.as_ref().expect("backup summary");
        assert!(b2.url_present);
        assert_eq!(b2.url_host.as_deref(), Some("cdn.example"));
        assert!(!b2.checksum_present);
        assert_eq!(b2.encrypted_key_len, 0);
        assert_eq!(b2.public_key_matches_session, None, "absent field is not a mismatch");

        // A different local session is reported as a mismatch, not silently accepted.
        let other = extract_transfer_events(&data_node, "PC-B", "OTHER");
        let b = other[0].backup.as_ref().expect("backup summary");
        assert_eq!(b.pc_name_matches_session, Some(false));
        assert_eq!(b.public_key_matches_session, Some(false));

        // No `controls` node at all -> empty.
        assert!(extract_transfer_events(&json!({}), "PC-A", "PUB-KEY").is_empty());
    }

    #[test]
    fn user_confirm_transfer_error_and_temp_key_are_classified() {
        let data_node = json!({
            "controls": [
                { "content": { "act": "user_confirm", "data": { "user_action": 0 } } },
                // Content as a JSON string, with the action carried as a numeric string.
                { "content": "{\"act\":\"user_confirm\",\"data\":{\"user_action\":\"2\"}}" },
                { "content": { "act": "user_confirm", "data": { "user_action": 3, "toid": "123456789" } } },
                { "content": { "act": "transfer_error", "data": { "status": 1, "error_code": 7 } } },
                { "content": { "act": "transfer_after_login", "data": { "temp_key": "TEMP-SECRET", "imei": "imei-1" } } },
                // Irrelevant / malformed controls are skipped without panicking.
                { "content": { "act": "typing", "data": { "toid": "1" } } },
                { "content": "not json" },
                { "nope": 1 },
            ]
        });

        let events = extract_transfer_events(&data_node, "PC-A", "PUB-KEY");
        assert_eq!(events.len(), 5, "{events:?}");

        assert_eq!(events[0].act, "user_confirm");
        assert_eq!(events[0].user_action, Some(0), "0 = declined");
        assert_eq!(events[1].user_action, Some(2), "string action, 2 = restoring");
        assert_eq!(events[2].user_action, Some(3), "3 = accepted");
        assert_eq!(events[2].thread_hint.as_deref(), Some("123456789"));
        assert!(events[2].backup.is_none() && !events[2].temp_key_present);

        assert_eq!(events[3].act, "transfer_error");
        assert_eq!(events[3].status, Some(1));
        assert_eq!(events[3].error_code, Some(7));

        assert_eq!(events[4].act, "transfer_after_login");
        assert!(events[4].temp_key_present);

        // The temp key and any unrelated field must never be retained.
        let dump = format!("{events:?}");
        for secret in ["TEMP-SECRET", "imei-1"] {
            assert!(!dump.contains(secret), "unexpected value kept: {dump}");
        }
    }

    #[test]
    fn make_call_payload_structure() {
        let p = build_make_call_payload("12345", "Nguyen Van A", Some("https://ava.zdn.vn/1.jpg"), true, false);
        assert_eq!(p["type"], "request");
        assert_eq!(p["command"], "makeCall");
        assert_eq!(p["data"]["type"], 3);
        assert_eq!(p["data"]["partner"][0]["id"], "12345");
        assert_eq!(p["data"]["partner"][0]["name"], "Nguyen Van A");
    }

    #[test]
    fn extract_voip_controls_parses_embedded_json_data() {
        let raw = json!({
            "controls": [
                {
                    "content": {
                        "act_type": "voip",
                        "act": "request",
                        "data": r#"{"callId":998877,"uidN":"12345","ts":1700000000000}"#
                    }
                },
                {
                    "content": {
                        "act_type": "typing",
                        "act": "typing"
                    }
                }
            ]
        });
        let voip = extract_voip_controls(&raw);
        assert_eq!(voip.len(), 1);
        assert_eq!(voip[0]["act"], "request");
        assert_eq!(voip[0]["data"]["callId"], 998877);
    }

    #[test]
    fn normalize_voicecall_data_always_returns_json_object() {
        assert!(normalize_voicecall_data(Value::Null).is_object());
        assert!(normalize_voicecall_data(json!("")).is_object());
        let parsed = normalize_voicecall_data(json!(r#"{"callId":42,"sessId":"abc"}"#));
        assert_eq!(parsed["callId"], 42);
        assert_eq!(parsed["sessId"], "abc");
    }

    #[test]
    fn init_payload_prefers_numeric_uin_over_send2me() {
        let mut s = Session::new(false, false, false).unwrap();
        s.login = Some(crate::session::LoginInfo {
            uin: "1000000000000000002".into(),
            send2me_id: "9000000000000000001".into(),
            display_name: "Hoang".into(),
            ..Default::default()
        });
        let init = build_init_payload(&s, "init", true);
        assert_eq!(init["data"]["local"]["id"], "1000000000000000002");
        assert_eq!(init["data"]["local"]["name"], "Hoang");
    }

    #[test]
    fn pure_rust_invite_fields_match_zrtc_format() {
        let resp_401 = json!({
            "rtpIP": "171.244.25.121:4200",
            "rtcpIP": "171.244.25.121:4200",
            "sessId": "sess_test_123",
            "servers": [
                {
                    "rtpaddr": "171.244.128.61:4200",
                    "rtcpaddr": "171.244.128.61:4200",
                    "rtpIPv6": "2401:5f80:3fff:7::15:4200",
                    "rtcpIPv6": "2401:5f80:3fff:7::15:4200",
                    "spTcp": 1
                },
                {
                    "rtpaddr": "171.244.25.118:4200",
                    "rtcpaddr": "171.244.25.118:4200",
                    "rtpIPv6": "2401:5f80:4001:e::10:4200",
                    "rtcpIPv6": "2401:5f80:4001:e::10:4200",
                    "spTcp": 1
                }
            ]
        });
        let (rtp, rtcp, sess, codec, ext) = build_pure_rust_invite_fields(&resp_401, true);
        assert_eq!(rtp, "171.244.128.61:4200");
        assert_eq!(rtcp, "171.244.128.61:4200");
        assert_eq!(sess, "sess_test_123");
        assert!(codec.contains("opus/16000/1"));
        let ext_val: Value = serde_json::from_str(ext.trim()).unwrap();
        assert_eq!(ext_val["callType"], 1);
        assert_eq!(ext_val["newZrtc"], 1);
        assert_eq!(ext_val["serverAddr"][0]["rtp"], "171.244.128.61:4200");
        // Every 401 relay must be advertised: the official callee counts `serverAddr`
        // (`_doInitZRTP size`) and a single-entry list kills the call in <1 s.
        assert_eq!(ext_val["serverAddr"].as_array().map(Vec::len), Some(2));
        assert_eq!(ext_val["serverAddr"][1]["rtp"], "171.244.25.118:4200");
        assert_eq!(ext_val["serverResult"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn zrtp_udp_binary_packets_match_zalocall_layout() {
        assert_eq!(uid_str_to_u32("1000000000000000005"), 2808348677);

        let sess = "100000001-100000001@100000002";
        let caller_init = build_zrtp_init_packet(true, 100000001, 100000003, 100000010, 2, sess);
        assert_eq!(caller_init[0], 0x01);
        assert_eq!(caller_init[1], 0x7E);
        assert_eq!(
            u32::from_le_bytes(caller_init[10..14].try_into().unwrap()),
            100000001
        );
        assert_eq!(
            u16::from_le_bytes(caller_init[18..20].try_into().unwrap()),
            0x000B
        );
        assert_eq!(caller_init[20], 2);
        assert_eq!(
            u32::from_le_bytes(caller_init[21..25].try_into().unwrap()),
            100000010
        );
        assert_eq!(
            u32::from_le_bytes(caller_init[25..29].try_into().unwrap()),
            100000003
        );
        assert_eq!(
            u16::from_le_bytes(caller_init[29..31].try_into().unwrap()) as usize,
            sess.len()
        );
        assert_eq!(&caller_init[31..], sess.as_bytes());

        let callee_init = build_zrtp_init_packet(false, 100000001, 100000002, 100000010, 2, sess);
        assert_eq!(
            u16::from_le_bytes(callee_init[18..20].try_into().unwrap()),
            0x000C
        );
        let ping = build_zrtp_ping_packet(100000001, 100000010, sess);
        assert_eq!(ping[1], 0x01);
        assert_eq!(
            u16::from_le_bytes(ping[18..20].try_into().unwrap()),
            0x0002
        );
        assert_eq!(ping[20], 0x01);
        assert_eq!(&ping[23..], sess.as_bytes());

        let echo = build_zrtp_echo_packet(0x4E1F, "1761096000");
        assert_eq!(
            u32::from_le_bytes(echo[6..10].try_into().unwrap()),
            0x4E1F
        );
        assert_eq!(
            u32::from_le_bytes(echo[10..14].try_into().unwrap()),
            0x003E9765
        );
        assert_eq!(
            u32::from_le_bytes(echo[14..18].try_into().unwrap()),
            0x012F833C
        );
        assert_eq!(
            u16::from_le_bytes(echo[18..20].try_into().unwrap()),
            0x0005
        );

        let end_pkt = build_zrtp_end_packet(100000001, 100000010, sess);
        assert_eq!(
            u16::from_le_bytes(end_pkt[18..20].try_into().unwrap()),
            0x0020
        );
        assert_eq!(end_pkt[20], 0x09);
        assert_eq!(
            u32::from_le_bytes(end_pkt[21..25].try_into().unwrap()) as usize,
            sess.len()
        );
    }

    #[tokio::test]
    #[ignore]
    async fn live_call_dtc_trace() {
        let mut s = Session::new(true, false, false).unwrap();
        s.load().expect("load session.json");
        let partner_id = "1000000000000000005";
        let mut ctrl_state = CtrlPollState::default();
        let _ = poll_voip_controls(&s, &mut ctrl_state).await;
        println!("[TRACE] Initial ctrl cursor synced: {}", ctrl_state.last_action_id);

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let call_id = ((now_ms % 1_800_000_000) + 100_000_000) as i64;
        let req_401 = json!({
            "calleeId": partner_id,
            "callId": call_id,
            "codec": "[]\n",
            "type": 1
        });
        let resp_401 = dispatch_send_signal(&s, 401, &req_401).await.expect("401");
        println!(
            "[TRACE] 401 id={}, fromId={}, toId={}, rtpIP={}",
            resp_401["id"], resp_401["fromId"], resp_401["toId"], resp_401["rtpIP"]
        );

        let real_call_id = resp_401["id"].as_i64().unwrap_or(call_id);
        let from_uid_u32 = resp_401["fromId"].as_u64().unwrap() as u32;
        let partner_uid_u32 = resp_401["toId"].as_u64().unwrap() as u32;
        let sess_id_raw = resp_401["sessId"].as_str().unwrap().to_string();

        let rtp_sock = Arc::new(tokio::net::UdpSocket::bind("0.0.0.0:0").await.unwrap());
        let rtcp_sock = Arc::new(tokio::net::UdpSocket::bind("0.0.0.0:0").await.unwrap());
        let local_port = rtp_sock.local_addr().unwrap().port();
        let servers = collect_relay_servers(&resp_401);
        println!("[TRACE] Bound local UDP port={}, servers={:?}", local_port, servers);

        let pkt_init_rtp = build_zrtp_init_packet(false, from_uid_u32, partner_uid_u32, real_call_id as u32, 1, &sess_id_raw);
        let pkt_init_rtcp = build_zrtp_init_packet(true, from_uid_u32, partner_uid_u32, real_call_id as u32, 1, &sess_id_raw);
        let pkt_ping = build_zrtp_ping_packet(from_uid_u32, real_call_id as u32, &sess_id_raw);
        let pkt_echo = build_zrtp_echo_packet(0x4E1F, &crate::login::unix_now().to_string());

        for (rtp_addr, rtcp_addr) in &servers {
            let _ = rtp_sock.send_to(&pkt_init_rtp, rtp_addr.as_str()).await;
            let _ = rtp_sock.send_to(&pkt_echo, rtp_addr.as_str()).await;
            let _ = rtp_sock.send_to(&pkt_ping, rtp_addr.as_str()).await;
            let _ = rtcp_sock.send_to(&pkt_init_rtcp, rtcp_addr.as_str()).await;
            let _ = rtcp_sock.send_to(&pkt_ping, rtcp_addr.as_str()).await;
        }

        // Read UDP responses for 500ms before 416
        let start_wait = std::time::Instant::now();
        let mut buf = [0u8; 1500];
        while start_wait.elapsed() < std::time::Duration::from_millis(500) {
            match tokio::time::timeout(std::time::Duration::from_millis(80), rtp_sock.recv_from(&mut buf)).await {
                Ok(Ok((n, peer))) => {
                    println!("[TRACE] Pre-416 RTP recv {} bytes from {}: {:02x?}", n, peer, &buf[..n.min(48)]);
                }
                _ => {}
            }
            match tokio::time::timeout(std::time::Duration::from_millis(40), rtcp_sock.recv_from(&mut buf)).await {
                Ok(Ok((n, peer))) => {
                    println!("[TRACE] Pre-416 RTCP recv {} bytes from {}: {:02x?}", n, peer, &buf[..n.min(48)]);
                }
                _ => {}
            }
        }

        let (rtp_ip, rtcp_ip, sess_id, codec_str, extend_str) =
            build_pure_rust_invite_fields_with_port(&resp_401, false, Some(local_port));
        let req_416 = json!({
            "callId": real_call_id,
            "calleeId": partner_id,
            "codec": codec_str,
            "extendData": extend_str,
            "rtcpAddress": rtcp_ip,
            "rtpAddress": rtp_ip,
            "session": sess_id
        });
        let resp_416 = dispatch_send_signal(&s, 416, &req_416).await.expect("416");
        println!("[TRACE] resp_416 = {}", resp_416);

        // Spawn UDP reader + keepalive
        let rtp_c = rtp_sock.clone();
        let rtcp_c = rtcp_sock.clone();
        let servers_c = servers.clone();
        let sess_c = sess_id_raw.clone();
        let udp_task = tokio::spawn(async move {
            let mut b1 = [0u8; 1500];
            let mut b2 = [0u8; 1500];
            let mut iv = tokio::time::interval(std::time::Duration::from_millis(300));
            let mut seq = 1u32;
            loop {
                tokio::select! {
                    _ = iv.tick() => {
                        let p_ir = build_zrtp_init_packet(false, from_uid_u32, partner_uid_u32, real_call_id as u32, 1, &sess_c);
                        let p_ic = build_zrtp_init_packet(true, from_uid_u32, partner_uid_u32, real_call_id as u32, 1, &sess_c);
                        let p_p = build_zrtp_ping_packet(from_uid_u32, real_call_id as u32, &sess_c);
                        let p_e = build_zrtp_echo_packet(seq, &crate::login::unix_now().to_string());
                        seq += 1;
                        for (ra, ca) in &servers_c {
                            let _ = rtp_c.send_to(&p_ir, ra.as_str()).await;
                            let _ = rtp_c.send_to(&p_p, ra.as_str()).await;
                            let _ = rtp_c.send_to(&p_e, ra.as_str()).await;
                            let _ = rtcp_c.send_to(&p_ic, ca.as_str()).await;
                            let _ = rtcp_c.send_to(&p_p, ca.as_str()).await;
                        }
                    }
                    Ok((n, peer)) = rtp_c.recv_from(&mut b1) => {
                        println!("[TRACE] Post-416 RTP recv {} bytes from {}: {:02x?}", n, peer, &b1[..n.min(64)]);
                    }
                    Ok((n, peer)) = rtcp_c.recv_from(&mut b2) => {
                        println!("[TRACE] Post-416 RTCP recv {} bytes from {}: {:02x?}", n, peer, &b2[..n.min(64)]);
                    }
                }
            }
        });

        let poll_start = std::time::Instant::now();
        while poll_start.elapsed() < std::time::Duration::from_secs(18) {
            if let Ok(evs) = poll_voip_controls(&s, &mut ctrl_state).await {
                for ev in evs {
                    println!("[TRACE] VOIP CTRL EVENT @ {:?}: {}", poll_start.elapsed(), serde_json::to_string(&ev).unwrap());
                }
            }
        }
        udp_task.abort();
    }

    #[test]
    fn parse_zrtp_init_response_extracts_token_and_public_p2p() {
        let mut pkt = vec![0u8; 21];
        pkt[0] = 0x02;
        pkt[1] = 0x7E;
        pkt[18..20].copy_from_slice(&0x000Bu16.to_le_bytes());
        pkt.extend_from_slice(&0i32.to_le_bytes());
        pkt.extend_from_slice(&100000010u32.to_le_bytes());
        pkt.extend_from_slice(&0xCAFE_BABEu32.to_le_bytes());
        let pub_addr = b"203.0.113.10|9849";
        pkt.extend_from_slice(&(pub_addr.len() as u16).to_le_bytes());
        pkt.extend_from_slice(pub_addr);

        let parsed = parse_zrtp_init_response(&pkt).expect("valid InitZRTP response");
        assert_eq!(parsed.0, 0);
        assert_eq!(parsed.1, 100000010);
        assert_eq!(parsed.2, 0xCAFE_BABE);
        assert_eq!(parsed.3, Some(("203.0.113.10".to_string(), 9849)));
    }

    #[test]
    fn build_callee_extend_data_includes_select2side_and_public_p2p() {
        let mut probe = ZrtpProbeResult {
            local_port: 61234,
            public_p2p: Some(("203.0.113.20".into(), 61234)),
            ..Default::default()
        };
        probe.echo_stats.insert("42.113.206.4:10490".into(), (14, 18));
        let ext_raw = build_callee_extend_data(
            "42.113.206.4:10490",
            "42.113.206.4:12490",
            false,
            Some(61234),
            Some(&probe),
        );
        let ext: Value = serde_json::from_str(ext_raw.trim()).expect("callee extendData JSON");
        assert_eq!(ext["select2side"], 1);
        assert_eq!(ext["numServers"], 0);
        let p2p = ext["p2p"].as_array().expect("p2p array");
        assert!(p2p.iter().any(|e| e["type"] == 0 && e["port"] == 61234));
        assert!(p2p
            .iter()
            .any(|e| e["type"] == 1 && e["ip"] == "203.0.113.20" && e["port"] == 61234));
    }

    #[test]
    fn incoming_request_media_params_extract_rtp_ser_ip_and_video() {
        let req_params = json!({
            "rtpSerIp": "171.244.128.62:4200",
            "rtpIP": "171.244.128.62:4200",
            "sessId": "sess_incoming_456",
            "extendData": r#"{"callType":1,"serverAddr":[{"rtp":"171.244.128.62:4200","rtcp":"171.244.128.62:4200"}],"video":{"codec":[{"name":"h264","payload":97}]}}"#,
        });
        let (rtp, rtcp, sess, _, _) =
            build_pure_rust_invite_fields_with_probe(&req_params, true, Some(55000), None);
        assert_eq!(rtp, "171.244.128.62:4200");
        assert_eq!(rtcp, "171.244.128.62:4200");
        assert_eq!(sess, "sess_incoming_456");
    }
}
