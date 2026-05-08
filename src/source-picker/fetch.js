// Source-picker — fetch (v0.32 Block F).
//
// Calls the Tauri command `fetch_workflow_context` which routes by
// source_type to Granola / Gmail / Slack / Calendar / Form / Manual and
// returns a normalised markdown context blob (header + body). The blob
// is what gets prepended to the workflow's user message — see each
// run_* command's user_message construction.

import { setContextBlob, setError } from "./state.js";

function tauri() {
  return window.__TAURI__?.core;
}

// Fetch the context blob for the current source. The "manual" source
// short-circuits to wrapping the textarea content in the same envelope
// shape so workflows don't have to branch on source type.
export async function fetchContext(state) {
  const t = tauri();
  if (!t) {
    setError(state, "Companion backend not available");
    return null;
  }

  state.fetching = true;
  state.error = null;
  try {
    const blob = await t.invoke("fetch_workflow_context", {
      input: {
        client_code: state.clientCode || null,
        source_type: state.sourceType,
        source_ref: state.sourceRef || null,
      },
    });
    setContextBlob(state, blob);
    return blob;
  } catch (e) {
    const msg = String(e?.message || e);
    setError(state, msg);
    return null;
  } finally {
    state.fetching = false;
  }
}

// Probe which sources have OAuth/MCP wired so the picker can hide
// unsupported options at runtime. Best-effort — failures fall back to
// "available" so we don't accidentally hide working surfaces.
export async function probeAvailability() {
  const t = tauri();
  const available = {
    granola: false,
    gmail: false,
    slack: false,
    calendar: false,
    form: true,    // Airtable read; assume on (Companion needs Airtable)
    manual: true,  // always on
  };
  if (!t) return available;

  // v0.41: status.state === "verified" replaces the old status.connected
  // bool. We treat anything other than verified as unavailable so
  // source-picker doesn't fan out to integrations that will fail at
  // call-time. "Failing" status surfaces in Settings; the picker just
  // hides the source until Caitlin re-verifies.
  const isVerified = (s) => s && s.state === "verified";

  // Granola
  try {
    const status = await t.invoke("get_granola_status");
    available.granola = isVerified(status);
  } catch (_) {}

  // Google (gmail + calendar share OAuth)
  try {
    const status = await t.invoke("get_google_status");
    const connected = isVerified(status);
    available.gmail = connected;
    available.calendar = connected;
  } catch (_) {}

  // Slack (OAuth path only — webhook write doesn't apply)
  try {
    const status = await t.invoke("get_slack_oauth_status");
    available.slack = isVerified(status);
  } catch (_) {}

  return available;
}
