# Thả cảm xúc khi rê chuột + reaction realtime — kế hoạch triển khai

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rê chuột vào tin thì có nút và thanh 6 cảm xúc. Reaction của mọi người (kể cả từ điện thoại) hiện ngay qua socket `612` / `610` / `611` và `lp_control`, đếm theo số người.

**Architecture:**
- Module thuần mới `src/reaction.rs` lo dữ liệu: kiểu `MsgReaction`/`ReactionEvent`, tách sự kiện từ payload, áp dụng theo "mỗi người một mục, `ts` mới thắng", tóm tắt cho chip.
- `ChatMessage.reactions` đổi sang `Vec<MsgReaction>`, đọc được dạng cũ.
- Worker chuyển sự kiện cho GUI qua `GuiEvent::ReactionsReceived`. GUI áp dụng vào tin trong bộ nhớ rồi lưu bằng `persist_threads` có sẵn.
- Giao diện rê chuột nằm trong `gui/messages.rs`: một nút tròn ngoài mép bong bóng, và một `egui::Area` cho thanh cảm xúc.

**Tech Stack:** Rust 1.88+, eframe/egui 0.29, serde/serde_json, tokio, SQLite (rusqlite).

**Spec:** `docs/superpowers/specs/2026-11-02-reaction-hover-realtime-design.md`. Bằng chứng: `re-notes/10_bang_chung_dong_bo.md` §17.

## Global Constraints

- MSRV `rust-version = "1.88"`: `cargo +1.88 check --locked --all-targets` phải sạch. **Không thêm crate mới.**
- Build trên Windows cần `CMAKE_POLICY_VERSION_MINIMUM=3.5`. Mọi lệnh `cargo` dùng `--locked`.
- Module mới phải khai báo ở **cả** `src/lib.rs` (`pub mod reaction;`) và `src/main.rs` (`mod reaction;`).
- `uid = "0"` là chính chủ tài khoản; `uid = ""` là dữ liệu cũ không rõ người thả; `emoji = ""` là đã gỡ (tombstone, không hiển thị).
- Chuỗi giao diện (tiếng Việt, đúng nguyên văn):
  - tên cảm xúc lấy từ `REACTION_LIST`: "Thích", "Yêu thích", "Haha", "Wow", "Buồn", "Phẫn nộ";
  - nút gỡ: "Gỡ cảm xúc";
  - trạng thái: "Đã thả cảm xúc", "Đã gỡ cảm xúc";
  - đuôi tooltip của chip: " — có bạn".
- Số đo giao diện:
  - nút tròn 24 px, cách mép bong bóng 6 px, đáy thẳng đáy bong bóng;
  - emoji trên thanh 28 px, 34 px khi rê qua;
  - mở thanh sau khi rê lên nút 250 ms; đóng khi rời nút và thanh quá 200 ms;
  - icon 👍 mờ ở alpha khoảng 0.45.
