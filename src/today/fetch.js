// Today dashboard — fetchers.
//
// Each fetcher writes into _state.today and never throws. Errors get
// logged + the section's slot stays null (or a sentinel) so render shows
// a graceful "(unavailable)" rather than crashing.
//
// All Airtable / GitHub / HTTP work goes through the Tauri bridge. The
// JS context can't reach external HTTPS directly under our CSP, and
// even if it could, the secrets (Airtable PAT, GitHub token) live in
// Keychain, only the Rust side reads them.

import { emptyTodayState } from "./state.js";

const STALE_MS = 5 * 60 * 1000; // 5 minutes
const LIVE_STALE_MS = 60 * 1000; // 60 seconds

function bridge() {
  return window.__TAURI__?.core;
}

async function safeInvoke(cmd, args) {
  const t = bridge();
  if (!t) throw new Error("not in tauri");
  return t.invoke(cmd, args);
}

// ── Helpers ───────────────────────────────────────────────────────────

function todayBounds() {
  // Local-day boundaries in the user's timezone. Airtable returns ISO
  // datetime strings; we compare against these as JS Date objects.
  const now = new Date();
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 0, 0, 0);
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 23, 59, 59);
  return { now, start, end };
}

function parseDate(s) {
  if (!s) return null;
  const d = new Date(s);
  return Number.isNaN(d.getTime()) ? null : d;
}

function clientCodesFromRecord(fields, clientLookup) {
  // Airtable returns multipleRecordLinks as an array of record IDs. We
  // map them to client codes via the lookup we built from the Clients
  // list. If we don't have the lookup yet, return an empty array.
  const ids = fields?.client;
  if (!Array.isArray(ids) || !clientLookup) return [];
  return ids.map((id) => clientLookup[id] || null).filter(Boolean);
}

// ── Section fetchers ──────────────────────────────────────────────────

export async function loadCommitments(state, clientLookup) {
  try {
    const raw = await safeInvoke("list_airtable_commitments");
    const data = JSON.parse(raw);
    const records = (data.records || []).map((r) => ({
      id: r.id,
      ...r.fields,
      _client_codes: clientCodesFromRecord(r.fields, clientLookup),
    }));
    const { start, end, now } = todayBounds();
    const due = [];
    const overdue = [];
    records.forEach((c) => {
      const dueAt = parseDate(c.due_at);
      if (!dueAt) return;
      if (dueAt < now && c.status !== "done" && c.status !== "cancelled") {
        if (dueAt >= start && dueAt <= end) {
          due.push(c);
        } else {
          overdue.push(c);
        }
      } else if (dueAt >= start && dueAt <= end) {
        due.push(c);
      }
    });
    // Sort due by time-of-day asc, overdue by how stale (oldest first).
    due.sort((a, b) => parseDate(a.due_at) - parseDate(b.due_at));
    overdue.sort((a, b) => parseDate(a.due_at) - parseDate(b.due_at));
    state.due_today = state.due_today || { commitments: [], decisions: [] };
    state.due_today.commitments = due;
    state.overdue = overdue;
    state.last_fetch_at.commitments = Date.now();
  } catch (e) {
    console.warn("loadCommitments failed:", e);
    state.due_today = state.due_today || { commitments: [], decisions: [] };
    state.due_today.commitments = state.due_today.commitments || [];
    state.overdue = state.overdue || [];
  }
}

export async function loadDecisions(state, clientLookup) {
  try {
    const raw = await safeInvoke("list_airtable_decisions");
    const data = JSON.parse(raw);
    const records = (data.records || []).map((r) => ({
      id: r.id,
      ...r.fields,
      _client_codes: clientCodesFromRecord(r.fields, clientLookup),
    }));
    const { start, end } = todayBounds();
    const dueToday = [];
    const open = [];
    records.forEach((d) => {
      // due_date is a date-only field; parse as local day.
      const dueDay = d.due_date ? new Date(`${d.due_date}T00:00:00`) : null;
      if (dueDay && dueDay >= start && dueDay <= end) {
        dueToday.push(d);
      }
      open.push(d);
    });
    // Open decisions sorted by due_date asc, undated last.
    open.sort((a, b) => {
      const ad = a.due_date || "9999-12-31";
      const bd = b.due_date || "9999-12-31";
      return ad.localeCompare(bd);
    });
    state.due_today = state.due_today || { commitments: [], decisions: [] };
    state.due_today.decisions = dueToday;
    state.decisions_open = open;
    state.last_fetch_at.decisions = Date.now();
  } catch (e) {
    console.warn("loadDecisions failed:", e);
    state.due_today = state.due_today || { commitments: [], decisions: [] };
    state.due_today.decisions = state.due_today.decisions || [];
    state.decisions_open = state.decisions_open || [];
  }
}

