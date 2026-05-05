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
// v0.21 added three pure-Airtable workflows (Schedule social post, Log
// time, Edit project) — these don't hit Claude, just file rows + a Receipt.

export function launch(key, clientCode, ctx) {
  // ctx.modals = { showStrategicThinkingModal, showMonthlyCheckinModal, ... }
  // ctx.toast  = (msg, opts?) => void
  // ctx.requireApiKey = async () => boolean
  if (!ctx?.modals) return;

  const dispatchTable = {
    "monthly-checkin": () => ctx.modals.showMonthlyCheckinModal(clientCode),
    "new-campaign-scope": () => ctx.modals.showNewCampaignScopeModal(clientCode),
    "quarterly-review": () => ctx.modals.showQuarterlyReviewModal(clientCode),
    "strategic-thinking": () => ctx.modals.showStrategicThinkingModal(),
    "schedule-social-post": () => ctx.modals.showScheduleSocialPostModal(clientCode),
    "log-time": () => ctx.modals.showLogTimeModal(clientCode),
    "edit-project": () => ctx.modals.showEditProjectModal(clientCode),
  };

  const fn = dispatchTable[key];
  if (!fn) {
    ctx.toast?.(`Unknown workflow: ${key}`);
    return;
  }

  // Workflows that hit Claude need the API key set first. Pure-Airtable
  // workflows skip the check.
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