- Không ghi nội dung tin vào log/trace. Không commit trace thô hay mã của Zalo.
- Commit theo Conventional Commits, tác giả `git -c user.name=Claude -c user.email=noreply@anthropic.com commit`, thân kết thúc bằng `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. QTzalo tự thả cảm xúc, rồi máy chủ gửi lại sự kiện mang `uidFrom` là `"0"` **hoặc** là `login.uin` → vẫn chỉ một mục của mình, chip không đếm đôi. Test ở Task 1.
2. Sự kiện cho hội thoại/tin không có trong bộ nhớ, hoặc `gMsgID` rỗng/`"0"` → bỏ qua, không panic, không đổi gì. Test ở Task 3.
3. `rIcon` là mã lạ (Zalo có nhiều mã hơn 6 cái) → hiện ❤️, không mất sự kiện. Tin đang có mục cũ `uid ""` → mục cũ vẫn giữ. Test ở Task 1.
4. Bong bóng bị cuộn khuất một phần dưới header/composer, hoặc menu chuột phải đang mở → nút cảm xúc không hiện ở ngoài vùng chat và không chặn menu. Kiểm thủ công ở Task 4.
5. Di chuột nhanh qua nhiều bong bóng liền nhau → mỗi lúc chỉ một thanh mở, thanh cũ đóng, nhấp đúp để trả lời vẫn chạy. Kiểm thủ công ở Task 4.

---

### Task 1: Module `src/reaction.rs` — dữ liệu và hàm thuần

**Files:**
- Create: `src/reaction.rs`
- Modify: `src/lib.rs` (thêm `pub mod reaction;` theo thứ tự chữ cái), `src/main.rs` (thêm `mod reaction;`)

**Interfaces:**
- Produces:
  - `pub struct MsgReaction { pub uid: String, pub emoji: String, pub ts: i64 }`. Derive `Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize`, kèm `#[serde(from = "MsgReactionRepr")]`.
    - `MsgReactionRepr` là enum `#[serde(untagged)]`: `Legacy(String)` | `Full { #[serde(default)] uid: String, emoji: String, #[serde(default)] ts: i64 }`.
    - Chuyển dạng `Legacy(s)` thành `{uid: "", emoji: if s.is_ascii() { chat::icon_to_emoji(&s) } else { s }, ts: 0}`.
  - `pub struct ReactionEvent { pub thread_id: String, pub is_group: bool, pub msg_id: String, pub uid: String, pub emoji: String, pub ts: i64 }`. Derive `Debug, Clone, PartialEq, Eq`.
  - `pub struct ReactionSummary { pub emojis: Vec<(String, usize)>, pub total: usize, pub mine: Option<String> }`.
  - `pub fn extract_events(payload: &serde_json::Value, self_uid: &str, cloud_id: Option<&str>) -> Vec<ReactionEvent>`
  - `pub fn apply(reactions: &mut Vec<MsgReaction>, ev: &ReactionEvent) -> bool`
  - `pub fn set_mine(reactions: &mut Vec<MsgReaction>, emoji: &str)`
  - `pub fn summary(reactions: &[MsgReaction]) -> ReactionSummary`
  - `pub fn summary_tooltip(s: &ReactionSummary) -> String`
  - `pub fn can_react(msg: &crate::chat::ChatMessage) -> bool`

Quy tắc đã chốt (từ spec §3):
- `extract_events`:
  - Tìm `reacts` (1-1) và `reactGroups` (nhóm) ở chính node, hoặc ở `node["data"]` (object, hoặc chuỗi JSON cần parse), hoặc ở `node["data"]["data"]`.
  - `content` là chuỗi JSON hoặc object. `rIcon` mặc định `""`. Mỗi mục trong `content.rMsg[]` tạo một sự kiện; `gMsgID` là số hoặc chuỗi, chuyển thành chuỗi.
  - Bỏ phần tử nếu `gMsgID` rỗng hoặc `"0"`, `ts` thiếu hoặc bằng 0, hoặc `uidFrom` thiếu. `ts`, `uidFrom`, `idTo` chấp nhận cả số lẫn chuỗi.
  - `uid` = `"0"` nếu `uidFrom ∈ {"0", self_uid}`, ngược lại giữ `uidFrom`.
  - `thread_id`: nhóm → `idTo` bỏ tiền tố `group_`. 1-1, do mình thả → `idTo`, nhưng nếu `idTo ∈ {"", "0", self_uid}` thì là `cloud_id`, và không có `cloud_id` thì bỏ sự kiện. 1-1, người khác thả → `uidFrom`.
  - `emoji` = `""` nếu `rIcon` rỗng, ngược lại `chat::icon_to_emoji(rIcon)`.
- `apply`:
  - Tìm mục cùng `uid`. Nếu `ev.ts < mục.ts` → `false`. Nếu `emoji` và `ts` đều giống → `false`. Ngược lại ghi đè → `true`.
  - Chưa có mục cùng `uid` → thêm mới, kể cả khi `emoji` rỗng (tombstone) → `true`.
  - Không bao giờ đụng mục `uid ""`.
- `set_mine`: ghi mục `uid "0"` thành `{emoji, ts: 0}`, chưa có thì thêm. Không so `ts`.
- `summary`:
  - Bỏ qua mục có `emoji` rỗng.
  - `emojis` sắp theo số người giảm dần; bằng nhau thì giữ thứ tự xuất hiện đầu tiên.
  - `total` = số mục có emoji. `mine` = emoji của mục `uid "0"` nếu khác rỗng.
- `summary_tooltip`: nối `"{emoji} {n}"` bằng `" · "`, thêm `" — có bạn"` nếu `mine.is_some()`. Chưa ai thả thì trả chuỗi rỗng.
- `can_react`: `false` nếu `id` rỗng, hoặc bắt đầu bằng `out_` hay `local_`.

- [ ] **Step 1: Viết test (thất bại) trong `#[cfg(test)] mod tests` của `src/reaction.rs`**

