//! Message reactions (v0.1.3): one entry per person, and the reaction events Zalo pushes.
//!
//! Evidence: `re-notes/10_bang_chung_dong_bo.md` §17. Design:
//! `docs/superpowers/specs/2026-11-02-reaction-hover-realtime-design.md`.
//!
//! [O] Socket `612_0` (while open), the `610_1` / `611_1` pulls (after being offline) and the
//! `lp_control` long-poll all carry `data.reacts[]` (1-1) and `data.reactGroups[]` (groups):
//! `{ uidFrom ("0" = this account), idTo, ts, msgType: "chat.reaction",
//!    content: "<json>" → { rIcon, rType, source, rMsg: [{ gMsgID, cMsgID, msgType }] } }`.

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
}

/// What the reaction chip shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReactionSummary {
    /// Emoji by number of people, most first; ties keep first-seen order.
    pub emojis: Vec<(String, usize)>,
    /// People with a reaction.
    pub total: usize,
    /// This account's reaction.
    pub mine: Option<String>,
}

/// String or number field as a string (`gMsgID`, `uidFrom`, `idTo` come as either).
fn str_or_num(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn int_or_str(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
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
                let msg_id = str_or_num(target.get("gMsgID"));
                if msg_id.is_empty() || msg_id == "0" {
                    continue;
                }
                out.push(ReactionEvent {
                    thread_id: thread_id.clone(),
                    is_group,
                    msg_id,
                    uid: uid.clone(),
                    emoji: emoji.clone(),
                    ts,
                });
            }
        }
    }
    out
}

/// Apply one server event: each person keeps their newest reaction. Returns `true` if the
/// stored data changed. Entries without a uid (pre-v0.1.3) are never touched.
pub fn apply(reactions: &mut Vec<MsgReaction>, ev: &ReactionEvent) -> bool {
    if ev.uid.is_empty() {
        return false;
    }
    match reactions.iter_mut().find(|r| r.uid == ev.uid) {
        Some(r) => {
            if ev.ts < r.ts || (r.emoji == ev.emoji && r.ts == ev.ts) {
                return false;
            }
            r.emoji = ev.emoji.clone();
            r.ts = ev.ts;
            true
        }
        None => {
            reactions.push(MsgReaction { uid: ev.uid.clone(), emoji: ev.emoji.clone(), ts: ev.ts });
            true
        }
    }
}

/// Apply events to the messages held in memory (the newest messages of each conversation).
/// Events for a conversation or message that is not loaded are dropped. Returns `true` if any
/// message changed.
pub fn apply_to_threads(threads: &mut [crate::chat::Thread], events: &[ReactionEvent]) -> bool {
    let mut changed = false;
    for ev in events {
        let Some(t) = threads.iter_mut().find(|t| t.id == ev.thread_id) else {
            continue;
        };
        if let Some(m) = t.messages.iter_mut().find(|m| m.id == ev.msg_id) {
            changed |= apply(&mut m.reactions, ev);
        }
    }
    changed
}

/// This account's reaction set from QTzalo (`""` = removed). `ts = 0` so the server's own event
/// for it, which carries the real time, always wins.
pub fn set_mine(reactions: &mut Vec<MsgReaction>, emoji: &str) {
    match reactions.iter_mut().find(|r| r.uid == "0") {
        Some(r) => {
            r.emoji = emoji.to_string();
            r.ts = 0;
        }
        None => reactions.push(MsgReaction { uid: "0".into(), emoji: emoji.to_string(), ts: 0 }),
    }
}

pub fn summary(reactions: &[MsgReaction]) -> ReactionSummary {
    let mut s = ReactionSummary::default();
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
        if r.uid == "0" {
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
        }
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
        let p = push_612(vec![item("222", "0", 10, "/-heart", &[0]), no_ts], vec![]);
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
    fn apply_latest_ts_wins_and_older_replay_is_ignored() {
        let mut r = Vec::new();
        assert!(apply(&mut r, &ev("222", false, "1", "222", "❤️", 10)));
        assert!(apply(&mut r, &ev("222", false, "1", "222", "😆", 20)));
        assert!(!apply(&mut r, &ev("222", false, "1", "222", "❤️", 15)));
        assert_eq!(r, vec![MsgReaction { uid: "222".into(), emoji: "😆".into(), ts: 20 }]);
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
    fn set_mine_then_server_echo_counts_once() {
        let mut r = Vec::new();
        set_mine(&mut r, "👍");
        apply(&mut r, &ev("222", false, "1", "0", "👍", 50));
        let s = summary(&r);
        assert_eq!(s.total, 1);
        assert_eq!(s.mine.as_deref(), Some("👍"));
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
        set_mine(&mut mine, "❤️");
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
        let mut threads = vec![thread_with("222", "7001")];
        assert!(apply_to_threads(&mut threads, &[ev("222", false, "7001", "222", "❤️", 5)]));
        assert_eq!(threads[0].messages[0].reactions.len(), 1);
    }

    #[test]
    fn apply_to_threads_ignores_unknown_thread_or_message() {
        let mut threads = vec![thread_with("222", "7001")];
        let events = [ev("999", false, "7001", "222", "❤️", 5), ev("222", false, "1", "222", "❤️", 5)];
        assert!(!apply_to_threads(&mut threads, &events));
        assert!(threads[0].messages[0].reactions.is_empty());
    }

    #[test]
    fn can_react_rejects_local_ids() {
        let msg = |id: &str| crate::chat::ChatMessage { id: id.into(), ..Default::default() };
        assert!(!can_react(&msg("")));
        assert!(!can_react(&msg("out_1")));
        assert!(!can_react(&msg("local_2")));
        assert!(can_react(&msg("7001")));
    }
}
