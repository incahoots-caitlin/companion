// Per-client view — state shape.
//
// Mounted on the global `_state.client`. Render reads from here, fetch
// writes to here. One-client-at-a-time: switching clients overwrites this
// slot, no per-client cache. The Today dashboard owns its own slot at
// `_state.today` and is unaffected.
//
// Shape:
//
// _state.client = {
//   code: 'NCT',
//   recordId: 'recXXX',
//   header: { name, status, primary_contact_name, primary_contact_email,
//             abn, dropbox_folder, last_touch },
//   workstreams: [],   // status in (active, blocked) AND client = recordId
//   decisions: [],     // status = open AND client = recordId
//   commitments: [],   // status = open AND client = recordId
//   projects: [],      // client = recordId, sorted by start_date desc
//   receipts: [],      // last 10 for this client
//   last_fetch_at: { ...section keys -> ms timestamp }
// }
//
// `null` on a section means "loading"; an empty array means "loaded, none".
// The render layer reads that distinction and shows skeleton vs empty state.

export function emptyClientState() {
  return {
    code: null,
    recordId: null,
    header: null,
    workstreams: null,
    decisions: null,
    commitments: null,
    projects: null,
    receipts: null,
    last_fetch_at: {},
  };
}