export async function loadWorkstreams(state, clientLookup) {
  try {
    const raw = await safeInvoke("list_airtable_workstreams");
    const data = JSON.parse(raw);
    const records = (data.records || []).map((r) => ({
      id: r.id,
      ...r.fields,
      _client_codes: clientCodesFromRecord(r.fields, clientLookup),
    }));
    // Sort by phase (sprint first, then sustainable, then recovery), then
    // last_touch_at desc inside each phase bucket.
    const phaseRank = { sprint: 0, sustainable: 1, recovery: 2 };
    records.sort((a, b) => {
      const ar = phaseRank[a.phase] ?? 99;
      const br = phaseRank[b.phase] ?? 99;
      if (ar !== br) return ar - br;
      const at = parseDate(a.last_touch_at)?.getTime() || 0;
      const bt = parseDate(b.last_touch_at)?.getTime() || 0;
      return bt - at;
    });
    state.workstreams = records;
    state.last_fetch_at.workstreams = Date.now();
  } catch (e) {
    console.warn("loadWorkstreams failed:", e);
    state.workstreams = state.workstreams || [];
  }
}

export async function loadReceiptsPending(state, clientLookup) {
  try {
    const raw = await safeInvoke("list_airtable_receipts_recent");
    const data = JSON.parse(raw);
    const pending = [];
    (data.records || []).forEach((r) => {
      const f = r.fields || {};
      const ticked = Number(f.ticked_count || 0);
      let total = 0;
      try {
        const payload = JSON.parse(f.json || "{}");
        (payload.sections || []).forEach((s) => {
          (s.items || []).forEach((it) => {
            if (it.type === "task") total += 1;
          });
        });
      } catch {
        return; // skip rows whose JSON won't parse
      }
      if (total === 0) return;
      if (ticked >= total) return;
      pending.push({
        airtable_id: r.id,
        id: f.id,
        title: f.title || "Receipt",
        date: f.date || "",
        workflow: f.workflow || "",
        ticked,
        total,
        _client_codes: clientCodesFromRecord(f, clientLookup),
      });
    });
    state.receipts_pending = pending;
    state.last_fetch_at.receipts_pending = Date.now();
  } catch (e) {
    console.warn("loadReceiptsPending failed:", e);
    state.receipts_pending = state.receipts_pending || [];
  }
}

export async function loadDrift(state, { force = false } = {}) {
  // Drift is a Rust-side composite check (filesystem + Airtable) cached
  // for an hour. Pass `force: true` to bypass the cache — the dashboard
  // refresh button uses that, normal load uses the cached value.
  try {
    const raw = await safeInvoke("check_drift", { force });
    state.drift = JSON.parse(raw) || [];
    state.last_fetch_at.drift = Date.now();
  } catch (e) {
    console.warn("loadDrift failed:", e);
    state.drift = state.drift || [];
  }
}

export async function loadMorningBriefing(state) {
  try {
    const raw = await safeInvoke("read_morning_briefing");
    if (!raw) {
      state.morning_briefing = null;
    } else {
      state.morning_briefing = JSON.parse(raw);
    }
    state.last_fetch_at.morning_briefing = Date.now();
  } catch (e) {
    console.warn("loadMorningBriefing failed:", e);
    state.morning_briefing = null;
  }
}

// ── Calendar (v0.24) ──────────────────────────────────────────────────
//
// Two pulls: today's events and the next 7 days. Both come from the
// same Google session. Status check is cheap (one Keychain read on the
// Rust side) so we always read it — render uses it to decide whether
// to show "Connect Google in Settings" instead of an empty list.

