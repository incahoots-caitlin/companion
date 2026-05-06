// Per-project view — state shape (v0.28 Block E).
//
// Mounted on the global `_state.project`. The per-project view replaces
// the right pane (#client-view) when a project is opened — same pattern
// as the chat surface in v0.27. Switching projects overwrites this slot,
// no per-project cache.
//
// Shape:
//
// _state.project = {
//   project_code: 'NCT-2026-06-tour-launch',
//   header: { code, name, status, campaign_type, start_date, end_date,
//             budget_total, brief_url, notes, client_code, client_name,
//             client_record_id, project_record_id },
//   updates: [],   // unified ProjectUpdate list, sorted desc
//   composer: { body: '', tags: [], saving: false, error: null },
//   loading: false,
//   error: null,
// }
//
// `updates: null` means "loading"; `[]` means "loaded, none". The
// renderer reads that distinction.

export function emptyProjectState() {
  return {
    project_code: null,
    header: null,
    updates: null,
    composer: { body: "", tags: [], saving: false, error: null },
    loading: false,
    error: null,
  };
}

// All seven note tag options. Defined here so the composer renders the
// same set the Airtable singleSelect was created with.
export const NOTE_TAGS = [
  "idea",
  "blocker",
  "decision-pending",
  "follow-up",
  "win",
  "risk",
  "miscellaneous",
];
