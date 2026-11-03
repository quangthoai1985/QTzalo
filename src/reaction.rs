//! Message reactions (v0.1.3): one entry per person, and the reaction events Zalo pushes.
//!
//! Evidence: `re-notes/10_bang_chung_dong_bo.md` §17. Design:
//! `docs/superpowers/specs/2026-11-02-reaction-hover-realtime-design.md`.
//!
//! [O] Socket `612_0` (while open), the `610_1` / `611_1` pulls (after being offline) and the
//! `lp_control` long-poll all carry `data.reacts[]` (1-1) and `data.reactGroups[]` (groups):
//! `{ uidFrom ("0" = this account), idTo, ts, msgType: "chat.reaction",
//!    content: "<json>" → { rIcon, rType, source, rMsg: [{ gMsgID, cMsgID, msgType }] } }`.
//!
//! Only the newest messages of each conversation are kept in memory (`store::hydrate_threads`
//! loads at most 200 per conversation), so a reaction can arrive for a message that is not
//! loaded. Those events are no longer dropped on the floor: they go into a bounded FIFO
//! ([`apply_deferred`]) and are counted with rate-limited logging, then applied as soon as the
//! message shows up ("Tải thêm tin nhắn cũ").
//!
//! [`add_mine`] / [`remove_mine`] are optimistic: the bubble changes before the network round
//! trip. [`rollback_mine`], [`snapshot_mine`] and [`restore_mine`] are the pure API the GUI/worker
//! can call when the send fails; the `GuiEvent` wiring is still pending, see
//! `docs/dev/BAN_GIAO.md` §8 ("Gửi lỗi thì không hoàn tác trên máy").

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One person's current reaction on a message. `emoji` is empty when they removed it (kept as a
/// tombstone so an older replayed event cannot bring it back). `uid` is `"0"` for this account
/// and empty for rows stored before v0.1.3, whose reactor is unknown.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "MsgReactionRepr")]
pub struct MsgReaction {
    pub uid: String,
    pub emoji: String,
    pub ts: i64,
}

/// Stored forms: a bare emoji or Zalo code (before v0.1.3), or the full object.
#[derive(Deserialize)]
#[serde(untagged)]
enum MsgReactionRepr {
    Legacy(String),
    Full {
        #[serde(default)]
        uid: String,
        emoji: String,
        #[serde(default)]
        ts: i64,
    },
}

impl From<MsgReactionRepr> for MsgReaction {
    fn from(r: MsgReactionRepr) -> Self {
        match r {
            // Zalo codes (`/-strong`, `:>`) are ASCII; emoji never are.
            MsgReactionRepr::Legacy(s) => MsgReaction {
                uid: String::new(),
                emoji: if s.is_ascii() {
                    crate::chat::icon_to_emoji(&s).to_string()
                } else {
                    s
                },
                ts: 0,
            },
            MsgReactionRepr::Full { uid, emoji, ts } => MsgReaction { uid, emoji, ts },
        }
    }
}

/// One reaction change read from `612` / `610` / `611` / `lp_control`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionEvent {
    pub thread_id: String,
    pub is_group: bool,
    /// `gMsgID` of the target message, as a string (the server sends a number).
    pub msg_id: String,
    /// `"0"` = this account, from any device.
    pub uid: String,
    /// Empty = removed.
    pub emoji: String,
    /// Server timestamp of the event (ms).
    pub ts: i64,
    /// `cMsgID` of the target. It names the message when `gMsgID` is 0 (a reaction to one's own
    /// message, `re-notes/10` §19).
    pub cli_msg_id: String,
}

/// What the reaction chip shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReactionSummary {
    /// Emoji by number of taps, most first; ties keep first-seen order.
    pub emojis: Vec<(String, usize)>,
    /// Taps (Zalo counts every tap).
    pub total: usize,
    /// This account's latest reaction.
    pub mine: Option<String>,
}

/// String or number field as a string (`gMsgID`, `uidFrom`, `idTo` come as either).
///
/// An integral JSON float (`7001.0`, which a writer may emit for a 64-bit id) is normalised to
/// `"7001"`; otherwise a numeric `gMsgID` of `0.0` would become `"0.0"` and stop matching the
/// "gMsgID 0 → use cMsgID" rule below.
fn str_or_num(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(f) = n.as_f64().filter(|f| f.fract() == 0.0) {
                if f >= -(2f64.powi(63)) && f < 2f64.powi(63) {
                    (f as i64).to_string()
                } else {
                    n.to_string()
                }
            } else {
                n.to_string()
            }
        }
        _ => String::new(),
    }
}

/// Integer field (`ts`) that the server normally sends as a number but sometimes as a string.
/// A float (whole or fractional) is truncated, so one odd serialiser cannot make the event fall
/// through the `ts == 0` filter.
fn int_or_str(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        Some(Value::String(s)) => s
            .trim()
            .parse::<i64>()
            .ok()
            .or_else(|| s.trim().parse::<f64>().ok().map(|f| f as i64))
            .unwrap_or(0),
        _ => 0,
    }
}

/// JSON object stored either inline or as a JSON string (`data`, `content`).
fn json_value(v: Option<&Value>) -> Option<Value> {
    match v? {
        Value::String(s) => serde_json::from_str(s.trim()).ok(),
        other => Some(other.clone()),
    }
}

