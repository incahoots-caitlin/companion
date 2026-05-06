// Studio CFO — fetch (v0.37 Block F).
//
// Calls the four cfo_* Tauri commands and writes results into the
// passed-in state object. Each call is independent, so a slow Airtable
// query for one slice doesn't block the rest. Errors per-slice land on
// state.error so the renderer can show a banner.

function bridge() {
  return typeof window !== "undefined" && window.__TAURI_INTERNALS__;
}

async function safeInvoke(cmd, args = undefined) {
  if (!bridge()) throw new Error("Tauri bridge not available");
  return await window.__TAURI_INTERNALS__.invoke(cmd, args);
}

export async function loadAll(state) {
  if (!bridge()) {
    state.error = "Open the Companion app to load CFO data. Preview is read-only.";
    return state;
  }
  state.loading = true;
  state.error = null;
  const args = { year: state.year, month: state.month };
  const tasks = [
    safeInvoke("cfo_studio_totals", args).then(
      (r) => (state.totals = r),
      (e) => (state.error = String(e))
    ),
    safeInvoke("cfo_per_client", args).then(
      (r) => (state.per_client = r),
      (e) => (state.error = String(e))
    ),
    safeInvoke("cfo_hour_creep_alerts").then(
      (r) => (state.alerts = r),
      (e) => (state.error = String(e))
    ),
    safeInvoke("cfo_outlook", args).then(
      (r) => (state.outlook = r),
      (e) => (state.error = String(e))
    ),
  ];
  await Promise.allSettled(tasks);
  state.loading = false;
  return state;
}

// Used by Today's live status row to pull just the margin number.
// Returns null when the bridge isn't available so the caller can hide
// the row gracefully.
export async function loadCurrentMonthMargin() {
  if (!bridge()) return null;
  try {
    const now = new Date();
    const totals = await safeInvoke("cfo_studio_totals", {
      year: now.getFullYear(),
      month: now.getMonth() + 1,
    });
    return totals;
  } catch (e) {
    return { error: String(e) };
  }
}