Dựng payload theo đúng cấu trúc `re-notes/10` §17: `content` là chuỗi JSON, `gMsgID`/`cMsgID` là số.

```rust
// self_uid = "111", cloud_id = Some("cloud1")
#[test] fn extracts_1_1_push_612() {
    // {"data":{"reacts":[{"uidFrom":"0","idTo":"222","ts":1793628066838,"msgType":"chat.reaction",
    //   "content":"{\"rIcon\":\"/-heart\",\"rType\":5,\"source\":6,\"msgSender\":\"x\",\"rMsg\":[{\"gMsgID\":7001,\"cMsgID\":1,\"msgType\":1}]}"}],
    //   "reactGroups":[]}}
    // → [ReactionEvent{thread_id:"222", is_group:false, msg_id:"7001", uid:"0", emoji:"❤️", ts:1793628066838}]
}
#[test] fn extracts_group_with_two_rmsg_and_data_as_json_string() { /* reactGroups, idTo "group_9", uidFrom "333", 2 rMsg → 2 events, thread "9", uid "333" */ }
#[test] fn peer_reaction_threads_by_sender() { /* reacts, uidFrom "222", idTo "0" → thread "222", uid "222" */ }
#[test] fn self_uin_is_normalised_to_zero() { /* uidFrom "111" (= self_uid), idTo "222" → uid "0", thread "222" */ }
#[test] fn self_reaction_in_cloud_maps_to_cloud_thread() { /* uidFrom "0", idTo "111" → thread "cloud1" */ }
#[test] fn skips_items_without_target_or_ts() { /* gMsgID 0, missing ts → [] */ }
#[test] fn empty_ricon_is_removal_and_unknown_code_is_heart() { /* rIcon "" → emoji ""; rIcon "/-ok" → "❤️" */ }
#[test] fn apply_latest_ts_wins_and_older_replay_is_ignored() {
    // apply ❤️@10 → true; apply 😆@20 → true (one entry, 😆); apply ❤️@15 → false (still 😆)
}
#[test] fn removal_leaves_tombstone_that_blocks_older_replay() {
    // ❤️@10, ""@20 → summary.total == 0; apply ❤️@10 → false; total still 0
}
#[test] fn set_mine_then_server_echo_counts_once() {
    // set_mine("👍"); apply(uid "0", "👍", ts 50) → summary.total == 1, mine == Some("👍")
}
#[test] fn apply_never_touches_legacy_entries() {
    // reactions = [legacy {uid:"", emoji:"❤️", ts:0}]; apply(uid "222", "😆", 5) → total 2
}
#[test] fn summary_orders_by_count_then_first_seen_and_tooltip() {
    // ❤️(a), 😆(b), ❤️(0) → emojis [("❤️",2),("😆",1)], total 3, mine Some("❤️")
    // summary_tooltip == "❤️ 2 · 😆 1 — có bạn"
}
#[test] fn deserialises_legacy_strings_and_roundtrips_new_form() {
    // from_str::<Vec<MsgReaction>>(r#"["❤️","/-strong"]"#) → [{"" ,"❤️",0},{"","👍",0}]
    // serialize vec![MsgReaction{uid:"0",emoji:"😆",ts:3}] then deserialize → equal
}
#[test] fn can_react_rejects_local_ids() { /* "" | "out_1" | "local_2" → false; "7001" → true */ }
```

- [ ] **Step 2: Chạy test, thấy thất bại**

Run: `cargo test --locked --lib reaction::`
Expected: lỗi biên dịch (các hàm/kiểu chưa có).

- [ ] **Step 3: Viết các kiểu và hàm ở phần Interfaces theo đúng quy tắc trên, thêm khai báo module vào `lib.rs` và `main.rs`**

- [ ] **Step 4: Chạy test, thấy qua**

Run: `cargo test --locked --lib reaction::`
Expected: 14 test PASS.

- [ ] **Step 5: Commit** — `feat(reaction): per-person reaction model and event parser`

---

### Task 2: Đổi `ChatMessage.reactions` sang `Vec<MsgReaction>` và sửa ngữ nghĩa thả

