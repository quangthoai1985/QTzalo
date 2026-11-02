# Thả cảm xúc khi rê chuột và nhận reaction theo thời gian thực — thiết kế (v0.1.3)

> Ngày 2026-11-02, nhánh `claude/v0.1.3`. Người dùng đã duyệt thiết kế trong phiên chat ngày 2026-11-02.
> Bằng chứng giao thức: `re-notes/10_bang_chung_dong_bo.md` §17 (trace thật, tối 02/11). Nhãn **[O]** = đã quan sát, **[I]** = suy luận, **[U]** = chưa biết.

## 1. Mục tiêu

Người dùng nêu: "thiếu chức năng thả biểu tượng cảm xúc khi rê chuột vào mỗi tin nhắn". Người dùng chọn phạm vi "rê chuột + nhận realtime".

Thành công khi:

1. Rê chuột vào một tin thì có nút thả cảm xúc. Thả, đổi hay gỡ cảm xúc chỉ cần rê chuột và bấm, không phải mở menu chuột phải.
2. Reaction của người khác, và của chính người dùng thả từ điện thoại, hiện trong QTzalo **ngay lúc đang mở**, và **sau khi mở lại app** nếu được thả lúc app tắt.
3. Thả cảm xúc không còn xoá reaction của người khác. Chip hiện đúng số người đã thả.

Hiện trạng (`1dd4ee6`):
- Đã có `chat::send_reaction` và 6 cảm xúc trong menu chuột phải (`messages.rs::bubble_context_menu`).
- Đã có chip `❤ 2` (`messages.rs::paint_reaction_chip`).
- `ChatMessage.reactions` là `Vec<String>` gồm các emoji không trùng, không biết ai thả. Thả cảm xúc thì `reactions = vec![của mình]` (`workspace.rs`, `gui.rs` `GuiEvent::Reacted`).
- Worker bỏ qua frame `612/610/611` (`socket::WsEvent::is_chat_ish` không gồm chúng) và bỏ qua reaction trong `lp_control`.

## 2. Bằng chứng giao thức đã dùng

Theo `re-notes/10` §17:

- **[O]** Reaction lúc đang mở đến bằng socket `612_0`. Reaction lúc tắt được trả lại khi kéo queue `610_1` (1-1); `611_1` (nhóm) có cùng khung nhưng chưa có dữ liệu. `lp_control` (HTTP) cũng trả reaction, kể cả toàn bộ từ lúc đăng nhập ở lần kéo đầu (`lastActionId = 1`).
- **[O]** `data.reacts[]` (1-1) và `data.reactGroups[]` (nhóm). Mỗi phần tử có `uidFrom` (`"0"` = chính chủ tài khoản), `idTo`, `ts`, `msgType = "chat.reaction"`, `content` là chuỗi JSON `{rIcon, rType, source, msgSender, rMsg: [{gMsgID: số, cMsgID: số, msgType}]}`.
- **[O]** `612` mang `queueStatus` của `610_0/610_1`. `sync_state::merge_ws_queue_status` đã tự cập nhật con trỏ từ mọi payload, nên lần mở sau không tải lại.
- **[I]** (zca-js) Gỡ cảm xúc là `rIcon = ""`, `rType = -1`. Phải kiểm chứng khi nghiệm thu (§7).
- **[U]** Đổi cảm xúc tạo một sự kiện mới hay sửa sự kiện cũ. Thiết kế chịu được cả hai cách, vì lấy sự kiện mới nhất của mỗi người.

## 3. Dữ liệu

Module mới `src/reaction.rs`: thuần, không I/O, có unit test. `chat.rs` đã khoảng 5,7k dòng nên không nhét thêm vào đó.