export async function loadCalendar(state) {
  const cal = state.calendar || (state.calendar = {
    status: null,
    today: null,
    week: null,
    error: null,
  });
  try {
    cal.status = await safeInvoke("get_google_status");
  } catch (e) {
    cal.status = { connected: false, has_client_id: false, last_sync_at: null };
  }
  if (!cal.status?.connected) {
    cal.today = [];
    cal.week = [];
    cal.error = null;
    state.last_fetch_at.calendar = Date.now();
    return;
  }
  const [todayRes, weekRes] = await Promise.allSettled([
    safeInvoke("list_calendar_today"),
    safeInvoke("list_calendar_week"),
  ]);
  if (todayRes.status === "fulfilled") {
    try { cal.today = JSON.parse(todayRes.value) || []; }
    catch { cal.today = []; }
  } else {
    cal.today = [];
    cal.error = String(todayRes.reason);
  }
  if (weekRes.status === "fulfilled") {
    try { cal.week = JSON.parse(weekRes.value) || []; }
    catch { cal.week = []; }
  } else {
    cal.week = [];
    if (!cal.error) cal.error = String(weekRes.reason);
  }
  // If both calls succeeded, refresh the status (last_sync_at may have
  // moved). Cheap second call but keeps the Settings-meta line fresh.
  if (todayRes.status === "fulfilled" && weekRes.status === "fulfilled") {
    cal.error = null;
    try { cal.status = await safeInvoke("get_google_status"); } catch {}
  }
  state.last_fetch_at.calendar = Date.now();
}

// ── Email triage (v0.25) ──────────────────────────────────────────────
//
// Two pulls: total unread count and a small urgent-thread list. Both
// only run when Google is connected with the Gmail scope. The Today
// render hides the section when status.connected is false or the gmail
// scope is missing — same hide-on-disconnect pattern as the calendar
// block.

export async function loadEmail(state) {
  const email = state.email || (state.email = { unread: null, urgent: null, error: null });
  // Reuse the calendar-fetched status when fresh; otherwise read.
  let status = state.calendar?.status;
  if (!status) {
    try { status = await safeInvoke("get_google_status"); }
    catch { status = { connected: false, scopes: [] }; }
  }
  const scopes = Array.isArray(status?.scopes) ? status.scopes : [];
  const hasGmail = scopes.includes("gmail");
  if (!status?.connected || !hasGmail) {
    email.unread = null;
    email.urgent = null;
    email.error = null;
    state.last_fetch_at.email = Date.now();
    return;
  }
  const [unreadRes, urgentRes] = await Promise.allSettled([
    safeInvoke("gmail_unread_count"),
    safeInvoke("list_gmail_urgent"),
  ]);
  if (unreadRes.status === "fulfilled") {
    email.unread = Number(unreadRes.value) || 0;
  } else {
    email.unread = null;
    email.error = String(unreadRes.reason);
  }
  if (urgentRes.status === "fulfilled") {
    try { email.urgent = JSON.parse(urgentRes.value) || []; }
    catch { email.urgent = []; }
  } else {
    email.urgent = [];
    if (!email.error) email.error = String(urgentRes.reason);
  }
  state.last_fetch_at.email = Date.now();
}

// ── Slack activity (v0.26) ────────────────────────────────────────────
//
// Reads the OAuth status first; bails early when not connected so we
// don't make network calls. When connected, pulls the unreads-per-channel
// summary and writes into _state.today.slack. Render hides the section
// entirely when status.connected is false.

export async function loadSlack(state) {
  const slack = state.slack || (state.slack = {
    status: null,
    unreads: null,
    error: null,
  });
  try {
    slack.status = await safeInvoke("get_slack_oauth_status");
  } catch (e) {
    slack.status = { connected: false, has_client_id: false, has_client_secret: false, last_sync_at: null };
  }
  if (!slack.status?.connected) {
    slack.unreads = [];
    slack.error = null;
    state.last_fetch_at.slack = Date.now();
    return;
  }
  try {
    const raw = await safeInvoke("list_slack_unreads");
    slack.unreads = JSON.parse(raw) || [];
    slack.error = null;
  } catch (e) {
    slack.unreads = [];
    slack.error = String(e);
  }
  state.last_fetch_at.slack = Date.now();
}

// ── Live status ───────────────────────────────────────────────────────