**Files:**
- Modify: `src/chat.rs` (struct `:31`; `extract_reactions` `:4236`; luật gộp `:232` và `:4480`; `reaction_code_and_type` `:2380`)
- Modify: `src/store.rs` (đọc `:446`, ghi `:581`; luật gộp `:534`; thêm test)
- Modify: `src/gui.rs` (`GuiEvent::Reacted` `:1199`)
- Modify: `src/gui/workspace.rs` (`BubbleAction::React` `:1447`)
- Modify: `src/gui/messages.rs`:
  - `show_timestamp` `:1523`, `marks_w` `:1717`, lề dưới `:1773`: dùng `summary(&msg.reactions).total > 0` thay cho `!msg.reactions.is_empty()`;
  - `paint_reaction_chip` `:1999`;
  - xoá khối `has_react` chết trong `render_reactions` `:3013` và `:3055-3071`.
- Modify: `src/tui.rs` (`:860`, `:1414`, `:2001`, literal trong test `:2608`), `src/gui/demo.rs` (`:161`)

**Interfaces:**
- Consumes (Task 1): `MsgReaction`, `set_mine`, `summary`, `summary_tooltip`.
- Produces:
  - `ChatMessage.reactions: Vec<crate::reaction::MsgReaction>`, giữ `#[serde(default, skip_serializing_if = "Vec::is_empty")]`;
  - `chat::reaction_code_and_type("") == ("", -1)`.

- [ ] **Step 1: Viết test thất bại**
  - `store.rs`: `reactions_roundtrip_through_raw_json`. Ghi một `ChatMessage` có `reactions = [{uid:"0",emoji:"😆",ts:0},{uid:"222",emoji:"❤️",ts:9}]` qua `persist_threads`, nạp lại bằng `hydrate_threads` → bằng nhau. Thêm một dòng `raw_json` dạng cũ `{"reactions":["❤️"]}` → nạp ra `[{uid:"", emoji:"❤️", ts:0}]`.
  - `chat.rs`: `reaction_code_for_removal`, assert `reaction_code_and_type("") == ("", -1)`.

- [ ] **Step 2: Chạy, thấy thất bại** — `cargo test --locked --lib -- reactions_roundtrip_through_raw_json reaction_code_for_removal`. Expected: lỗi biên dịch hoặc FAIL.

- [ ] **Step 3: Đổi kiểu và sửa từng chỗ gọi**
  - `extract_reactions` trả về `Vec<MsgReaction>`, các mục ở dạng cũ (`uid ""`).
  - Luật gộp ở `chat.rs:4480` đổi thành "chỉ lấy khi `existing.reactions` rỗng", giống `:232` và `store.rs:534`, để reaction từ lịch sử không đè lên dữ liệu theo người.
  - `store.rs` đọc bằng `serde_json::from_value::<Vec<MsgReaction>>`; ghi bằng `json!(m.reactions)`.
  - `reaction_code_and_type`: thêm nhánh `"" => ("", -1)` trước nhánh mặc định.
  - `workspace.rs` `React(r)`: `mine = summary(&real.reactions).mine`. Nếu `mine.as_deref() == Some(r)` thì `return`. Ngược lại `set_mine(&mut real.reactions, r)`, `status_msg = format!("Đã thả cảm xúc {r}")`, `persist_threads_async`, gửi `WorkerCmd::React` như cũ.
  - `gui.rs` `Reacted`: không sửa `reactions` nữa, chỉ đặt `status_msg` là "Đã gỡ cảm xúc" nếu `reaction.is_empty()`, ngược lại "Đã thả cảm xúc".
  - `paint_reaction_chip`:
    - icon lấy từ `summary.emojis` (tối đa 3, qua `emoji_to_tex_key`);
    - số đếm là `summary.total` (chỉ hiện khi > 1, như cũ);
    - sau khi vẽ, gọi `ui.interact(chip, Id::new(("react_chip", &msg.id, msg.ts)), Sense::hover()).on_hover_text(summary_tooltip(&s))`.
  - `tui.rs`: chỗ thả cảm xúc gọi `set_mine(&mut m.reactions, chat::icon_to_emoji(code))`; chỗ hiển thị lặp `summary(&m.reactions).emojis`; literal test dùng `vec![MsgReaction { uid: "0".into(), emoji: "👍".into(), ts: 0 }]`.
  - `demo.rs`: `g1[1].reactions` = một ❤️ của `uid "lan"` + một ❤️ của `uid "0"`.

- [ ] **Step 4: Chạy toàn bộ test** — `cargo test --locked`. Expected: PASS hết (191 test cũ + test mới).

- [ ] **Step 5: Commit** — `refactor(reaction): ChatMessage stores reactions per person`

