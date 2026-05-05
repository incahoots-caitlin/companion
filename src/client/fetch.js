// Per-client view — fetchers.
//
// Each fetcher writes into _state.client and never throws. Errors get
// logged + the section's slot stays an empty array so render shows the
// graceful empty state rather than crashing.
//
// All Airtable work goes through the Tauri bridge. The JS context can't
// reach external HTTPS directly under our CSP; the Airtable PAT lives in
// Keychain and only the Rust side reads it.
//
// Strategy: reuse the broad list commands the Today build added
// (list_airtable_workstreams / decisions / commitments / projects), then
// filter in JS by client record id. Datasets are small (under 100 records
// each in v0.19) so this stays cheap. Receipts are the only client-scoped
// fetch that pages — it uses the new list_airtable_receipts_for_client
// command added in Block B3.

import { emptyClientState } from "./state.js";

const STALE_MS = 5 * 60 * 1000; // 5 minutes — matches Today dashboard

function bridge() {
  return window.__TAURI__?.core;
}

async function safeInvoke(cmd, args) {
  const t = bridge();
  if (!t) throw new Error("not in tauri");
  return t.invoke(cmd, args);
}

function parseDate(s) {
  if (!s) return null;
  const d = new Date(s);
  return Number.isNaN(d.getTime()) ? null : d;
}

function recordLinksContain(fields, recordId) {
  const ids = fields?.client;
  if (!Array.isArray(ids)) return false;
  return ids.includes(recordId);
}

// ── Header / record id resolver ───────────────────────────────────────

export async function loadHeader(state, code) {
  // Resolve client by code from the active-clients list. Stores record id
  // and the fields the header reads. Re-used by every other fetcher to
  // filter cross-table records.
  try {
    const raw = await safeInvoke("list_airtable_clients");
    const data = JSON.parse(raw);
    const wanted = (code || "").toUpperCase();
    const match = (data.records || []).find(
      (r) => (r.fields?.code || "").toUpperCase() === wanted
    );
    if (!match) {
      state.recordId = null;
      state.header = { name: code, status: "unknown" };
      return;
    }
    state.recordId = match.id;
    state.header = {
      name: match.fields?.name || code,
      status: match.fields?.status || "active",
      primary_contact_name: match.fields?.primary_contact_name || null,
      primary_contact_email: match.fields?.primary_contact_email || null,
      abn: match.fields?.abn || null,
      dropbox_folder: match.fields?.dropbox_folder || null,
      gmail_thread_filter: match.fields?.gmail_thread_filter || null,
      notes: match.fields?.notes || null,
      // last_touch is filled in by loadReceipts once receipts arrive.
      last_touch: null,
    };
    state.last_fetch_at.header = Date.now();
  } catch (e) {
    console.warn("loadHeader failed:", e);
    state.recordId = null;
    state.header = { name: code, status: "unknown" };
  }
}

// ── Workstreams ───────────────────────────────────────────────────────

export async function loadWorkstreams(state) {
  try {
    if (!state.recordId) {
      state.workstreams = [];
      return;
    }
    const raw = await safeInvoke("list_airtable_workstreams");
    const data = JSON.parse(raw);
    const records = (data.records || [])
      .filter((r) => recordLinksContain(r.fields, state.recordId))
      .map((r) => ({ id: r.id, ...r.fields }));
    // Sort: blocked first, then by last_touch_at desc inside status bucket.
    records.sort((a, b) => {
      const ar = a.status === "blocked" ? 0 : 1;
      const br = b.status === "blocked" ? 0 : 1;
      if (ar !== br) return ar - br;
      const at = parseDate(a.last_touch_at)?.getTime() || 0;
      const bt = parseDate(b.last_touch_at)?.getTime() || 0;
      return bt - at;
    });
    state.workstreams = records;
    state.last_fetch_at.workstreams = Date.now();
  } catch (e) {
    console.warn("loadWorkstreams failed:", e);
    state.workstreams = [];
  }
}

// ── Decisions ─────────────────────────────────────────────────────────

export async function loadDecisions(state) {
  try {
    if (!state.recordId) {
      state.decisions = [];
      return;
    }
    const raw = await safeInvoke("list_airtable_decisions");
    const data = JSON.parse(raw);
    const records = (data.records || [])
      .filter((r) => recordLinksContain(r.fields, state.recordId))
      .map((r) => ({ id: r.id, ...r.fields }));
    // Sort by due_date asc, undated last.
    records.sort((a, b) => {
      const ad = a.due_date || "9999-12-31";
      const bd = b.due_date || "9999-12-31";
      return ad.localeCompare(bd);
    });
    state.decisions = records;
    state.last_fetch_at.decisions = Date.now();
  } catch (e) {
    console.warn("loadDecisions failed:", e);
    state.decisions = [];
  }
}

// ── Commitments ───────────────────────────────────────────────────────

export async function loadCommitments(state) {
  try {
    if (!state.recordId) {
      state.commitments = [];
      return;
    }
    const raw = await safeInvoke("list_airtable_commitments");
    const data = JSON.parse(raw);
    const records = (data.records || [])
      .filter((r) => recordLinksContain(r.fields, state.recordId))
      .map((r) => ({ id: r.id, ...r.fields }));
    // Sort by due_at asc, undated last.
    records.sort((a, b) => {
      const at = parseDate(a.due_at)?.getTime();
      const bt = parseDate(b.due_at)?.getTime();
      if (at == null && bt == null) return 0;
      if (at == null) return 1;
      if (bt == null) return -1;
      return at - bt;
    });
    state.commitments = records;
    state.last_fetch_at.commitments = Date.now();
  } catch (e) {
    console.warn("loadCommitments failed:", e);
    state.commitments = [];
  }
}