export async function loadLiveStatus(state) {
  // Run all four checks concurrently, write each into state independently
  // so a slow GitHub check doesn't block the fast Studio version read.
  const tasks = [
    loadStudioVersion(state),
    loadGithubActions(state),
    loadContextForMe(state),
    loadContextDeploy(state),
  ];
  await Promise.allSettled(tasks);
  state.last_fetch_at.live_status = Date.now();
}

async function loadStudioVersion(state) {
  try {
    const v = await safeInvoke("get_studio_version");
    state.live_status.studio = { version: v };
  } catch (e) {
    state.live_status.studio = { version: "unknown", error: String(e) };
  }
}

async function loadGithubActions(state) {
  try {
    const raw = await safeInvoke("check_github_actions", {
      repo: "incahoots-caitlin/companion",
    });
    state.live_status.github_actions = JSON.parse(raw);
  } catch (e) {
    state.live_status.github_actions = { error: String(e) };
  }
}

async function loadContextForMe(state) {
  try {
    const ok = await safeInvoke("check_url_up", { url: "https://contextfor.me" });
    state.live_status.contextfor_me = { ok };
  } catch (e) {
    state.live_status.contextfor_me = { error: String(e) };
  }
}

async function loadContextDeploy(state) {
  // No reliable Vercel API auth path in v0.18. Spec says "fall back to
  // (check manually)" if Vercel isn't easy. Try the contextfor.me up
  // signal as a proxy and leave commit/deployed_at blank.
  state.live_status.context = { manual: true };
}

// ── Top-level loader ──────────────────────────────────────────────────

async function buildClientLookup() {
  // Map record id -> client code so we can decorate commitments and
  // decisions with their client codes for rendering.
  try {
    const raw = await safeInvoke("list_airtable_clients");
    const data = JSON.parse(raw);
    const lookup = {};
    (data.records || []).forEach((r) => {
      const code = r.fields?.code;
      if (code) lookup[r.id] = code;
    });
    return lookup;
  } catch (e) {
    console.warn("client lookup failed:", e);
    return {};
  }
}

export async function loadAll(state) {
  if (!state) state = emptyTodayState();
  if (!bridge()) return state;
  const lookup = await buildClientLookup();
  await Promise.allSettled([
    loadCommitments(state, lookup),
    loadDecisions(state, lookup),
    loadWorkstreams(state, lookup),
    loadReceiptsPending(state, lookup),
    loadMorningBriefing(state),
    loadLiveStatus(state),
    loadDrift(state, { force: true }),
    loadCalendar(state),
    loadSlack(state),
  ]);
  // Email runs after calendar so it can reuse the freshly-fetched
  // Google status without a second Keychain hit.
  await loadEmail(state);
  return state;
}

export async function refreshStale(state) {
  if (!bridge()) return;
  const now = Date.now();
  const lookup = await buildClientLookup();
  const tasks = [];
  if ((state.last_fetch_at.commitments || 0) + STALE_MS < now) {
    tasks.push(loadCommitments(state, lookup));
  }
  if ((state.last_fetch_at.decisions || 0) + STALE_MS < now) {
    tasks.push(loadDecisions(state, lookup));
  }
  if ((state.last_fetch_at.workstreams || 0) + STALE_MS < now) {
    tasks.push(loadWorkstreams(state, lookup));
  }
  if ((state.last_fetch_at.receipts_pending || 0) + STALE_MS < now) {
    tasks.push(loadReceiptsPending(state, lookup));
  }
  if ((state.last_fetch_at.morning_briefing || 0) + STALE_MS < now) {
    tasks.push(loadMorningBriefing(state));
  }
  if ((state.last_fetch_at.live_status || 0) + LIVE_STALE_MS < now) {
    tasks.push(loadLiveStatus(state));
  }
  // Drift TTL is on the Rust side (1hr); pulling on every refreshStale
  // is cheap because the Rust cache returns immediately when warm.
  tasks.push(loadDrift(state));
  if ((state.last_fetch_at.calendar || 0) + STALE_MS < now) {
    tasks.push(loadCalendar(state));
  }
  if ((state.last_fetch_at.email || 0) + STALE_MS < now) {
    tasks.push(loadEmail(state));
  }
  if ((state.last_fetch_at.slack || 0) + STALE_MS < now) {
    tasks.push(loadSlack(state));
  }
  await Promise.allSettled(tasks);
}

export const TIMINGS = { STALE_MS, LIVE_STALE_MS };