```rust
/// One person's current reaction on a message. `emoji` is empty when they removed it
/// (kept as a tombstone so an older replayed event cannot bring it back).
pub struct MsgReaction { pub uid: String, pub emoji: String, pub ts: i64 }

/// One reaction change read from `612` / `610` / `611` / `lp_control`.
pub struct ReactionEvent {
    pub thread_id: String, pub is_group: bool,
    pub msg_id: String,   // gMsgID of the target message, as a string
    pub uid: String,      // "0" = this account (any device)
    pub emoji: String,    // "" = removed
    pub ts: i64,          // server ts of the event
}
```

- `ChatMessage.reactions: Vec<MsgReaction>`, thay cho `Vec<String>`.
  - Serde đọc được cả **dạng cũ**: phần tử là chuỗi emoji thì thành `MsgReaction { uid: "", emoji, ts: 0 }`. Dùng `#[serde(untagged)]` ở mức phần tử, áp dụng cho `threads_cache.json` và cột `raw_json` trong SQLite (`store.rs`).
  - Ghi ra luôn dạng mới.
- `uid = "0"` là chính chủ tài khoản. `uidFrom` bằng `"0"` hoặc bằng `login.uin` đều chuẩn hoá thành `"0"`.
- `uid = ""` là dữ liệu cũ không rõ người thả. Giữ nguyên để hiển thị; sự kiện mới không ghi đè lên mục này.

Các hàm thuần trong `reaction.rs`:

| Hàm | Việc |
|---|---|
| `extract_events(payload, self_uid, cloud_id) -> Vec<ReactionEvent>` | Đọc `data.reacts` (1-1) và `data.reactGroups` (nhóm), chấp nhận `data` là object hoặc chuỗi JSON, `content` là chuỗi JSON hoặc object, `gMsgID` là số hoặc chuỗi. Mỗi mục `rMsg` thành một sự kiện. Bỏ qua phần tử thiếu `gMsgID` hoặc `ts`. |
| hội thoại của sự kiện | Nhóm: `idTo` (bỏ tiền tố `group_`). 1-1: nếu `uidFrom ∈ {"0", self}` thì là `idTo`, và nếu `idTo ∈ {"", "0", self}` thì là "Cloud của tôi" (`cloud_id`); ngược lại là `uidFrom`. |
| emoji | `rIcon` rỗng thì `""` (gỡ). Còn lại dùng `chat::icon_to_emoji`; mã lạ ra ❤️ như hiện nay. |
| `apply(reactions, &event) -> bool` | Tìm mục cùng `uid`. Nếu `event.ts` nhỏ hơn `ts` đã có thì bỏ qua. Ngược lại thì ghi `emoji`/`ts`, hoặc thêm mục mới. Trả `true` nếu có thay đổi. Mục cục bộ do QTzalo vừa thả có `ts = 0`, nên sự kiện của máy chủ luôn thắng. |
| `set_mine(reactions, emoji)` | Ghi thẳng mục `uid "0"` thành `{emoji, ts: 0}` (thả, đổi, gỡ từ QTzalo), không so `ts`. Sự kiện máy chủ gửi về sau, có `ts` thật, sẽ chốt lại. |
| `summary(reactions) -> ReactionSummary` | Chỉ tính mục có emoji khác rỗng. Trả về: emoji theo số người giảm dần (chip lấy 3 cái đầu), tổng số người, emoji của mình (`uid "0"`). |
| `can_react(msg) -> bool` | `false` với tin chưa có id máy chủ (`id` rỗng, `out_*`, `local_*`), giống `send_reaction` hiện nay. |

## 4. Luồng nhận

