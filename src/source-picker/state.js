// Source-picker — state (v0.32 Block F).
//
// Tracks which source the user has chosen for a workflow's input, plus
// the fetched context blob once they've pulled it. Each modal that uses
// the source-picker holds its own state instance — there's no global.
//
// Source type keys (string, kept lowercase to match Rust):
//   "granola"  — Granola call list (existing pull from v0.22)
//   "gmail"    — Gmail thread, by ID or search (Gmail MCP from v0.25)
//   "slack"    — Slack thread URL or #client-{slug} pull (v0.26)
//   "calendar" — Google Calendar event picker (v0.24)
//   "form"     — Latest Lead Intake / Discovery Pre-Brief submission
//   "manual"   — Plain textarea paste (always available)

export const SOURCE_TYPES = ["granola", "gmail", "slack", "calendar", "form", "manual"];

export const SOURCE_LABELS = {
  granola: "Granola call",
  gmail: "Gmail thread",
  slack: "Slack thread",
  calendar: "Calendar event",
  form: "Form submission",
  manual: "Manual paste",
};

export function createState({ clientCode = null } = {}) {
  return {
    clientCode,
    sourceType: "manual",
    sourceRef: "",
    contextBlob: null, // string when fetched, null when pending
    fetching: false,
    error: null,
  };
}

// Replace the source type and clear any prior fetched blob — switching
// source means the previous blob is stale.
export function setSource(state, sourceType) {
  state.sourceType = sourceType;
  state.sourceRef = "";
  state.contextBlob = null;
  state.error = null;
  return state;
}

export function setSourceRef(state, ref) {
  state.sourceRef = ref;
  return state;
}

export function setContextBlob(state, blob) {
  state.contextBlob = blob;
  state.error = null;
  return state;
}

export function setError(state, message) {
  state.error = message;
  return state;
}

export function clear(state) {
  state.contextBlob = null;
  state.error = null;
  state.fetching = false;
  return state;
}
