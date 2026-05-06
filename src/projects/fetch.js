// Per-project view — fetchers (v0.28 Block E).
//
// Each fetcher writes into `_state.project` and never throws — errors
// are surfaced via the `error` field so render can show a soft fallback
// instead of an exception. The aggregator command on the Rust side is
// already fault-tolerant; if Slack is off, only the Slack updates skip.

function bridge() {
  return window.__TAURI__?.core;
}

async function safeInvoke(cmd, args) {
  const t = bridge();
  if (!t) throw new Error("not in tauri");
  return t.invoke(cmd, args);
}

export async function loadProject(state, projectCode) {
  state.project_code = projectCode;
  state.loading = true;
  state.error = null;
  try {
    const feed = await safeInvoke("list_project_updates", { projectCode });
    state.header = feed.header || { code: projectCode };
    state.updates = Array.isArray(feed.updates) ? feed.updates : [];
  } catch (e) {
    console.warn("loadProject failed:", e);
    state.error = String(e);
    state.header = state.header || { code: projectCode };
    state.updates = [];
  } finally {
    state.loading = false;
  }
}

export async function refreshUpdates(state) {
  if (!state.project_code) return;
  try {
    const feed = await safeInvoke("list_project_updates", {
      projectCode: state.project_code,
    });
    state.header = feed.header || state.header;
    state.updates = Array.isArray(feed.updates) ? feed.updates : [];
  } catch (e) {
    console.warn("refreshUpdates failed:", e);
  }
}

export async function listActiveProjects(clientCode) {
  try {
    const list = await safeInvoke("list_active_projects_for_client", {
      clientCode,
    });
    return Array.isArray(list) ? list : [];
  } catch (e) {
    console.warn("list_active_projects_for_client failed:", e);
    return [];
  }
}

export async function createNote(projectCode, body, tags) {
  return safeInvoke("create_project_note", {
    payload: {
      project_code: projectCode,
      body,
      tags: tags || [],
    },
  });
}

export async function updateNote(noteRecordId, body, tags) {
  return safeInvoke("update_project_note", {
    payload: {
      note_record_id: noteRecordId,
      body,
      tags,
    },
  });
}

export async function deleteNote(noteRecordId) {
  return safeInvoke("delete_project_note", { noteRecordId });
}
