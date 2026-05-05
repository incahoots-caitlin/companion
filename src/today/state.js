// Today dashboard — state shape.
//
// Mounted on the global `_state.today`. Render functions read from here;
// fetch functions write to here. Keeps render and fetch decoupled.
//
// Shape:
//
// _state.today = {
//   due_today: { commitments: [...], decisions: [...] },
//   overdue:   [ ...commitments past due ],
//   workstreams: [ ...active or blocked workstreams ],
//   decisions_open: [ ...open decisions sorted by due_date asc ],
//   live_status: {
//     studio:        { version, released_at? },
//     context:       { commit?, deployed_at?, status? }, // best-effort
//     github_actions:{ ok, conclusion, url, updated_at },
//     contextfor_me: { ok }
//   },
//   drift: [],                              // hidden until Block B6
//   receipts_pending: [ { id, title, ticked, total, client_code } ],
//   morning_briefing: { text, generated_at, path } | null,
//   calendar: {
//     status: { connected: bool, has_client_id: bool, last_sync_at, scopes },
//     today: [ ...CalendarEvent ] | null,
//     week:  [ ...CalendarEvent ] | null,
//     error: string | null,
//   },
//   email: {
//     // v0.25 — only filled in when Google is connected with the gmail
//     // scope. Hidden entirely otherwise.
//     unread: number | null,
//     urgent: [ ...EmailThread ] | null,
//     error: string | null,
//   },
//   slack: {
//     // v0.26 — only filled in when Slack OAuth is connected. Hidden
//     // entirely otherwise. unreads is null while loading, an empty
//     // array when the user has nothing to triage.
//     status: { connected: bool, has_client_id, has_client_secret, last_sync_at },
//     unreads: [ ...UnreadSummary ] | null,
//     error: string | null,
//   },
//   last_fetch_at:        { all section keys } -> ms timestamp
// }
//
// Each top-level key may be `null` while a section is loading. Render
// functions show a skeleton ("Loading...") in that state and the empty
// string in the steady-state empty state.

export function emptyTodayState() {
  return {
    due_today: null,
    overdue: null,
    workstreams: null,
    decisions_open: null,
    live_status: {
      studio: null,
      context: null,
      github_actions: null,
      contextfor_me: null,
    },
    drift: null,
    receipts_pending: null,
    morning_briefing: null,
    calendar: {
      status: null,
      today: null,
      week: null,
      error: null,
    },
    email: {
      unread: null,
      urgent: null,
      error: null,
    },
    slack: {
      status: null,
      unreads: null,
      error: null,
    },
    last_fetch_at: {},
  };
}
