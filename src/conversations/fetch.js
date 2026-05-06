// Conversations chat surface — Tauri bridge (v0.27 Block D).
//
// Three commands cover the full surface:
//   - load_conversation(workstream_code) -> ConversationPayload
//   - send_message(workstream_code, user_message) -> ConversationPayload
//   - archive_conversation(workstream_code) -> ()
//
// Each fetcher writes the returned payload into _state.conversation.
// Errors bubble up — render layer surfaces them as toasts.

function bridge() {
  return window.__TAURI__?.core;
}

async function safeInvoke(cmd, args) {
  const t = bridge();
  if (!t) throw new Error("not in tauri");
  return t.invoke(cmd, args);
}

function applyPayload(state, payload) {
  state.workstream_code = payload.workstream_code || state.workstream_code;
  state.workstream_record_id = payload.workstream_record_id || state.workstream_record_id;
  state.workstream_title = payload.workstream_title || state.workstream_title;
  state.conv_id = payload.conv_id || null;
  state.record_id = payload.record_id || null;
  state.status = payload.status || "active";
  state.started_at = payload.started_at || null;
  state.last_message_at = payload.last_message_at || null;
  state.messages = Array.isArray(payload.messages) ? payload.messages : [];
  state.loaded_at = Date.now();
}

export async function loadConversation(state, workstreamCode) {
  state.workstream_code = workstreamCode;
  state.messages = [];
  state.status = "new";
  const payload = await safeInvoke("load_conversation", {
    workstreamCode,
  });
  applyPayload(state, payload);
  return payload;
}

export async function sendMessage(state, userMessage) {
  if (!state.workstream_code) throw new Error("No workstream selected");
  state.sending = true;
  try {
    const payload = await safeInvoke("send_message", {
      workstreamCode: state.workstream_code,
      userMessage,
    });
    applyPayload(state, payload);
    return payload;
  } finally {
    state.sending = false;
  }
}

export async function archiveConversation(workstreamCode) {
  return safeInvoke("archive_conversation", {
    workstreamCode,
  });
}
