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
    last_fetch_at: {},
  };
}
