// Skills registry — single source of truth for the skill list (v0.34).
//
// Caitlin's reframe: workflows are the views (Today, per-client, per-
// project, Team, Personal: Social launch). Skills are the actions inside
// a workflow. Each skill knows where it surfaces and how to launch.
//
// Each entry:
//   id           — stable string key, used by main.js to dispatch.
//   label        — UI title.
//   description  — short subtitle on the card.
//   category     — one of: onboard | cycle | draft | quick.
//                   Drives grouping in the overflow modal.
//   contexts     — array of view names where this skill surfaces:
//                   today_quick   — the 3-card Quick Skills strip on Today
//                   client        — per-client view's Skills section
//                   project       — per-project view's Skills section
//                   team          — Team view's Skills section
//                   subcontractor — per-Subcontractor view's Skills section (v0.38)
//                   social_launch — Personal: Social launch Skills section
//                   overflow      — appears in the More skills modal
//                  (overflow is implicit for every skill — the modal lists
//                   the lot grouped by category. Other contexts are opt-in.)
//   gates        — optional array of hints for filtering at render time:
//                   only_for_client: "NCT"
//                   project_status: ["wrap","done","wrapped","complete","completed"]
//                   client_only_at_create — hide on existing clients (NCO)
//   needs        — what context the dispatcher pre-fills:
//                   none | client_code | project_code
//   placeholder  — true if the underlying modal isn't wired yet. The
//                  overflow card still renders but the dispatcher toasts
//                  "not wired yet" instead of opening a missing modal.

export const SKILLS = [
  // ── Onboard ──────────────────────────────────────────────────────────
  {
    id: "new-client-onboarding",
    label: "New Client Onboarding",
    description: "Discovery → scope → CSA → handoff",
    category: "onboard",
    contexts: ["client"],
    gates: { client_only_at_create: true },
    needs: "none",
  },
  {
    id: "subcontractor-onboarding",
    label: "Subcontractor Onboarding",
    description: "CSA + role + pre-start info",
    category: "onboard",
    contexts: ["team", "subcontractor"],
    needs: "none",
  },
  {
    id: "campaign-launch-checklist",
    label: "Campaign Launch Checklist",
    description: "Six-phase pre-launch verification",
    category: "onboard",
    contexts: ["project"],
    gates: {
      project_type_or_status: {
        types: ["campaign"],
        statuses: ["active"],
      },
    },
    needs: "project_code",
  },
  {
    id: "promote-lead",
    label: "Promote Lead",
    description: "Cascade lead → client + discovery project",
    category: "onboard",
    contexts: [],
    needs: "none",
    placeholder: true, // Promote Lead lives on the Pipeline view per-row
  },

  // ── Cycle ────────────────────────────────────────────────────────────
  {
    id: "monthly-checkin",
    label: "Monthly Check-in",
    description: "Last 30 days · pre-filled to this client",
    category: "cycle",
    contexts: ["client"],
    needs: "client_code",
  },
  {
    id: "quarterly-review",
    label: "Quarterly Review",
    description: "Last 90 days · QBR receipt",
    category: "cycle",
    contexts: ["client"],
    needs: "client_code",
  },
  {
    id: "strategic-thinking",
    label: "Strategic Thinking",
    description: "Open thinking session",
    category: "cycle",
    contexts: ["today_quick"],
    needs: "none",
  },
  {
    id: "friday-close",
    label: "Friday Close",
    description: "Manual trigger · weekly close-out",
    category: "cycle",
    contexts: [],
    needs: "none",
    placeholder: true,
  },
  {
    id: "monday-status-sweep",
    label: "Monday Status Sweep",
    description: "Manual trigger · weekly status sweep",
    category: "cycle",
    contexts: [],
    needs: "none",
    placeholder: true,
  },

  // ── Draft ────────────────────────────────────────────────────────────
  {
    id: "build-scope",
    label: "Build Scope",
    description: "SOW in Caitlin's voice",
    category: "draft",
    contexts: ["client", "project"],
    needs: "client_code",
  },
  {
    id: "wrap-report",
    label: "Draft Wrap Report",
    description: "For projects at wrap or done",
    category: "draft",
    contexts: ["client", "project"],
    gates: {
      project_status: ["wrap", "done", "wrapped", "complete", "completed"],
    },
    needs: "project_code",
  },
  {
    id: "press-release",
    label: "Draft Press Release",
    description: "From any source brief",
    category: "draft",
    contexts: ["client", "project"],
    needs: "client_code",
  },
  {
    id: "edm-writer",
    label: "Draft EDM",
    description: "Subject lines + full body",
    category: "draft",
    contexts: ["client", "project"],
    needs: "client_code",
  },
  {
    id: "reels-scripting",
    label: "Script a Reel",
    description: "Hook, beats, captions",
    category: "draft",
    contexts: ["client", "project"],
    needs: "client_code",
  },
  {
    id: "hook-generator",
    label: "Generate hooks",
    description: "6 variants for any topic",
    category: "draft",
    contexts: ["client", "project"],
    needs: "none",
  },
  {
    id: "draft-caption",
    label: "Draft caption",
    description: "Social caption · 3 variants",
    category: "draft",
    contexts: ["client", "project"],
    needs: "client_code",
  },
  {
    id: "nct-caption",
    label: "Draft NCT social caption",
    description: "Venue voice · 3 variants",
    category: "draft",
    contexts: ["client", "project"],
    gates: { only_for_client: "NCT" },
    needs: "client_code",
  },
  {
    id: "client-email",
    label: "Draft email to client",
    description: "Caitlin's voice, ready to humanise",
    category: "draft",
    contexts: ["client"],
    needs: "client_code",
  },
  {
    id: "humanizer",
    label: "Edit pass: humanise",
    description: "Strip AI tells from any draft",
    category: "draft",
    contexts: [],
    needs: "none",
  },
  {
    id: "copy-editor",
    label: "Edit pass: copy editor",
    description: "Tighten + match voice",
    category: "draft",
    contexts: [],
    needs: "none",
  },
  {
    id: "in-cahoots-social",
    label: "In Cahoots social post",
    description: "Founder-led brand draft",
    category: "draft",
    contexts: ["social_launch"],
    needs: "none",
  },

  // ── Quick ────────────────────────────────────────────────────────────
  {
    id: "log-time",
    label: "Log time",
    description: "Hours → TimeLogs",
    category: "quick",
    contexts: ["today_quick", "subcontractor"],
    needs: "none",
  },
  {
    id: "schedule-social-post",
    label: "Schedule social post",
    description: "Drafts to SocialPosts",
    category: "quick",
    contexts: ["today_quick", "subcontractor"],
    needs: "none",
  },
  {
    id: "edit-project",
    label: "Edit project",
    description: "Update fields, file diff",
    category: "quick",
    contexts: ["client", "project", "subcontractor"],
    needs: "client_code",
  },
];