---

### Task 3: Luồng nhận — socket `612/610/611`, `lp_control`, áp dụng trong GUI

**Files:**
- Modify: `src/reaction.rs` (thêm `apply_to_threads` + test)
- Modify: `src/socket.rs` (hằng mới cạnh `CMD_GET_MSG` `:62`)
- Modify: `src/call.rs` (`poll_voip_controls` `:953`)
- Modify: `src/gui.rs` (enum `GuiEvent` `:105`, xử lý sự kiện cạnh `Reacted` `:1199`)
- Modify: `src/gui/worker.rs` (vòng đọc WS `:552-603`, vòng `lp_control` `:625-646`)

**Interfaces:**
- Consumes (Task 1): `ReactionEvent`, `extract_events`, `apply`.
- Produces:
  - `pub fn apply_to_threads(threads: &mut [crate::chat::Thread], events: &[ReactionEvent]) -> bool` (reaction.rs);
  - `pub const CMD_PULL_REACT_1_1: u16 = 610; pub const CMD_PULL_REACT_GROUP: u16 = 611; pub const CMD_PUSH_REACTION: u16 = 612;` (socket.rs);
  - `pub struct CtrlPoll { pub voip: Vec<Value>, pub data: Value }` và `pub async fn poll_controls(s: &Session, state: &mut CtrlPollState) -> Result<CtrlPoll>` (call.rs);
  - `GuiEvent::ReactionsReceived(Vec<crate::reaction::ReactionEvent>)`.

- [ ] **Step 1: Viết test thất bại trong `reaction.rs`**
  - `apply_to_threads_updates_matching_message`: thread `"222"` có tin `id "7001"`; sự kiện ❤️ → `true`, tin có 1 mục.
  - `apply_to_threads_ignores_unknown_thread_or_message`: thread `"999"`, hoặc `msg_id "1"` → `false`, không panic, dữ liệu không đổi.

- [ ] **Step 2: Chạy, thấy thất bại** — `cargo test --locked --lib reaction::`

- [ ] **Step 3: Viết `apply_to_threads`.** Với mỗi sự kiện: tìm `t.id == ev.thread_id`, rồi `m.id == ev.msg_id`, rồi gọi `apply`. Trả `true` nếu có ít nhất một thay đổi.

- [ ] **Step 4: Chạy, thấy qua** — `cargo test --locked --lib reaction::`

- [ ] **Step 5: Nối dây**
  - `call.rs`:
    - Tách thân `poll_voip_controls` thành `poll_controls`. `data` = `data_node.clone()`, kể cả ở lần kéo đầu (`is_initial_cursor`); ở lần đó `voip` vẫn rỗng như cũ.
    - `poll_voip_controls` thành `poll_controls(..).await.map(|p| p.voip)`. Giữ nguyên `call_probe` và các chỗ gọi trong `call.rs`.
  - `worker.rs`, vòng WS: sau khối VoIP, nếu `matches!(ev.cmd, CMD_PUSH_REACTION | CMD_PULL_REACT_1_1 | CMD_PULL_REACT_GROUP)` thì gọi `extract_events(&ev.payload, &li.uin, Some(li.send2me_id.as_str()).filter(|s| !s.is_empty()))` với `li = s_snap.login`. Có sự kiện thì `gui_tx_ws.send(GuiEvent::ReactionsReceived(evs))` và `ctx_ws.request_repaint()`.
  - `worker.rs`, vòng `lp_control`: clone thêm `gui_tx` và `egui_ctx` vào task. Gọi `poll_controls`. `voip` xử lý như cũ; `data` qua `extract_events` như trên.
  - `gui.rs`: `GuiEvent::ReactionsReceived(evs) => if crate::reaction::apply_to_threads(&mut self.threads, &evs) { self.persist_threads_async(); }`.

- [ ] **Step 6: Build và test** — `cargo build --locked --bin zalo-rust` rồi `cargo test --locked`. Expected: build sạch, test PASS.

- [ ] **Step 7: Commit** — `feat(reaction): receive reactions from socket 612/610/611 and lp_control`

---

### Task 4: Giao diện rê chuột — nút 👍, thanh 6 cảm xúc, nút gỡ

**Files:**
- Modify: `src/gui/messages.rs`:
  - enum `BubbleAction` `:2072`: thêm `Unreact`;
  - `render_chat_bubble` `:1485`, hai nhánh tin gửi/tin đến quanh `:1833` và `:1912`;
  - hàm mới cạnh `paint_reaction_chip`.
