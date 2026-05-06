// Conversations chat surface — state shape (v0.27 Block D).
//
// One conversation belongs to one workstream. The chat surface lives at
// `_state.conversation` on the global state container and is overwritten
// when a different workstream is opened. No multi-conversation cache — if
// Caitlin needs to flick between threads she clicks the workstream rail.
//
// Shape:
//
// _state.conversation = {
//   workstream_code: 'ws-context-launch',
//   workstream_record_id: 'recXXX',
//   workstream_title: 'Context launch',
//   conv_id: 'conv_2026-05-05_18-30-00' | null,
//   record_id: 'recYYY' | null,
//   status: 'active' | 'archived' | 'new',
//   started_at: ISO | null,
//   last_message_at: ISO | null,
//   messages: [ { role, content, ts } ],
//   sending: false,           // true while Anthropic call is in flight
//   loaded_at: ms timestamp,
// }

export function emptyConversationState() {
  return {
    workstream_code: null,
    workstream_record_id: null,
    workstream_title: null,
    conv_id: null,
    record_id: null,
    status: "new",
    started_at: null,
    last_message_at: null,
    messages: [],
    sending: false,
    loaded_at: 0,
  };
}