1. **Socket** (`gui/worker.rs`, vòng đọc `WsEvent`): với `ev.cmd ∈ {612, 610, 611}`, gọi `reaction::extract_events(&ev.payload, uin, send2me_id)`. Nếu có sự kiện thì gửi `GuiEvent::ReactionsReceived(Vec<ReactionEvent>)` và `request_repaint`. Thêm hằng `CMD_PUSH_REACTION = 612`, `CMD_PULL_REACT_1_1 = 610`, `CMD_PULL_REACT_GROUP = 611` vào `socket.rs`.
2. **`lp_control`** (`call.rs`):
   - Thêm `poll_controls(s, state) -> Result<CtrlPoll { voip: Vec<Value>, data: Value }>`.
   - `poll_voip_controls` thành hàm bọc trả `.voip`, giữ nguyên hành vi cho `call_probe` và các chỗ gọi trong `call.rs`. Bản phát lại đầu tiên vẫn bỏ VoIP như cũ, nhưng `data` vẫn được trả về.
   - Worker gọi `poll_controls` rồi lấy reaction từ `data`, kể cả bản phát lại đầu tiên. Nhờ vậy, ngay lần đầu chạy bản mới, các reaction thả từ lúc đăng nhập cũng hiện.
3. **GUI** (`gui.rs`, `GuiEvent::ReactionsReceived`):
   - Với mỗi sự kiện, tìm hội thoại theo `thread_id`, rồi tìm tin theo `id == msg_id` trong các tin đang có trong bộ nhớ (200 tin gần nhất mỗi hội thoại).
   - Gọi `reaction::apply`. Nếu có thay đổi thì gọi `persist_threads_async()` (một lần cho cả đợt).
   - `store::upsert_message` đã cập nhật `raw_json` khi trùng `msg_id`, nên reaction được lưu bền.
   - Không thấy hội thoại hoặc tin thì bỏ qua (§8).
4. **Trùng lặp** (socket, `lp_control` và phát lại sau khi mở lại app đều có thể trả cùng một sự kiện): vô hại, vì `apply` giữ mục mới nhất của mỗi người.

## 5. Luồng gửi

- `BubbleAction::React(emoji)` (từ thanh rê chuột, nút 👍 và menu chuột phải):
  - Nếu emoji trùng emoji hiện tại của mình thì không làm gì.
  - Ngược lại: cập nhật cục bộ `MsgReaction { uid: "0", emoji, ts: 0 }`, `persist_threads_async`, rồi gửi `WorkerCmd::React` như hiện nay.
  - Thay cho kiểu bật/tắt cũ, vốn xoá cục bộ nhưng lại gửi thả lên máy chủ.
- `BubbleAction::Unreact` (nút ✕): cập nhật cục bộ `emoji = ""`, rồi gửi `WorkerCmd::React { reaction: "" }`. `chat::reaction_code_and_type("")` trả `("", -1)` **[I]**.
- `GuiEvent::Reacted` không ghi đè `reactions` nữa, chỉ cập nhật dòng trạng thái ("Đã thả cảm xúc" / "Đã gỡ cảm xúc").
- Gửi lỗi: giữ hành vi hiện nay (dòng trạng thái báo lỗi, không hoàn tác cục bộ).
- Ở chế độ `--demo`: chỉ cập nhật cục bộ. Không có worker nên không gửi gì.

## 6. Giao diện (`gui/messages.rs`)

Phong cách theo theme hiện có (`ThemePalette`), dùng ảnh emoji sẵn có (`assets/emojis`, `REACTION_LIST`).

1. **Nút cảm xúc khi rê chuột:**
   - Hiện khi con trỏ nằm trong vùng bong bóng hoặc vùng nút, trong vùng hiển thị của khung chat (`clip_rect`), không có popup hay menu nào đang mở, và `can_react(msg)` đúng.
   - Nút tròn 24 px, nằm **ngoài** bong bóng, đáy thẳng với đáy bong bóng: bên phải với tin đến, bên trái với tin gửi. Không đè lên chip hay giờ gửi.
   - Nền `bg_elevated`, viền `border`, có bóng nhẹ. Icon là emoji hiện tại của mình; nếu chưa thả thì là 👍 mờ (alpha khoảng 0.45).
   - Bấm nút là `React("👍")`.