// Category metadata for the overflow modal.
export const CATEGORIES = [
  { key: "onboard", label: "Onboard" },
  { key: "cycle", label: "Cycle" },
  { key: "draft", label: "Draft" },
  { key: "quick", label: "Quick" },
];

// ── Helpers ────────────────────────────────────────────────────────────

// Skills that should appear in a given context. Optional `filter` args:
//   client_code     — uppercase code to test only_for_client gates
//   project_status  — lowercase status to test project_status gates
//   project_type    — lowercase campaign_type to test project_type_or_status
//   has_existing    — bool, true when client already exists (hides
//                     client_only_at_create skills like NCO)
export function skillsForContext(context, filter = {}) {
  const code = (filter.client_code || "").toUpperCase();
  const status = String(filter.project_status || "").toLowerCase();
  const type = String(filter.project_type || "").toLowerCase();
  const hasExisting = filter.has_existing !== false; // default true
  return SKILLS.filter((s) => {
    if (!s.contexts.includes(context)) return false;
    if (s.gates?.only_for_client && s.gates.only_for_client !== code) return false;
    if (s.gates?.project_status && !s.gates.project_status.includes(status)) return false;
    if (s.gates?.client_only_at_create && hasExisting) return false;
    if (s.gates?.project_type_or_status) {
      const { types = [], statuses = [] } = s.gates.project_type_or_status;
      const typeOk = types.length === 0 ? false : types.includes(type);
      const statusOk = statuses.length === 0 ? false : statuses.includes(status);
      if (!typeOk && !statusOk) return false;
    }
    return true;
  });
}

export function skillsByCategory() {
  const grouped = new Map();
  CATEGORIES.forEach((c) => grouped.set(c.key, []));
  SKILLS.forEach((s) => {
    if (!grouped.has(s.category)) grouped.set(s.category, []);
    grouped.get(s.category).push(s);
  });
  return grouped;
}

export function findSkill(id) {
  return SKILLS.find((s) => s.id === id) || null;
}