/// The node holding `reacts` / `reactGroups`: the value itself, its `data` (object or JSON
/// string), or `data.data`.
fn reaction_node(payload: &Value) -> Option<Value> {
    let has = |v: &Value| v.get("reacts").is_some() || v.get("reactGroups").is_some();
    if has(payload) {
        return Some(payload.clone());
    }
    let data = json_value(payload.get("data"))?;
    if has(&data) {
        return Some(data);
    }
    json_value(data.get("data")).filter(has)
}

/// Every reaction change in a socket / long-poll payload.
pub fn extract_events(payload: &Value, self_uid: &str, cloud_id: Option<&str>) -> Vec<ReactionEvent> {
    let mut out = Vec::new();
    let Some(node) = reaction_node(payload) else {
        return out;
    };
    let is_self = |uid: &str| uid == "0" || (!self_uid.is_empty() && uid == self_uid);
    for (key, is_group) in [("reacts", false), ("reactGroups", true)] {
        let Some(items) = node.get(key).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let uid_from = str_or_num(item.get("uidFrom"));
            let id_to = str_or_num(item.get("idTo"));
            let ts = int_or_str(item.get("ts"));
            if uid_from.is_empty() || ts == 0 {
                continue;
            }
            let thread_id = if is_group {
                id_to.trim_start_matches("group_").to_string()
            } else if is_self(&uid_from) {
                if id_to.is_empty() || is_self(&id_to) {
                    match cloud_id {
                        Some(c) if !c.is_empty() => c.to_string(),
                        _ => continue,
                    }
                } else {
                    id_to
                }
            } else {
                uid_from.clone()
            };
            if thread_id.is_empty() {
                continue;
            }
            let Some(content) = json_value(item.get("content")) else {
                continue;
            };
            let r_icon = content.get("rIcon").and_then(Value::as_str).unwrap_or("");
            let emoji = if r_icon.is_empty() {
                String::new()
            } else {
                crate::chat::icon_to_emoji(r_icon).to_string()
            };
            let uid = if is_self(&uid_from) { "0".to_string() } else { uid_from.clone() };
            let targets = content.get("rMsg").and_then(Value::as_array).cloned().unwrap_or_default();
            for target in &targets {
                let mut msg_id = str_or_num(target.get("gMsgID"));
                let cli_msg_id = str_or_num(target.get("cMsgID"));
                if msg_id == "0" {
                    msg_id.clear();
                }
                // `gMsgID` 0: a reaction to one's own message, named by its `cliMsgId` only.
                if msg_id.is_empty() && (cli_msg_id.is_empty() || cli_msg_id == "0") {
                    continue;
                }
                out.push(ReactionEvent {
                    thread_id: thread_id.clone(),
                    is_group,
                    msg_id,
                    uid: uid.clone(),
                    emoji: emoji.clone(),
                    ts,
                    cli_msg_id,
                });
            }
        }
    }
    out
}

/// Apply one server event. Like Zalo, every tap counts: each event is one entry, and the same
/// event delivered twice (same person, emoji and `ts`) counts once. A removal clears that
/// person's taps and stays as a marker (`emoji` empty), so older replays cannot bring them back.
/// Returns `true` if the stored data changed. Entries without a uid (pre-v0.1.3) are never
/// touched.
pub fn apply(reactions: &mut Vec<MsgReaction>, ev: &ReactionEvent) -> bool {
    if ev.uid.is_empty() {
        return false;
    }
    let removed_at = reactions
        .iter()
        .filter(|r| r.uid == ev.uid && r.emoji.is_empty())
        .map(|r| r.ts)
        .max();
    if removed_at.is_some_and(|t| t >= ev.ts) {
        return false;
    }
    if ev.emoji.is_empty() {
        reactions.retain(|r| r.uid != ev.uid || (!r.emoji.is_empty() && r.ts > ev.ts));
        reactions.push(MsgReaction { uid: ev.uid.clone(), emoji: String::new(), ts: ev.ts });
        return true;
    }
    if reactions.iter().any(|r| r.uid == ev.uid && r.emoji == ev.emoji && r.ts == ev.ts) {
        return false;
    }
    // The server's echo of a tap made in QTzalo settles that pending entry.
    if let Some(p) = reactions.iter_mut().find(|r| r.uid == ev.uid && r.emoji == ev.emoji && r.ts == 0) {
        p.ts = ev.ts;
        return true;
    }
    reactions.push(MsgReaction { uid: ev.uid.clone(), emoji: ev.emoji.clone(), ts: ev.ts });
    true
}

/// How many reaction events for messages outside the in-memory window are kept for a later
/// retry. Bounded, so a long session with plenty of traffic for old conversations cannot grow
/// memory without limit.
const MAX_DEFERRED: usize = 200;

/// Log the first missed event, then one line per this many: a burst of events for unloaded
/// conversations must not flood the dev log (`crate::logger`).
const MISSED_LOG_EVERY: u64 = 50;

static MISSED_EVENTS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Events that could not be applied because their conversation or message is not in memory
    /// (only the newest messages of each conversation are loaded). Replayed by
    /// [`apply_deferred`] once the target shows up.
    ///
    /// Thread-local on purpose: `GuiEvent::ReactionsReceived` is handled on the GUI thread, so one
    /// queue per thread is enough in production and unit tests stay independent of each other.
    static DEFERRED: RefCell<VecDeque<ReactionEvent>> = const { RefCell::new(VecDeque::new()) };
}

/// Snapshot of the deferred queue, for tests (`apply_deferred` keeps it private otherwise).
#[cfg(test)]
fn deferred_events() -> Vec<ReactionEvent> {
    DEFERRED.with(|q| q.borrow().iter().cloned().collect())
}