2. **Thanh 6 cảm xúc:**
   - Mở khi rê lên nút khoảng 250 ms. Là `egui::Area` (`Order::Foreground`) dạng viên thuốc, nằm phía trên nút: tin đến canh trái theo nút, tin gửi canh phải. Kẹp trong màn hình.
   - Mỗi cảm xúc 28 px, phóng lên 34 px khi rê qua, có tooltip tên ("Thích", "Yêu thích"…). Nếu mình đã thả thì thêm nút ✕ "Gỡ cảm xúc".
   - Thanh giữ mở khi con trỏ nằm trên nút hoặc trên thanh. Rời cả hai quá 200 ms thì đóng. Mỗi lúc chỉ mở một thanh, trạng thái lưu trong `ctx.memory` (temp data).
   - Bấm một cảm xúc là `React(emoji)`; bấm ✕ là `Unreact`. Sau khi bấm thì đóng thanh.
3. **Chip:** đếm theo `summary` (số người, tối đa 3 emoji). Rê lên chip thì hiện tooltip dạng "❤️ 2 · 😆 1", thêm "— có bạn" nếu mình đã thả.
4. **Menu chuột phải:** giữ nguyên 6 cảm xúc, dùng chung `React` ở §5.
5. **Demo:** biến `QTZALO_DEMO_REACT_BAR=1` mở sẵn thanh cảm xúc trên một tin mẫu, để chụp ảnh sáng và tối. Dữ liệu mẫu `demo.rs` chuyển sang `MsgReaction` (một ❤️ của người khác, một ❤️ của mình).

TUI (`tui.rs`) chỉ đổi cho khớp kiểu mới (thả = `apply` một mục `uid "0"`), không thêm tính năng.

## 7. Kiểm chứng

- **Unit test** (`reaction.rs`, `store.rs`):
  - tách sự kiện từ payload dựng theo cấu trúc §2: `reacts`/`reactGroups`; `content` là chuỗi; `gMsgID` là số; nhiều `rMsg`; `data` là chuỗi JSON;
  - chuẩn hoá `"0"`/`self`; ánh xạ "Cloud của tôi";
  - gỡ cảm xúc tạo mục rỗng (tombstone); sự kiện cũ hơn bị bỏ qua; mục cục bộ `ts = 0` thua sự kiện máy chủ;
  - `summary` đếm và sắp xếp đúng;
  - đọc dạng cũ (chuỗi) và ghi/đọc lại dạng mới qua serde và `raw_json`.
- **Build và tĩnh:**
  - `cargo test --locked` xanh;
  - build MSVC trên PC;
  - `cargo +1.88 check --locked --all-targets`;
  - `cargo audit` (không thêm crate mới).
- **Ảnh `--demo`:** thanh cảm xúc ở giao diện sáng và tối, chip có tooltip. Xem ảnh trước khi kết luận.
- **Nghiệm thu thật (người dùng thao tác trên điện thoại, QTzalo chạy với trace):**
  1. Thả, đổi rồi gỡ cảm xúc trên điện thoại khi QTzalo đang mở: chip cập nhật trong vài giây, đúng emoji. Gỡ thì chip mất.
  2. Tắt QTzalo, thả cảm xúc trên điện thoại, mở lại: chip hiện.
  3. Trong QTzalo, rê chuột thả cảm xúc vào một tin của "Cloud của tôi": điện thoại hiện đúng emoji. Sau đó bấm ✕: điện thoại gỡ. Nếu điện thoại không gỡ thì bỏ nút ✕ và ghi lại **[O]**.
  4. Ghi kết quả vào `re-notes/10` (§18).

## 8. Ngoài phạm vi và giới hạn chấp nhận

- Đếm theo số người, không theo số lần bấm (Zalo có thể cộng dồn khi một người bấm nhiều lần).
- Danh sách tên người đã thả; các nút trả lời/chuyển tiếp khi rê chuột.
- Reaction cho tin nằm ngoài 200 tin gần nhất trong bộ nhớ của mỗi hội thoại thì bị bỏ qua.
- Hoàn tác cục bộ khi gửi lỗi.
- Bộ đếm `clearUnreadsReact` của `get-last-msgs` (reaction chưa xem).