- Modify: `src/gui/workspace.rs` (xử lý `BubbleAction::Unreact` cạnh `React`)

**Interfaces:**
- Consumes: `reaction::{summary, can_react}` (Task 1); `chat::reaction_code_and_type("")` và luồng `WorkerCmd::React` (Task 2).
- Produces (riêng trong `messages.rs`):
  - `fn react_button_rect(bubble: Rect, outgoing: bool) -> Rect`
  - `fn react_bar_rect(button: Rect, outgoing: bool, items: usize, screen: Rect) -> Rect`
  - `fn hover_reaction_controls(ui: &mut egui::Ui, bubble: Rect, msg: &ChatMessage, outgoing: bool, pal: &ThemePalette, emoji_textures: &HashMap<String, TextureHandle>) -> Option<BubbleAction>`

Hình học đã chốt:
- `react_button_rect`: 24×24. Tin đến: `min = (bubble.right() + 6, bubble.bottom() - 24)`. Tin gửi: `min = (bubble.left() - 30, bubble.bottom() - 24)`.
- `react_bar_rect`:
  - mỗi ô 34 px, đệm 6 px, nên rộng `items * 34 + 12` và cao 46;
  - `bottom = button.top() - 6`;
  - tin đến: `left = button.left() - 8`; tin gửi: `right = button.right() + 8`;
  - dịch (không co) cho nằm gọn trong `screen`.

Trạng thái: một `ReactBarState { key: Id, open: bool, hover_since: f64, last_inside: f64 }`, lưu bằng `ctx.data_mut(|d| d.insert_temp(Id::new("qtzalo_react_bar"), ..))`, `key = Id::new(("react", &msg.id, msg.ts))`.
- Nút hiện khi:
  - `can_react(msg)`;
  - không có popup/menu nào mở (`ui.memory(|m| m.any_popup_open())`);
  - con trỏ nằm trong `bubble ∪ button` (nới 2 px) **và** trong `ui.clip_rect()`;
  - **hoặc** thanh của tin này đang mở, con trỏ ở trên thanh hoặc mới rời dưới 200 ms.
- Rê trên nút liên tục 250 ms thì mở thanh. Rời cả nút lẫn thanh quá 200 ms thì đóng. Còn bộ đếm giờ đang chạy thì `ctx.request_repaint_after(50 ms)`.
- Bấm nút → `React("👍")`, trừ khi `mine == Some("👍")`.
- Thanh:
  - `egui::Area::new(key.with("bar")).order(egui::Order::Foreground).fixed_pos(bar.min)`;
  - nền `pal.bg_elevated`, viền `pal.border`, bo tròn `23.0`, bóng như chip;
  - 6 ô theo `REACTION_LIST` (ảnh 28 px, 34 px khi rê qua, `on_hover_text(tooltip)`), bấm → `React(code)`;
  - nếu `mine.is_some()` thêm ô `lucide::X`, tooltip "Gỡ cảm xúc", bấm → `Unreact`;
  - bấm xong thì đặt `open = false`.
- Icon nút: emoji của mình (`emoji_to_tex_key`, 16 px). Chưa thả thì vẽ texture `like` với `Color32::from_white_alpha(115)`.
- Demo: nếu `is_demo_mode()` và `QTZALO_DEMO_REACT_BAR=1` và `msg.id == "d3"` thì luôn hiện nút và thanh mở.

- [ ] **Step 1: Viết test thất bại (`mod react_geometry_tests` trong `messages.rs`)**
  - `button_sits_outside_bubble_bottom_aligned`: bubble `(100,100)-(300,160)`. Tin đến → `Rect::from_min_size((306,136), (24,24))`. Tin gửi → `min (70,136)`.
  - `bar_is_above_button_and_kept_on_screen`: nút ở `(10,300)`, tin gửi, 7 ô, `screen = (0,0)-(800,600)` → `bar.left() >= 0`, `bar.bottom() == 294`, `bar.width() == 250`.

- [ ] **Step 2: Chạy, thấy thất bại** — `cargo test --locked --lib react_geometry_tests`

