// Per-client view — workflow launchers.
//
// Thin dispatch layer: takes a workflow key from the per-client workflow
// grid and calls the matching modal function in main.js with the client
// code passed in as a prefill.
//
// Most modals already accept `prefillClientCode` (added when the per-client
// shortcuts shipped in v0.15). New Client Onboarding and Subcontractor
// Onboarding don't take a client argument because they create the client.
//
// Three workflows in the grid are placeholders (Schedule social post, Log
// time, Edit project) — those show a toast saying they ship in v0.20.

export function launch(key, clientCode, ctx) {
  // ctx.modals = { showStrategicThinkingModal, showMonthlyCheckinModal, ... }
  // ctx.toast  = (msg, opts?) => void
  // ctx.requireApiKey = async () => boolean
  if (!ctx?.modals) return;

  const placeholders = ["schedule-social-post", "log-time", "edit-project"];
  if (placeholders.includes(key)) {
    ctx.toast?.("Ships in v0.20");
    return;
  }

  const dispatchTable = {
    "monthly-checkin": () => ctx.modals.showMonthlyCheckinModal(clientCode),
    "new-campaign-scope": () => ctx.modals.showNewCampaignScopeModal(clientCode),
    "quarterly-review": () => ctx.modals.showQuarterlyReviewModal(clientCode),
    "strategic-thinking": () => ctx.modals.showStrategicThinkingModal(),
  };

  const fn = dispatchTable[key];
  if (!fn) {
    ctx.toast?.(`Unknown workflow: ${key}`);
    return;
  }

  // Workflows that hit Claude need the API key set first.
  const needsKey = [
    "monthly-checkin",
    "new-campaign-scope",
    "quarterly-review",
    "strategic-thinking",
  ];
  if (needsKey.includes(key) && ctx.requireApiKey) {
    ctx.requireApiKey().then((ok) => {
      if (ok) fn();
    });
  } else {
    fn();
  }
}