/// Empty the deferred queue so tests do not inherit another test's leftovers when libtest runs
/// them one at a time on the same thread (`--test-threads=1`).
#[cfg(test)]
fn clear_deferred() {
    DEFERRED.with(|q| q.borrow_mut().clear());
}

/// Rate-limit rule for [`note_missed`]: log the first miss, then every [`MISSED_LOG_EVERY`]th.
fn missed_log_due(count: u64) -> bool {
    count == 1 || count % MISSED_LOG_EVERY == 0
}

/// Remember one event that had no loaded target, so a later [`apply_deferred`] can apply it.
/// Returns `true` when it was not already queued (the socket, `lp_control` and the replay after a
/// restart can all carry the same change).
fn defer(ev: &ReactionEvent) -> bool {
    DEFERRED.with(|q| {
        let mut q = q.borrow_mut();
        if q.iter().any(|e| e == ev) {
            return false;
        }
        q.push_back(ev.clone());
        while q.len() > MAX_DEFERRED {
            q.pop_front();
        }
        true
    })
}

/// Count a newly deferred event and log it with rate limiting (dev mode only, via
/// `crate::logger`).
fn note_missed(ev: &ReactionEvent) {
    let count = MISSED_EVENTS.fetch_add(1, Ordering::Relaxed) + 1;
    if !missed_log_due(count) {
        return;
    }
    let target = if ev.msg_id.is_empty() {
        &ev.cli_msg_id
    } else {
        &ev.msg_id
    };
    crate::logger::log_error(
        "reaction::apply_to_threads",
        &format!(
            "deferred {count} event(s) with no loaded target: thread {}, msg {target}, uid {} (empty emoji = removal)",
            ev.thread_id, ev.uid
        ),
    );
}

/// Apply one event to its loaded message. `None` when the conversation or the message is not in
/// memory (the caller then defers it); `Some(changed)` when it was applied.
fn apply_event(threads: &mut [crate::chat::Thread], ev: &ReactionEvent) -> Option<bool> {
    let t = threads.iter_mut().find(|t| t.id == ev.thread_id)?;
    // By `gMsgID` when the event has one, else by `cliMsgId` (own messages).
    let target = |m: &&mut crate::chat::ChatMessage| {
        if ev.msg_id.is_empty() {
            !ev.cli_msg_id.is_empty() && m.cli_msg_id.as_deref() == Some(ev.cli_msg_id.as_str())
        } else {
            m.id == ev.msg_id
        }
    };
    let m = t.messages.iter_mut().find(target)?;
    Some(apply(&mut m.reactions, ev))
}

/// Apply the events deferred by earlier batches (their target was outside the in-memory window)
/// and keep the ones that still have no target. Returns `true` if any message changed.
///
/// [`apply_to_threads`] already retries this before every new batch, so calling it directly is
/// only needed to catch up immediately after loading more history
/// (`WorkerCmd::LoadGroupHistory`, "Tải thêm tin nhắn cũ").
pub fn apply_deferred(threads: &mut [crate::chat::Thread]) -> bool {
    let pending = DEFERRED.with(|q| std::mem::take(&mut *q.borrow_mut()));
    if pending.is_empty() {
        return false;
    }
    let mut changed = false;
    let mut still = VecDeque::with_capacity(pending.len());
    for ev in pending {
        match apply_event(threads, &ev) {
            Some(c) => changed |= c,
            None => still.push_back(ev),
        }
    }
    DEFERRED.with(|q| *q.borrow_mut() = still);
    changed
}

/// Apply events to the messages held in memory (the newest messages of each conversation).
///
/// An event for a conversation or message that is not loaded is queued (bounded FIFO,
/// [`apply_deferred`]) and logged with rate limiting instead of being dropped silently, so a
/// reaction on an older message is applied once that message is loaded. Returns `true` if any
/// message changed.
pub fn apply_to_threads(threads: &mut [crate::chat::Thread], events: &[ReactionEvent]) -> bool {
    let mut changed = apply_deferred(threads);
    for ev in events {
        match apply_event(threads, ev) {
            Some(c) => changed |= c,
            None => {
                if defer(ev) {
                    note_missed(ev);
                }
            }
        }
    }
    changed
}

/// A tap made in QTzalo, pending (`ts = 0`) until the server's echo gives it its real time.
pub fn add_mine(reactions: &mut Vec<MsgReaction>, emoji: &str) {
    reactions.push(MsgReaction { uid: "0".into(), emoji: emoji.to_string(), ts: 0 });
}

/// "Gỡ cảm xúc" in QTzalo: all my taps go, with a pending removal marker.
pub fn remove_mine(reactions: &mut Vec<MsgReaction>) {
    reactions.retain(|r| r.uid != "0");
    reactions.push(MsgReaction { uid: "0".into(), emoji: String::new(), ts: 0 });
}