- [ ] **Step 3: Viết `react_button_rect`, `react_bar_rect`, `hover_reaction_controls` theo các số đã chốt.** Gọi `hover_reaction_controls` ở cả hai nhánh của `render_chat_bubble`, ngay sau `paint_reaction_chip`, rồi gộp kết quả: `card_action.or(react_action).or_else(|| bubble_context_menu(..))`. Thứ tự này giữ nguyên nhấp đúp để trả lời và menu chuột phải.

- [ ] **Step 4: Viết xử lý `Unreact` trong `workspace.rs`.** Nếu `summary(..).mine.is_none()` thì `return`. Ngược lại `set_mine(.., "")`, `persist_threads_async`, gửi `WorkerCmd::React { reaction: String::new(), .. }`.

- [ ] **Step 5: Chạy test** — `cargo test --locked`. Expected: PASS.

- [ ] **Step 6: Chụp ảnh demo và xem.**
  - Chạy `zalo-rust.exe --gui --demo` với `QTZALO_DEMO_THREAD=0 QTZALO_DEMO_REACT_BAR=1 QTZALO_DEMO_SIZE=1280x800`, một lần thêm `QTZALO_DEMO_THEME=dark`.
  - Chụp cửa sổ bằng script PowerShell (`System.Drawing`, `CopyFromScreen` theo khung cửa sổ của tiến trình) trong scratchpad.
  - Mở ảnh xem. Expected: thanh 6 cảm xúc nằm trên nút, cạnh tin "d3"; chip của "d2" ghi `2`; hai theme đều rõ.

- [ ] **Step 7: Kiểm thủ công trong `--demo`** (Review Focus 4–5):
  - rê qua lại nhanh nhiều bong bóng → chỉ một thanh mở;
  - cuộn để bong bóng khuất dưới header → nút không lộ ra ngoài vùng chat;
  - mở menu chuột phải → nút ẩn, menu vẫn chạy;
  - nhấp đúp bong bóng → vẫn vào chế độ trả lời;
  - rê lên chip → tooltip "❤️ 2 — có bạn";
  - thả rồi gỡ → chip cập nhật.

- [ ] **Step 8: Commit** — `feat(gui): hover reaction button and picker on every message`

---

### Task 5: Kiểm chứng toàn nhánh, nghiệm thu thật, tài liệu

**Files:**
- Modify: `CHANGELOG.md` (mục mới `## [Unreleased]` trên `## [0.1.2]`, phần "Thêm mới" và "Sửa lỗi")
- Modify: `docs/HUONG_DAN_SU_DUNG.md:42` (thêm cách rê chuột thả, đổi, gỡ cảm xúc)
- Modify: `re-notes/10_bang_chung_dong_bo.md` (§18: kết quả nghiệm thu, nhãn [O])
- Modify: `docs/superpowers/specs/2026-11-02-reaction-hover-realtime-design.md` (chỉ khi nghiệm thu bác bỏ giả thiết gỡ cảm xúc: ghi lại và bỏ nút ✕)

- [ ] **Step 1: Kiểm tra tĩnh.** Chạy lần lượt `cargo test --locked`, `cargo build --locked --bin zalo-rust`, `cargo +1.88 check --locked --all-targets`, `cargo audit`. Expected: test PASS, build và check sạch, audit 0 lỗ hổng (2 cảnh báo hết bảo trì như 0.1.2).

- [ ] **Step 2: Nghiệm thu thật** (QTzalo chạy với `ZCALL_WS_TRACE=1`, trace lưu ở `%USERPROFILE%\qtzalo-trace\`; người dùng thao tác trên điện thoại):
  1. Thả, đổi rồi gỡ cảm xúc trên điện thoại lúc QTzalo đang mở → chip cập nhật trong vài giây; gỡ thì chip mất.
  2. Tắt QTzalo, thả cảm xúc trên điện thoại, mở lại → chip hiện.
  3. Rê chuột thả cảm xúc vào một tin của "Cloud của tôi" → điện thoại hiện đúng emoji. Bấm ✕ → điện thoại gỡ. Nếu không gỡ: bỏ ô ✕ trong `hover_reaction_controls` và `BubbleAction::Unreact`, chạy lại Step 1.
  - Expected: cả ba đạt; trace có `POST …/api/message/reaction` với `error_code 0`.

- [ ] **Step 3: Ghi tài liệu.** `re-notes/10` §18 (cấu trúc đổi/gỡ quan sát được, nhãn [O]/[I]); CHANGELOG; hướng dẫn sử dụng.

- [ ] **Step 4: Commit** — `docs(reaction): acceptance evidence, changelog, user guide`
