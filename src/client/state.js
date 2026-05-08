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
//             abn, dropbox_folder, last_touch, gmail_thread_filter,
//             drive_folder_id },
//   workstreams: [],   // status in (active, blocked) AND client = recordId
//   decisions: [],     // status = open AND client = recordId
//   commitments: [],   // status = open AND client = recordId
//   projects: [],      // client = recordId, sorted by start_date desc
//   receipts: [],      // last 10 for this client
//   meetings: [],      // upcoming Calendar events that mention this client
//   emails: [],        // last 3 Gmail threads matching the client filter
//   drive_files: [],   // Drive files modified in last 14 days for client folder
//   slack_activity: { channel, messages } | null,  // last 24h on #client-{slug}; null when no channel matches or not connected
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
    meetings: null,
    emails: null,
    drive_files: null,
    slack_activity: null,
    // v0.43 — Recent and upcoming Calendar events for this client
    // (yesterday → +7d). null while loading, empty array when nothing
    // matches.
    recent_upcoming: null,
    last_fetch_at: {},
  };
}