/// Undo one optimistic change after the server rejected the send (`WorkerCmd::React` failure).
///
/// Removes **one** pending entry (`ts == 0`) of `uid` whose emoji is exactly `emoji` — the last
/// one, which is the tap that was just sent. Entries of other people, entries settled by the
/// server's echo (`ts != 0`), pending taps with another emoji and pre-v0.1.3 rows (empty `uid`,
/// reactor unknown) are never touched. Pass `emoji == ""` to undo a failed "gỡ cảm xúc": that
/// drops its pending removal marker. Returns `true` when an entry was removed; calling it twice
/// is harmless (the second call finds nothing).
///
/// For a failed removal the taps that [`remove_mine`] deleted are gone locally; capture them with
/// [`snapshot_mine`] before the optimistic call and put them back with [`restore_mine`].
///
/// Intended hook — **not wired yet** (this session only owns `reaction.rs`): the react failure
/// path in `gui/worker.rs` + `gui.rs` should find the message by `(thread_id, msg_id)` and call
/// this. Today that path reports `GuiEvent::StatusUpdate("Thả cảm xúc thất bại: …")`;
/// `GuiEvent::SendFailed` is the text-message variant and can carry a reaction only after a
/// follow-up change outside this file.
pub fn rollback_mine(msg: &mut crate::chat::ChatMessage, uid: &str, emoji: &str) -> bool {
    if uid.is_empty() {
        // Legacy rows have no reactor; there is nothing that could be "mine" to roll back.
        return false;
    }
    let Some(pos) = msg
        .reactions
        .iter()
        .rposition(|r| r.uid == uid && r.emoji == emoji && r.ts == 0)
    else {
        return false;
    };
    msg.reactions.remove(pos);
    true
}

/// This account's entries, to copy before an optimistic [`remove_mine`] whose failure would
/// otherwise lose them. Pair with [`restore_mine`].
pub fn snapshot_mine(reactions: &[MsgReaction], uid: &str) -> Vec<MsgReaction> {
    if uid.is_empty() {
        return Vec::new();
    }
    reactions.iter().filter(|r| r.uid == uid).cloned().collect()
}

/// Undo an optimistic [`remove_mine`] after the send failed: drop the optimistic state (including
/// the pending removal marker) and put the [`snapshot_mine`] entries back where this account's
/// first entry was, so the chip's first-seen order does not shuffle. Other people's entries and
/// legacy rows are kept as they are. Returns `true` when the list changed; a second call with the
/// same snapshot is a no-op.
pub fn restore_mine(reactions: &mut Vec<MsgReaction>, uid: &str, saved: &[MsgReaction]) -> bool {
    if uid.is_empty() {
        return false;
    }
    let first = reactions.iter().position(|r| r.uid == uid);
    let mut restored: Vec<MsgReaction> =
        reactions.iter().filter(|r| r.uid != uid).cloned().collect();
    match first {
        Some(pos) => {
            let pos = pos.min(restored.len());
            for (offset, entry) in saved.iter().enumerate() {
                restored.insert(pos + offset, entry.clone());
            }
        }
        None => restored.extend(saved.iter().cloned()),
    }
    if restored == *reactions {
        return false;
    }
    *reactions = restored;
    true
}

pub fn summary(reactions: &[MsgReaction]) -> ReactionSummary {
    let mut s = ReactionSummary::default();
    let mut mine_rank: Option<i64> = None;
    // A pre-v0.1.3 entry (no reactor) is most likely the same reaction a live event has since
    // attributed to someone: do not count it twice. Stored data keeps it.
    let shadowed = |r: &MsgReaction| {
        r.uid.is_empty() && reactions.iter().any(|o| !o.uid.is_empty() && o.emoji == r.emoji)
    };
    for r in reactions.iter().filter(|r| !r.emoji.is_empty() && !shadowed(r)) {
        s.total += 1;
        match s.emojis.iter_mut().find(|(e, _)| *e == r.emoji) {
            Some(entry) => entry.1 += 1,
            None => s.emojis.push((r.emoji.clone(), 1)),
        }
        // My latest tap; a pending one (ts 0) is the newest.
        let rank = if r.ts == 0 { i64::MAX } else { r.ts };
        if r.uid == "0" && mine_rank.is_none_or(|m| rank >= m) {
            mine_rank = Some(rank);
            s.mine = Some(r.emoji.clone());
        }
    }
    // Stable sort: equal counts keep first-seen order.
    s.emojis.sort_by(|a, b| b.1.cmp(&a.1));
    s
}

/// Chip tooltip: `❤️ 2 · 😆 1 — có bạn`.
pub fn summary_tooltip(s: &ReactionSummary) -> String {
    let mut text = s
        .emojis
        .iter()
        .map(|(e, n)| format!("{e} {n}"))
        .collect::<Vec<_>>()
        .join(" · ");
    if s.mine.is_some() && !text.is_empty() {
        text.push_str(" — có bạn");
    }
    text
}

