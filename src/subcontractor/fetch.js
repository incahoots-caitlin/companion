// Per-Subcontractor view fetch layer (v0.38).
//
// One Tauri call: load_subcontractor_view(code) returns the header,
// workstreams, commitments, timelogs and recent receipts as a single
// JSON blob. We hydrate the state object in place — render.js reads it.

function bridge() {
  return window.__TAURI__?.core;
}

async function safeInvoke(cmd, args) {
  const t = bridge();
  if (!t) throw new Error("not in tauri");
  return t.invoke(cmd, args);
}

export async function loadAll(state, code) {
  state.error = null;
  state.loading = true;
  try {
    const data = await safeInvoke("load_subcontractor_view", { code });
    state.record_id = data.record_id || "";
    state.header = data.header || null;
    state.workstreams = Array.isArray(data.workstreams)
      ? data.workstreams
      : [];
    state.commitments = Array.isArray(data.commitments)
      ? data.commitments
      : [];
    state.timelogs = Array.isArray(data.timelogs) ? data.timelogs : [];
    state.receipts = Array.isArray(data.receipts) ? data.receipts : [];
    state.month_start = data.month_start || "";
  } catch (e) {
    state.error = String(e?.message || e);
  } finally {
    state.loading = false;
  }
  return state;
}
