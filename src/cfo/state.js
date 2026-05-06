// Studio CFO — state shape (v0.37 Block F).
//
// Lives at _state.cfo. Pure data; render reads from here, fetch writes
// here. Fetch only runs while the view is mounted.
//
// Shape:
// _state.cfo = {
//   year: 2026,
//   month: 5,                 // 1-12
//   totals: StudioTotals | null,
//   per_client: ClientFinancials[] | null,
//   alerts: HourCreepAlert[] | null,
//   outlook: NextMonthOutlook | null,
//   error: string | null,
//   loading: bool,
// }

export function emptyCfoState() {
  const now = new Date();
  return {
    year: now.getFullYear(),
    month: now.getMonth() + 1,
    totals: null,
    per_client: null,
    alerts: null,
    outlook: null,
    error: null,
    loading: false,
  };
}

export function shiftMonth(state, delta) {
  let y = state.year;
  let m = state.month + delta;
  while (m < 1) {
    m += 12;
    y -= 1;
  }
  while (m > 12) {
    m -= 12;
    y += 1;
  }
  state.year = y;
  state.month = m;
}
