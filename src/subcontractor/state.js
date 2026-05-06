// Per-Subcontractor view state (v0.38).
//
// Mirrors the shape returned by the Tauri load_subcontractor_view
// command. The view container reads only this object.

export function emptyState(code = "") {
  return {
    code: (code || "").toUpperCase(),
    record_id: "",
    header: null,
    workstreams: [],
    commitments: [],
    timelogs: [],
    receipts: [],
    month_start: "",
    loading: false,
    error: null,
  };
}