/// Messages without a server id yet (still sending) cannot be reacted to.
pub fn can_react(msg: &crate::chat::ChatMessage) -> bool {
    !msg.id.is_empty() && !msg.id.starts_with("out_") && !msg.id.starts_with("local_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SELF_UID: &str = "111";
    const CLOUD: Option<&str> = Some("cloud1");

    fn content(r_icon: &str, targets: &[u64]) -> String {
        let r_msg: Vec<_> = targets
            .iter()
            .map(|g| json!({ "gMsgID": g, "cMsgID": 1, "msgType": 1 }))
            .collect();
        json!({ "rIcon": r_icon, "rType": 5, "source": 6, "msgSender": "x", "rMsg": r_msg })
            .to_string()
    }

    fn item(uid_from: &str, id_to: &str, ts: i64, r_icon: &str, targets: &[u64]) -> serde_json::Value {
        json!({
            "actionId": "1", "msgId": "2", "cliMsgId": "3", "msgType": "chat.reaction",
            "uidFrom": uid_from, "idTo": id_to, "ts": ts, "ttl": 0,
            "content": content(r_icon, targets),
        })
    }

    fn push_612(reacts: Vec<serde_json::Value>, groups: Vec<serde_json::Value>) -> serde_json::Value {
        json!({ "data": { "controls": [], "reacts": reacts, "reactGroups": groups }, "error_code": 0 })
    }

    fn ev(thread: &str, group: bool, msg: &str, uid: &str, emoji: &str, ts: i64) -> ReactionEvent {
        ReactionEvent {
            thread_id: thread.into(),
            is_group: group,
            msg_id: msg.into(),
            uid: uid.into(),
            emoji: emoji.into(),
            ts,
            cli_msg_id: "1".into(),
        }
    }

    #[test]
    fn own_message_reaction_is_identified_by_cli_msg_id() {
        // [O] 03/11: the phone reacting to a photo it sent itself uses gMsgID 0 and the photo's
        // cliMsgId (`re-notes/10` §19).
        let content = json!({
            "rType": 0, "rIcon": ":>", "msgSender": "130655484", "source": 0,
            "rMsg": [{ "cMsgID": 1793705979715_u64, "gMsgID": 0, "msgType": 32 }]
        })
        .to_string();
        let p = push_612(
            vec![json!({ "uidFrom": "0", "idTo": "222", "ts": "1793713381878", "content": content })],
            vec![],
        );
        let evs = extract_events(&p, SELF_UID, CLOUD);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].msg_id, "");
        assert_eq!(evs[0].cli_msg_id, "1793705979715");
        assert_eq!(evs[0].thread_id, "222");
    }

    #[test]
    fn apply_to_threads_matches_own_message_by_cli_msg_id() {
        clear_deferred();
        let mut t = thread_with("222", "8447783931887");
        t.messages[0].cli_msg_id = Some("1793705979715".into());
        let mut threads = vec![t];
        let mut e = ev("222", false, "", "0", "😆", 5);
        e.cli_msg_id = "1793705979715".into();
        assert!(apply_to_threads(&mut threads, &[e]));
        assert_eq!(threads[0].messages[0].reactions.len(), 1);
    }

    #[test]
    fn extracts_1_1_push_612() {
        let p = push_612(vec![item("0", "222", 1793628066838, "/-heart", &[7001])], vec![]);
        assert_eq!(
            extract_events(&p, SELF_UID, CLOUD),
            vec![ev("222", false, "7001", "0", "❤️", 1793628066838)]
        );
    }

    #[test]
    fn extracts_group_with_two_rmsg_and_data_as_json_string() {
        let data = json!({ "reacts": [], "reactGroups": [item("333", "group_9", 5, "/-strong", &[41, 42])] });
        let p = json!({ "data": data.to_string() });
        assert_eq!(
            extract_events(&p, SELF_UID, CLOUD),
            vec![ev("9", true, "41", "333", "👍", 5), ev("9", true, "42", "333", "👍", 5)]
        );
    }

    #[test]
    fn peer_reaction_threads_by_sender() {
        let p = push_612(vec![item("222", "0", 7, ":>", &[7002])], vec![]);
        assert_eq!(extract_events(&p, SELF_UID, CLOUD), vec![ev("222", false, "7002", "222", "😆", 7)]);
    }

    #[test]
    fn self_uin_is_normalised_to_zero() {
        let p = push_612(vec![item(SELF_UID, "222", 8, ":o", &[7003])], vec![]);
        assert_eq!(extract_events(&p, SELF_UID, CLOUD), vec![ev("222", false, "7003", "0", "😮", 8)]);
    }

    #[test]
    fn self_reaction_in_cloud_maps_to_cloud_thread() {
        let p = push_612(vec![item("0", SELF_UID, 9, "/-heart", &[7004])], vec![]);
        assert_eq!(extract_events(&p, SELF_UID, CLOUD), vec![ev("cloud1", false, "7004", "0", "❤️", 9)]);
        assert!(extract_events(&p, SELF_UID, None).is_empty());
    }

    #[test]
    fn skips_items_without_target_or_ts() {
        let mut no_ts = item("222", "0", 0, "/-heart", &[7005]);
        no_ts.as_object_mut().unwrap().remove("ts");
        // gMsgID 0 is a valid target only with a cMsgID (own messages); here both are 0.
        let mut no_target = item("222", "0", 10, "/-heart", &[0]);
        no_target["content"] = json!(json!({
            "rIcon": "/-heart", "rType": 5,
            "rMsg": [{ "gMsgID": 0, "cMsgID": 0, "msgType": 1 }]
        })
        .to_string());
        let p = push_612(vec![no_target, no_ts], vec![]);
        assert!(extract_events(&p, SELF_UID, CLOUD).is_empty());
    }

    #[test]
    fn empty_ricon_is_removal_and_unknown_code_is_heart() {
        let p = push_612(
            vec![item("222", "0", 11, "", &[7006]), item("222", "0", 12, "/-ok", &[7007])],
            vec![],
        );
        let evs = extract_events(&p, SELF_UID, CLOUD);
        assert_eq!(evs[0].emoji, "");
        assert_eq!(evs[1].emoji, "❤️");
    }

    #[test]
    fn taps_accumulate_like_zalo() {
        // [O] 03/11: three taps on ❤ on the phone are three events; another emoji is added,
        // not swapped (`re-notes/10` §19).
        let mut r = Vec::new();
        assert!(apply(&mut r, &ev("222", false, "1", "222", "❤️", 10)));
        assert!(apply(&mut r, &ev("222", false, "1", "222", "❤️", 11)));
        assert!(apply(&mut r, &ev("222", false, "1", "222", "😆", 20)));
        assert!(apply(&mut r, &ev("222", false, "1", "222", "❤️", 15)));
        let s = summary(&r);
        assert_eq!(s.total, 4);
        assert_eq!(s.emojis, vec![("❤️".to_string(), 3), ("😆".to_string(), 1)]);
    }

    #[test]
    fn same_event_delivered_twice_counts_once() {
        let mut r = Vec::new();
        assert!(apply(&mut r, &ev("222", false, "1", "222", "❤️", 10)));
        assert!(!apply(&mut r, &ev("222", false, "1", "222", "❤️", 10)));
        assert_eq!(summary(&r).total, 1);
    }

    #[test]
    fn removal_clears_every_tap_of_that_person_only() {
        let mut r = Vec::new();
        apply(&mut r, &ev("222", false, "1", "222", "❤️", 10));
        apply(&mut r, &ev("222", false, "1", "222", "😆", 11));
        apply(&mut r, &ev("222", false, "1", "333", "😮", 12));
        assert!(apply(&mut r, &ev("222", false, "1", "222", "", 20)));
        assert_eq!(summary(&r).emojis, vec![("😮".to_string(), 1)]);
        // Tapping again after the removal counts again.
        assert!(apply(&mut r, &ev("222", false, "1", "222", "❤️", 30)));
        assert_eq!(summary(&r).total, 2);
    }

    #[test]
    fn mine_is_my_latest_emoji_and_remove_mine_clears_it() {
        let mut r = Vec::new();
        apply(&mut r, &ev("222", false, "1", "0", "❤️", 10));
        apply(&mut r, &ev("222", false, "1", "0", "😆", 20));
        assert_eq!(summary(&r).mine.as_deref(), Some("😆"));
        add_mine(&mut r, "👍");
        assert_eq!(summary(&r).mine.as_deref(), Some("👍"));
        assert_eq!(summary(&r).total, 3);
        remove_mine(&mut r);
        let s = summary(&r);
        assert_eq!((s.total, s.mine), (0, None));
    }

    #[test]
    fn removal_leaves_tombstone_that_blocks_older_replay() {
        let mut r = Vec::new();
        apply(&mut r, &ev("222", false, "1", "222", "❤️", 10));
        apply(&mut r, &ev("222", false, "1", "222", "", 20));
        assert_eq!(summary(&r).total, 0);
        assert!(!apply(&mut r, &ev("222", false, "1", "222", "❤️", 10)));
        assert_eq!(summary(&r).total, 0);
    }

    #[test]
    fn add_mine_then_server_echo_counts_once() {
        let mut r = Vec::new();
        add_mine(&mut r, "👍");
        add_mine(&mut r, "👍");
        apply(&mut r, &ev("222", false, "1", "0", "👍", 50));
        apply(&mut r, &ev("222", false, "1", "0", "👍", 51));
        let s = summary(&r);
        assert_eq!(s.total, 2);
        assert_eq!(s.mine.as_deref(), Some("👍"));
        assert!(r.iter().all(|m| m.ts > 0));
    }

    #[test]
    fn apply_never_touches_legacy_entries() {
        let mut r = vec![MsgReaction { uid: String::new(), emoji: "❤️".into(), ts: 0 }];
        assert!(apply(&mut r, &ev("222", false, "1", "222", "😆", 5)));
        assert_eq!(summary(&r).total, 2);
        assert_eq!(r[0], MsgReaction { uid: String::new(), emoji: "❤️".into(), ts: 0 });
    }

    #[test]
    fn legacy_entry_is_not_counted_twice_with_same_emoji_person() {
        let legacy = || vec![MsgReaction { uid: String::new(), emoji: "❤️".into(), ts: 0 }];
        let mut r = legacy();
        apply(&mut r, &ev("222", false, "1", "222", "❤️", 5));
        assert_eq!(summary(&r).total, 1);
        let mut mine = legacy();
        add_mine(&mut mine, "❤️");
        let s = summary(&mine);
        assert_eq!(s.total, 1);
        assert_eq!(s.emojis, vec![("❤️".to_string(), 1)]);
        assert_eq!(s.mine.as_deref(), Some("❤️"));
    }

    #[test]
    fn summary_orders_by_count_then_first_seen_and_tooltip() {
        let r = vec![
            MsgReaction { uid: "a".into(), emoji: "❤️".into(), ts: 1 },
            MsgReaction { uid: "b".into(), emoji: "😆".into(), ts: 2 },
            MsgReaction { uid: "0".into(), emoji: "❤️".into(), ts: 3 },
        ];
        let s = summary(&r);
        assert_eq!(s.emojis, vec![("❤️".to_string(), 2), ("😆".to_string(), 1)]);
        assert_eq!(s.total, 3);
        assert_eq!(s.mine.as_deref(), Some("❤️"));
        assert_eq!(summary_tooltip(&s), "❤️ 2 · 😆 1 — có bạn");
        assert_eq!(summary_tooltip(&summary(&[])), "");
    }

    #[test]
    fn deserialises_legacy_strings_and_roundtrips_new_form() {
        let legacy: Vec<MsgReaction> = serde_json::from_str(r#"["❤️","/-strong"]"#).unwrap();
        assert_eq!(
            legacy,
            vec![
                MsgReaction { uid: String::new(), emoji: "❤️".into(), ts: 0 },
                MsgReaction { uid: String::new(), emoji: "👍".into(), ts: 0 },
            ]
        );
        let new_form = vec![MsgReaction { uid: "0".into(), emoji: "😆".into(), ts: 3 }];
        let text = serde_json::to_string(&new_form).unwrap();
        assert_eq!(serde_json::from_str::<Vec<MsgReaction>>(&text).unwrap(), new_form);
    }

    fn thread_with(id: &str, msg_id: &str) -> crate::chat::Thread {
        crate::chat::Thread {
            id: id.into(),
            name: String::new(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: vec![crate::chat::ChatMessage { id: msg_id.into(), ..Default::default() }],
        }
    }

    #[test]
    fn apply_to_threads_updates_matching_message() {
        clear_deferred();
        let mut threads = vec![thread_with("222", "7001")];
        assert!(apply_to_threads(&mut threads, &[ev("222", false, "7001", "222", "❤️", 5)]));
        assert_eq!(threads[0].messages[0].reactions.len(), 1);
    }

    #[test]
    fn apply_to_threads_ignores_unknown_thread_or_message() {
        clear_deferred();
        let mut threads = vec![thread_with("222", "7001")];
        let events = [ev("999", false, "7001", "222", "❤️", 5), ev("222", false, "1", "222", "❤️", 5)];
        assert!(!apply_to_threads(&mut threads, &events));
        assert!(threads[0].messages[0].reactions.is_empty());
        // Missing targets are queued for a later replay, not lost.
        assert_eq!(deferred_events(), events.to_vec());
    }

    #[test]
    fn can_react_rejects_local_ids() {
        let msg = |id: &str| crate::chat::ChatMessage { id: id.into(), ..Default::default() };
        assert!(!can_react(&msg("")));
        assert!(!can_react(&msg("out_1")));
        assert!(!can_react(&msg("local_2")));
        assert!(can_react(&msg("7001")));
    }

    // --- Server payload shapes that differ from the common one (BAN_GIAO §8) ----------------

    /// `content` as an object (not a JSON string), `uidFrom`/`idTo` as numbers and `gMsgID` as a
    /// string all name the same event.
    #[test]
    fn content_object_with_numeric_ids_and_string_gmsgid() {
        let p = push_612(
            vec![json!({
                "uidFrom": 222,
                "idTo": 0,
                "ts": "1793628066838",
                "content": {
                    "rIcon": "/-heart",
                    "rType": 5,
                    "source": 6,
                    "rMsg": [{ "gMsgID": "8447783931887", "cMsgID": "1793705979715", "msgType": 1 }],
                },
            })],
            vec![],
        );
        let mut expected = ev("222", false, "8447783931887", "222", "❤️", 1793628066838);
        expected.cli_msg_id = "1793705979715".into();
        assert_eq!(extract_events(&p, SELF_UID, CLOUD), vec![expected]);
    }

    /// Numeric `uidFrom` 0 (this account) plus object `content` inside `data` sent as a JSON
    /// string.
    #[test]
    fn numeric_self_uid_and_object_content_inside_json_string_data() {
        let data = json!({
            "reacts": [{
                "uidFrom": 0,
                "idTo": 222,
                "ts": 7,
                "content": { "rIcon": "/-strong", "rMsg": [{ "gMsgID": 42, "cMsgID": 0 }] },
            }],
            "reactGroups": [],
        });
        let p = json!({ "data": data.to_string() });
        let mut expected = ev("222", false, "42", "0", "👍", 7);
        expected.cli_msg_id = "0".into();
        assert_eq!(extract_events(&p, SELF_UID, CLOUD), vec![expected]);
    }

    /// A JSON writer may serialise 64-bit ids and `ts` as floats; `7001.0` must still match the
    /// message id `"7001"`, and `gMsgID 0.0` must still fall back to `cMsgID`.
    #[test]
    fn integral_float_ids_and_ts_are_normalised() {
        let p = push_612(
            vec![json!({
                "uidFrom": 222.0,
                "idTo": 0.0,
                "ts": 1793628066838.0,
                "content": { "rIcon": "/-heart", "rMsg": [{ "gMsgID": 7001.0, "cMsgID": 0.0 }] },
            })],
            vec![],
        );
        let mut expected = ev("222", false, "7001", "222", "❤️", 1793628066838);
        expected.cli_msg_id = "0".into();
        assert_eq!(extract_events(&p, SELF_UID, CLOUD), vec![expected]);

        let p = push_612(
            vec![json!({
                "uidFrom": "0",
                "idTo": "222",
                "ts": 12,
                "content": { "rIcon": ":>", "rMsg": [{ "gMsgID": 0.0, "cMsgID": 1793705979715_u64 }] },
            })],
            vec![],
        );
        let evs = extract_events(&p, SELF_UID, CLOUD);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].msg_id, "", "gMsgID 0.0 is still 0, not id \"0.0\"");
        assert_eq!(evs[0].cli_msg_id, "1793705979715");
        assert_eq!(evs[0].thread_id, "222");
    }

    /// Garbage `content` values are ignored instead of panicking or turning into a removal.
    #[test]
    fn malformed_content_is_ignored() {
        let reacts: Vec<_> = [json!(123), json!("not json"), json!([]), json!(null), json!({ "rMsg": [] })]
            .into_iter()
            .map(|content| json!({ "uidFrom": "222", "idTo": "0", "ts": 5, "content": content }))
            .collect();
        assert!(extract_events(&push_612(reacts, vec![]), SELF_UID, CLOUD).is_empty());
    }

    // --- Events outside the in-memory window are queued instead of lost ----------------------

    #[test]
    fn missed_event_is_queued_once_and_replayed_when_the_message_appears() {
        clear_deferred();
        let mut threads = vec![thread_with("222", "7001")];
        let missing = ev("222", false, "9999", "222", "❤️", 5);

        assert!(!apply_to_threads(&mut threads, std::slice::from_ref(&missing)));
        assert_eq!(deferred_events(), vec![missing.clone()]);
        // The same change from the socket and from `lp_control` is queued only once.
        assert!(!apply_to_threads(&mut threads, std::slice::from_ref(&missing)));
        assert_eq!(deferred_events().len(), 1);

        // "Tải thêm tin nhắn cũ" brings the message; a later batch replays the queue.
        threads[0]
            .messages
            .push(crate::chat::ChatMessage { id: "9999".into(), ..Default::default() });
        assert!(apply_to_threads(&mut threads, &[]));
        assert!(deferred_events().is_empty());
        assert_eq!(threads[0].messages[1].reactions.len(), 1);
        assert!(!apply_deferred(&mut threads), "nothing left to replay");
    }

    #[test]
    fn deferred_queue_is_bounded_and_keeps_the_newest_events() {
        clear_deferred();
        let mut threads: Vec<crate::chat::Thread> = Vec::new();
        for i in 0..(MAX_DEFERRED + 5) {
            let e = ev("999", false, &format!("{}", 10_000 + i), "222", "❤️", i as i64 + 1);
            assert!(!apply_to_threads(&mut threads, std::slice::from_ref(&e)));
        }
        let queued = deferred_events();
        assert_eq!(queued.len(), MAX_DEFERRED);
        assert_eq!(queued[0].msg_id, "10005", "oldest five were evicted");
        assert_eq!(
            queued[MAX_DEFERRED - 1].msg_id,
            format!("{}", 10_000 + MAX_DEFERRED + 4),
            "the newest event is still queued"
        );
        clear_deferred();
    }

    #[test]
    fn missed_event_logging_is_rate_limited() {
        assert!(missed_log_due(1), "the first miss is always logged");
        assert!(!missed_log_due(2));
        assert!(!missed_log_due(MISSED_LOG_EVERY - 1));
        assert!(missed_log_due(MISSED_LOG_EVERY));
        assert!(!missed_log_due(MISSED_LOG_EVERY + 1));
        assert!(missed_log_due(MISSED_LOG_EVERY * 2));
    }

    // --- Rollback of optimistic taps / removals when the send fails --------------------------

    #[test]
    fn rollback_mine_removes_only_my_pending_entry() {
        let pending = MsgReaction { uid: "0".into(), emoji: "👍".into(), ts: 0 };
        let settled = MsgReaction { uid: "0".into(), emoji: "👍".into(), ts: 1_793_550_037_212 };
        let peer = MsgReaction { uid: "222".into(), emoji: "👍".into(), ts: 5 };
        let legacy = MsgReaction { uid: String::new(), emoji: "👍".into(), ts: 0 };
        let pending_other_emoji = MsgReaction { uid: "0".into(), emoji: "😆".into(), ts: 0 };
        let mut msg = crate::chat::ChatMessage {
            reactions: vec![
                pending,
                settled.clone(),
                peer.clone(),
                legacy.clone(),
                pending_other_emoji.clone(),
            ],
            ..Default::default()
        };

        assert!(rollback_mine(&mut msg, "0", "👍"));
        assert_eq!(
            msg.reactions,
            vec![settled, peer, legacy, pending_other_emoji],
            "only my pending tap of this emoji goes"
        );
        // Idempotent: a repeated rollback for the same failed tap changes nothing.
        assert!(!rollback_mine(&mut msg, "0", "👍"));
        assert_eq!(msg.reactions.len(), 4);
        // Pre-v0.1.3 rows have no reactor (`uid` empty), so they can never be rolled back.
        assert!(!rollback_mine(&mut msg, "", "👍"));
        assert!(msg.reactions.iter().any(|r| r.uid.is_empty()));
    }

    #[test]
    fn rollback_mine_clears_a_failed_removal_marker() {
        let peer = MsgReaction { uid: "222".into(), emoji: "❤️".into(), ts: 9 };
        let mut msg = crate::chat::ChatMessage {
            reactions: vec![
                peer.clone(),
                MsgReaction { uid: "0".into(), emoji: String::new(), ts: 0 },
            ],
            ..Default::default()
        };

        assert!(rollback_mine(&mut msg, "0", ""));
        assert_eq!(msg.reactions, vec![peer]);
        assert_eq!(summary(&msg.reactions).total, 1);
        assert!(!rollback_mine(&mut msg, "0", ""));
    }

    #[test]
    fn snapshot_and_restore_undo_a_failed_removal() {
        let peer = MsgReaction { uid: "222".into(), emoji: "❤️".into(), ts: 10 };
        let mine_love = MsgReaction { uid: "0".into(), emoji: "❤️".into(), ts: 20 };
        let mine_laugh = MsgReaction { uid: "0".into(), emoji: "😆".into(), ts: 21 };
        let mut reactions = vec![peer.clone(), mine_love.clone(), mine_laugh.clone()];

        let saved = snapshot_mine(&reactions, "0");
        assert_eq!(saved, vec![mine_love.clone(), mine_laugh.clone()]);
        remove_mine(&mut reactions); // optimistic: my taps go, a pending removal marker stays
        assert_eq!(summary(&reactions).mine, None);
        assert!(reactions.iter().any(|r| r.uid == "0" && r.emoji.is_empty() && r.ts == 0));

        assert!(restore_mine(&mut reactions, "0", &saved));
        assert_eq!(reactions, vec![peer, mine_love, mine_laugh]);
        assert_eq!(summary(&reactions).mine.as_deref(), Some("😆"));
        // Idempotent with the same snapshot; an empty uid cannot restore anything.
        assert!(!restore_mine(&mut reactions, "0", &saved));
        assert!(!restore_mine(&mut reactions, "", &saved));
        assert!(snapshot_mine(&reactions, "").is_empty());
    }
}