// ── Projects ──────────────────────────────────────────────────────────

export async function loadProjects(state) {
  try {
    if (!state.recordId) {
      state.projects = [];
      return;
    }
    const raw = await safeInvoke("list_airtable_projects");
    const data = JSON.parse(raw);
    const records = (data.records || [])
      .filter((r) => recordLinksContain(r.fields, state.recordId))
      .map((r) => ({ id: r.id, ...r.fields }));
    // Sort by start_date desc, undated last.
    records.sort((a, b) => {
      const ad = a.start_date || "";
      const bd = b.start_date || "";
      if (!ad && !bd) return 0;
      if (!ad) return 1;
      if (!bd) return -1;
      return bd.localeCompare(ad);
    });
    state.projects = records;
    state.last_fetch_at.projects = Date.now();
  } catch (e) {
    console.warn("loadProjects failed:", e);
    state.projects = [];
  }
}

// ── Receipts ──────────────────────────────────────────────────────────

export async function loadReceipts(state) {
  try {
    if (!state.recordId) {
      state.receipts = [];
      return;
    }
    const raw = await safeInvoke("list_airtable_receipts_for_client", {
      recordId: state.recordId,
      limit: 10,
    });
    const data = JSON.parse(raw);
    const records = (data.records || []).map((r) => {
      const f = r.fields || {};
      let total = 0;
      try {
        const payload = JSON.parse(f.json || "{}");
        (payload.sections || []).forEach((s) => {
          (s.items || []).forEach((it) => {
            if (it.type === "task") total += 1;
          });
        });
      } catch {
        // ignore — total stays 0, render shows "x of 0"
      }
      return {
        airtable_id: r.id,
        id: f.id,
        title: f.title || "Receipt",
        date: f.date || "",
        workflow: f.workflow || "",
        ticked: Number(f.ticked_count || 0),
        total,
        json: f.json || null,
      };
    });
    state.receipts = records;
    state.last_fetch_at.receipts = Date.now();

    // Roll the most recent receipt date up into the header as last_touch.
    if (state.header && records.length) {
      state.header.last_touch = records[0].date || null;
    }
  } catch (e) {
    console.warn("loadReceipts failed:", e);
    state.receipts = [];
  }
}

// ── Meetings (v0.24) ──────────────────────────────────────────────────
//
// Calls the Rust-side calendar matcher. Skips silently when Google
// isn't connected — the section hides cleanly via render's empty check.

export async function loadMeetings(state) {
  try {
    if (!state.code || !state.header) {
      state.meetings = [];
      return;
    }
    // Quick status check first. If not connected we skip the network
    // call entirely.
    let status = null;
    try { status = await safeInvoke("get_google_status"); } catch {}
    if (!status?.connected) {
      state.meetings = [];
      return;
    }
    const aliases = state.header.gmail_thread_filter
      ? [state.header.gmail_thread_filter]
      : [];
    const raw = await safeInvoke("list_calendar_for_client", {
      input: {
        client_code: state.code,
        client_name: state.header.name || null,
        aliases,
      },
    });
    const events = JSON.parse(raw || "[]");
    state.meetings = Array.isArray(events) ? events : [];
    state.last_fetch_at.meetings = Date.now();
  } catch (e) {
    console.warn("loadMeetings failed:", e);
    state.meetings = [];
  }
}

// ── Top-level loaders ─────────────────────────────────────────────────

export async function loadAll(state, code) {
  if (!state) state = emptyClientState();
  state.code = (code || "").toUpperCase();
  if (!bridge()) return state;
  // Header first so we have the record id; the others fan out concurrently.
  await loadHeader(state, state.code);
  await Promise.allSettled([
    loadWorkstreams(state),
    loadDecisions(state),
    loadCommitments(state),
    loadProjects(state),
    loadReceipts(state),
    loadMeetings(state),
  ]);
  return state;
}

export async function refreshStale(state) {
  if (!bridge() || !state.recordId) return;
  const now = Date.now();
  const tasks = [];
  if ((state.last_fetch_at.workstreams || 0) + STALE_MS < now) {
    tasks.push(loadWorkstreams(state));
  }
  if ((state.last_fetch_at.decisions || 0) + STALE_MS < now) {
    tasks.push(loadDecisions(state));
  }
  if ((state.last_fetch_at.commitments || 0) + STALE_MS < now) {
    tasks.push(loadCommitments(state));
  }
  if ((state.last_fetch_at.projects || 0) + STALE_MS < now) {
    tasks.push(loadProjects(state));
  }
  if ((state.last_fetch_at.receipts || 0) + STALE_MS < now) {
    tasks.push(loadReceipts(state));
  }
  if ((state.last_fetch_at.meetings || 0) + STALE_MS < now) {
    tasks.push(loadMeetings(state));
  }
  await Promise.allSettled(tasks);
}

// Refetch the slices most likely to change after a workflow runs against
// this client. Used by main.js when a workflow modal closes successfully.
export async function refreshAfterWorkflow(state) {
  if (!bridge() || !state.recordId) return;
  await Promise.allSettled([loadReceipts(state), loadWorkstreams(state)]);
}

export const TIMINGS = { STALE_MS };
