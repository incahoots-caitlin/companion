// Skills dispatcher (v0.34).
//
// Single entry point for skill-card clicks from any view. main.js builds
// a `ctx` object with the modal openers + helpers (toast, requireApiKey),
// and calls dispatch(skill_id, options) — we route to the right modal,
// pre-filling client_code or project_code where the registry says so.
//
// This replaces the per-view dispatch tables (client/workflows.js stays
// for backwards-compat with anything still wired to client:workflow-click,
// but new code should go through this).

import { findSkill } from "./registry.js";

// Skills that hit Anthropic and need an API key. Pure-Airtable skills
// skip the check.
const NEEDS_API_KEY = new Set([
  "monthly-checkin",
  "quarterly-review",
  "strategic-thinking",
  "new-client-onboarding",
  "subcontractor-onboarding",
  "build-scope",
  "wrap-report",
  "press-release",
  "edm-writer",
  "reels-scripting",
  "hook-generator",
  "draft-caption",
  "nct-caption",
  "client-email",
  "humanizer",
  "copy-editor",
  "in-cahoots-social",
]);

export async function dispatch(skillId, options = {}, ctx = {}) {
  const skill = findSkill(skillId);
  if (!skill) {
    ctx.toast?.(`Unknown skill: ${skillId}`);
    return;
  }
  if (skill.placeholder) {
    ctx.toast?.(`${skill.label} isn't wired yet.`);
    return;
  }
  if (!ctx.modals) return;

  // Gate: Tauri-only + API key for skills that hit Anthropic.
  if (NEEDS_API_KEY.has(skillId) && ctx.requireApiKey) {
    const ok = await ctx.requireApiKey();
    if (!ok) return;
  } else if (ctx.requireTauri) {
    const ok = await ctx.requireTauri();
    if (!ok) return;
  }

  const clientCode = options.client_code || null;
  const projectCode = options.project_code || null;

  switch (skillId) {
    case "new-client-onboarding":
      return ctx.modals.showNewClientOnboardingModal?.();
    case "subcontractor-onboarding":
      return ctx.modals.showSubcontractorOnboardingModal?.();
    case "monthly-checkin":
      return ctx.modals.showMonthlyCheckinModal?.(clientCode);
    case "quarterly-review":
      return ctx.modals.showQuarterlyReviewModal?.(clientCode);
    case "strategic-thinking":
      return ctx.modals.showStrategicThinkingModal?.();
    case "build-scope":
      return ctx.modals.showBuildScopeModal?.(clientCode);
    case "wrap-report":
      return ctx.modals.showWrapReportModal?.(projectCode);
    case "press-release":
      return ctx.modals.showPressReleaseModal?.(clientCode);
    case "edm-writer":
      return ctx.modals.showEdmWriterModal?.(clientCode);
    case "reels-scripting":
      return ctx.modals.showReelsScriptingModal?.(clientCode);
    case "hook-generator":
      return ctx.modals.showHookGeneratorModal?.();
    case "draft-caption":
      // Generic caption falls through to the NCT writer for NCT clients;
      // anywhere else we use the social-caption-writer skill which is
      // wired through reels/social. For now the closest live modal is
      // the NCT one when client_code = NCT, else show in-cahoots social
      // as a fallback. The proper per-client variant lands later.
      if ((clientCode || "").toUpperCase() === "NCT") {
        return ctx.modals.showNctCaptionModal?.(clientCode);
      }
      return ctx.modals.showInCahootsSocialModal?.();
    case "nct-caption":
      return ctx.modals.showNctCaptionModal?.(clientCode);
    case "client-email":
      return ctx.modals.showClientEmailModal?.(clientCode);
    case "humanizer":
      return ctx.modals.showHumaniserModal?.();
    case "copy-editor":
      return ctx.modals.showCopyEditorModal?.();
    case "in-cahoots-social":
      return ctx.modals.showInCahootsSocialModal?.();
    case "log-time":
      return ctx.modals.showLogTimeModal?.(clientCode);
    case "schedule-social-post":
      return ctx.modals.showScheduleSocialPostModal?.(clientCode);
    case "edit-project":
      return ctx.modals.showEditProjectModal?.(clientCode);
    default:
      ctx.toast?.(`No dispatcher for ${skillId}`);
  }
}
