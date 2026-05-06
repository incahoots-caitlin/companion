// Companion - v0 spike
//
// Renders the Strategic Thinking session receipt as the first
// item in the feed. AI workflow runner wires up in the next build.

import * as todayFetch from "./today/fetch.js";
import * as todayRender from "./today/render.js";
import { emptyTodayState } from "./today/state.js";
import * as clientFetch from "./client/fetch.js";
import * as clientRender from "./client/render.js";
import * as clientWorkflows from "./client/workflows.js";
import * as clientSidebar from "./client/sidebar.js";
import { emptyClientState } from "./client/state.js";
import * as convFetch from "./conversations/fetch.js";
import * as convRender from "./conversations/render.js";
import { emptyConversationState } from "./conversations/state.js";
import * as projectFetch from "./projects/fetch.js";
import * as projectRender from "./projects/render.js";
import { emptyProjectState } from "./projects/state.js";
import * as subcontractorFetch from "./subcontractor/fetch.js";
import * as subcontractorRender from "./subcontractor/render.js";
import { emptyState as emptySubcontractorState } from "./subcontractor/state.js";
import * as forms from "./forms/index.js";
import { createState as createPickerState } from "./source-picker/state.js";
import { mountSourcePicker } from "./source-picker/render.js";
import { probeAvailability as probeSourceAvailability } from "./source-picker/fetch.js";
import { dispatch as dispatchSkill } from "./skills/dispatch.js";
import { show as showSkillsOverflow } from "./skills/overflow.js";
import { skillsForContext } from "./skills/registry.js";
import * as globalSearch from "./search/index.js";
import * as cfoFetch from "./cfo/fetch.js";
import * as cfoRender from "./cfo/render.js";
import { emptyCfoState, shiftMonth as cfoShiftMonth } from "./cfo/state.js";
import { poller, CADENCE_PRESETS, readCadencePreference, writeCadencePreference } from "./live/poller.js";

// Global state container. Today dashboard owns _state.today; per-client
// view owns _state.client. Switching clients overwrites _state.client.
// _state.conversation owns the chat surface (v0.27 Block D).
// _state.project owns the per-project Updates surface (v0.28 Block E).
const _state = {
  today: emptyTodayState(),
  client: emptyClientState(),
  conversation: emptyConversationState(),
  project: emptyProjectState(),
  cfo: emptyCfoState(),
  subcontractor: emptySubcontractorState(),
};

// Track whether the chat surface is mounted on top of #client-view. Set
// when a workstream is opened, cleared when the user hits "Back" or
// switches clients.
let _chatActive = false;
// Track whether the per-project view is mounted on top of #client-view.
let _projectActive = false;

// v0.40 — live mode. Polling is now driven by the central `poller`
// instance from src/live/poller.js. Sections register themselves on
// view mount and unregister on view switch. The poller handles
// foreground/background pause, manual refresh, and per-section error
// backoff. The legacy clearLiveStatusTimer / clearCalendarTimer names
// are kept as no-op shims so the surrounding view-switch code still
// reads cleanly; they delegate to poller.unregisterPrefix.
function clearLiveStatusTimer() {
  // v0.40 — drops only the today.* pollers (client/project lifecycles
  // are managed by their own loaders).
  poller.unregisterPrefix("today.");
}

function startLiveStatusTimer() {
  // Wired from registerTodayPollers() — kept as a stub so the existing
  // showTodayView path stays the same shape.
}

function clearCalendarTimer() {
  // Calendar is now part of the today.* poller bundle and handled by
  // clearLiveStatusTimer above.
}

function startCalendarTimer() {
  // Calendar is now part of the today.* poller bundle.
}

// ── Today view: register live pollers ─────────────────────────────────
//
// Called from showTodayView and the initial mount. Each section gets its
// own poller registration so a slow GitHub Actions check doesn't block
// the workstreams refresh. The data sections share the "data" cadence
// (default 30s); live status uses "status" cadence (default 10s).
function registerTodayPollers() {
  if (!isTauri) return;
  const lookupRef = { value: null };
  const ensureLookup = async () => {
    if (lookupRef.value) return lookupRef.value;
    lookupRef.value = await rebuildClientLookup();
    return lookupRef.value;
  };

  poller.unregisterPrefix("today.");

  poller.register({
    id: "today.commitments",
    kind: "data",
    fetch: async () => {
      const lookup = await ensureLookup();
      await todayFetch.loadCommitments(_state.today, lookup);
    },
    onAfter: () => {
      todayRender.drawSection(_state.today, "due-today");
      todayRender.drawSection(_state.today, "overdue");
    },
    getHeader: () => todayRender.sectionHeader("due-today"),
    getSignature: () => todaySignature("commitments", _state.today),
  });

  poller.register({
    id: "today.decisions",
    kind: "data",
    fetch: async () => {
      const lookup = await ensureLookup();
      await todayFetch.loadDecisions(_state.today, lookup);
    },
    onAfter: () => {
      todayRender.drawSection(_state.today, "decisions");
    },
    getHeader: () => todayRender.sectionHeader("decisions"),
    getSignature: () => todaySignature("decisions", _state.today),
  });

  poller.register({
    id: "today.workstreams",
    kind: "data",
    fetch: async () => {
      const lookup = await ensureLookup();
      await todayFetch.loadWorkstreams(_state.today, lookup);
    },
    onAfter: () => {
      todayRender.drawSection(_state.today, "workstreams");
    },
    getHeader: () => todayRender.sectionHeader("workstreams"),
    getSignature: () => todaySignature("workstreams", _state.today),
  });

  poller.register({
    id: "today.receipts_pending",
    kind: "data",
    fetch: async () => {
      const lookup = await ensureLookup();
      await todayFetch.loadReceiptsPending(_state.today, lookup);
    },
    onAfter: () => {
      todayRender.drawSection(_state.today, "receipts-pending");
    },
    getHeader: () => todayRender.sectionHeader("receipts-pending"),
    getSignature: () => todaySignature("receipts_pending", _state.today),
  });

  poller.register({
    id: "today.drift",
    kind: "data",
    fetch: async () => {
      await todayFetch.loadDrift(_state.today);
    },
    onAfter: () => {
      todayRender.drawSection(_state.today, "drift");
    },
    getHeader: () => todayRender.sectionHeader("drift"),
    getSignature: () => todaySignature("drift", _state.today),
  });

  poller.register({
    id: "today.notes",
    // ProjectNotes don't have a today-level surface yet; the per-project
    // updates feed handles them. Reserved id slot so the brief's "project
    // notes refresh on Today" line has a registered owner if added later.
    kind: "data",
    fetch: async () => {},
    onAfter: () => {},
    getHeader: () => null,
    getSignature: () => "",
  });

  poller.register({
    id: "today.live_status",
    kind: "status",
    fetch: async () => {
      await todayFetch.loadLiveStatus(_state.today);
    },
    onAfter: () => {
      todayRender.drawSection(_state.today, "live-status");
    },
    getHeader: () => todayRender.sectionHeader("live-status"),
    getSignature: () => todaySignature("live_status", _state.today),
  });

  // Calendar moves slowly; bucketed under "data" (30s) which is fine.
  poller.register({
    id: "today.calendar",
    kind: "data",
    fetch: async () => {
      await todayFetch.loadCalendar(_state.today);
    },
    onAfter: () => {
      todayRender.drawSection(_state.today, "calendar");
    },
    getHeader: () => todayRender.sectionHeader("calendar"),
    getSignature: () => todaySignature("calendar", _state.today),
  });
}

function todaySignature(kind, state) {
  // Cheap fingerprint per section. Only fields we care about for "did
  // anything change?" — id + status/due_at/total. Stringified so the
  // poller's signature compare is a flat ===.
  try {
    switch (kind) {
      case "commitments": {
        const a = (state.due_today?.commitments || []).map((c) => `${c.id}:${c.status || ""}:${c.due_at || ""}`);
        const b = (state.overdue || []).map((c) => `${c.id}:${c.status || ""}:${c.due_at || ""}`);
        return a.join("|") + "#" + b.join("|");
      }
      case "decisions":
        return (state.decisions_open || []).map((d) => `${d.id}:${d.status || ""}:${d.due_date || ""}`).join("|");
      case "workstreams":
        return (state.workstreams || []).map((w) => `${w.id}:${w.status || ""}:${w.last_touch_at || ""}`).join("|");
      case "receipts_pending":
        return (state.receipts_pending || []).map((r) => `${r.airtable_id}:${r.ticked}/${r.total}`).join("|");
      case "drift":
        return (state.drift || []).map((d) => `${d.severity || ""}:${d.title || ""}`).join("|");
      case "live_status": {
        const ls = state.live_status || {};
        return [
          ls.studio?.version,
          ls.github_actions?.status,
          ls.github_actions?.conclusion,
          ls.contextfor_me?.ok,
          typeof ls.margin?.margin === "number" ? Math.round(ls.margin.margin) : "",
        ].join("|");
      }
      case "calendar":
        return (state.calendar?.today || []).map((e) => `${e.id}:${e.start || ""}`).join("|");
      default:
        return "";
    }
  } catch {
    return "";
  }
}

// ── Per-client view: register live pollers ─────────────────────────────
function registerClientPollers() {
  if (!isTauri) return;
  poller.unregisterPrefix("client.");
  if (!_state.client?.recordId) {
    // Header still loading. Caller re-registers once recordId exists.
    return;
  }

  const sections = [
    ["workstreams", clientFetch.loadWorkstreams, "workstreams"],
    ["decisions", clientFetch.loadDecisions, "decisions"],
    ["commitments", clientFetch.loadCommitments, "commitments"],
    ["projects", clientFetch.loadProjects, "projects"],
    ["receipts", clientFetch.loadReceipts, "receipts"],
    ["meetings", clientFetch.loadMeetings, "meetings"],
    ["emails", clientFetch.loadEmails, "emails"],
    ["drive_files", clientFetch.loadDriveFiles, "drive"],
    ["slack_activity", clientFetch.loadSlackActivity, "slack"],
  ];

  sections.forEach(([slot, loader, sectionName]) => {
    poller.register({
      id: `client.${slot}`,
      kind: "data",
      fetch: async () => loader(_state.client),
      onAfter: () => clientRender.drawSection(_state.client, sectionName),
      getHeader: () => clientRender.sectionHeader(sectionName),
      getSignature: () => clientSignature(slot, _state.client),
    });
  });
}

function clientSignature(slot, state) {
  try {
    const arr = state[slot];
    if (!Array.isArray(arr)) return "";
    return arr.map((r) => `${r.id || r.code || r.name || ""}`).join("|");
  } catch {
    return "";
  }
}

// ── Per-project view: register live poller ────────────────────────────
function registerProjectPollers() {
  if (!isTauri) return;
  poller.unregisterPrefix("project.");
  if (!_state.project?.project_code) return;

  poller.register({
    id: "project.updates",
    kind: "data",
    fetch: async () => {
      await projectFetch.refreshUpdates(_state.project);
    },
    onAfter: () => {
      projectRender.drawUpdatesSection(_state.project);
    },
    getHeader: () => projectRender.sectionHeader("updates"),
    getSignature: () => {
      const u = _state.project?.updates || [];
      return u.map((x) => `${x.kind || ""}:${x.id || x.airtable_id || ""}:${x.created_at || x.timestamp || ""}`).join("|");
    },
  });
}

const STRATEGIC_THINKING = {
  id: "rcpt_2026-05-02_07-45-12",
  project: "in-cahoots-studio",
  workflow: "strategic-thinking",
  title: "RECEIPT — STRATEGIC THINKING SESSION",
  date: "Saturday 02 May 2026",
  sections: [
    {
      items: [
        { qty: "1", text: "Original idea: freelancer pool / collective arm" },
        { qty: "✓", text: "Pushback received and integrated" },
        { qty: "✓", text: "Reference research (Jane Abrahami)" },
        { qty: "✓", text: "Model interrogation" },
        { qty: "1", text: "Structural pivot: bid-together → studio-led with subcontractors" },
        { qty: "✓", text: "Domain repurposed: incahoots.cool" },
        { qty: "1", text: "Acronym landed: COOL" },
        { qty: "1", text: "Acronym expansion: Collective Of Organised Logic" },
        { qty: "✓", text: "Strategic coherence with Context noticed and named" },
        { qty: "1", text: "Member nomenclature: cool people" },
        { qty: "✓", text: "Legal vehicle confirmed: In Cahoots Group Pty Ltd" },
        { qty: "1", text: "Two-tier shape: studio + community" },
        { qty: "✓", text: "Sequencing: post-wedding launch" },
      ],
    },
    {
      header: "BONUS ITEMS (no extra charge)",
      items: [
        { qty: "1", text: "Context rebrand → cntxt" },
        { qty: "1", text: "Domain plan: cntxt.studio" },
        { qty: "✓", text: "Trademark logic clarified" },
        { qty: "1", text: "Product family architecture: studio + collective + tools" },
        {
          type: "task",
          text: "Register cntxt.studio domain",
          done: false,
          on_done: "slack:#all-in-cahoots",
        },
      ],
    },
  ],
  position: {
    header: "POSITION ESTABLISHED",
    quote:
      "In Cahoots provides organised logic for the indie cultural sector — through the studio, through COOL, and through cntxt.",
  },
  totals: [
    { label: "SUBTOTAL", value: "$0.00" },
    { label: "GST", value: "$0.00" },
    { label: "TOTAL", value: "$0.00", grand: true },
  ],
  paid_block: {
    stamp: "PAID IN FULL",
    method: "Saturday morning thinking",
    issued_by: "a thinking partner",
    customer: "Caitlin Reilly",
    status: "fully fkn sick",
  },
  footer_note: "Thank you for your business.",
};

// Tiny helper for DOM construction.
function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === "class") node.className = v;
    else if (k === "style") node.setAttribute("style", v);
    else if (k === "data") {
      for (const [dk, dv] of Object.entries(v)) node.dataset[dk] = dv;
    } else if (k in node) node[k] = v;
    else node.setAttribute(k, v);
  }
  for (const c of children) {
    if (c == null) continue;
    if (typeof c === "string") node.appendChild(document.createTextNode(c));
    else node.appendChild(c);
  }
  return node;
}

function renderItem(item, absoluteIndex) {
  if (item.type === "task") {
    const checkbox = el("input", {
      type: "checkbox",
      checked: item.done ?? false,
      disabled: item.done ?? false,
    });
    const label = el("span", { class: "task-label" }, [item.text]);
    const qty = el("span", { class: "receipt-item-qty" }, [item.qty || "☐"]);
    const row = el(
      "div",
      {
        class: "receipt-item receipt-item-task" + (item.done ? " done" : ""),
      },
      [el("label", {}, [checkbox, label]), qty]
    );
    row.dataset.itemIndex = String(absoluteIndex);
    if (item.on_done) row.dataset.onDone = item.on_done;
    return row;
  }
  return el("div", { class: "receipt-item" }, [
    el("div", { class: "receipt-item-text" }, [item.text]),
    el("div", { class: "receipt-item-qty" }, [item.qty || ""]),
  ]);
}

function renderReceipt(receipt) {
  const root = el("article", { class: "receipt" });
  root.dataset.receiptId = receipt.id;

  // Brand wordmark + entity line (mirrors the typst thermal header).
  const brand = el("div", { class: "receipt-brand" });
  const wordmark = el("img", {
    class: "receipt-wordmark",
    src: "assets/header_serif.png",
    alt: "In Cahoots",
  });
  brand.appendChild(wordmark);
  brand.appendChild(
    el("div", { class: "receipt-entity" }, [
      "In Cahoots Group Pty Ltd · ABN 12687932949 · incahoots.marketing",
    ])
  );
  root.appendChild(brand);
  root.appendChild(el("div", { class: "receipt-divider" }));

  // Doc label + date + reprint action
  root.appendChild(
    el("div", { class: "receipt-header" }, [
      el("div", { class: "receipt-title" }, [receipt.title]),
      el("div", { class: "receipt-meta-cluster" }, [
        el("span", { class: "receipt-meta" }, [receipt.date]),
        el("button", {
          class: "receipt-action-btn",
          "data-action": "reprint",
          title: "Reprint to Munbyn",
        }, ["Reprint"]),
      ]),
    ])
  );

  // Sections — track absolute item index across all sections so the
  // backend tick_item command can address any task by single integer.
  let absoluteIndex = 0;
  receipt.sections.forEach((section, idx) => {
    if (section.header) {
      root.appendChild(
        el("div", { class: "receipt-section-label" }, [section.header])
      );
    }
    section.items.forEach((item) => {
      root.appendChild(renderItem(item, absoluteIndex));
      absoluteIndex += 1;
    });
    if (idx < receipt.sections.length - 1) {
      root.appendChild(el("div", { class: "receipt-divider" }));
    }
  });

  // Position quote
  if (receipt.position) {
    root.appendChild(el("div", { class: "receipt-divider" }));
    root.appendChild(
      el("div", { class: "receipt-section-label" }, [receipt.position.header])
    );
    root.appendChild(
      el("div", { class: "receipt-position" }, [`"${receipt.position.quote}"`])
    );
  }

  // Totals
  if (receipt.totals && receipt.totals.length) {
    root.appendChild(el("div", { class: "receipt-divider" }));
    const totals = el("div", { class: "receipt-totals" });
    receipt.totals.forEach((t) => {
      totals.appendChild(el("div", { class: t.grand ? "total" : "" }, [t.label]));
      totals.appendChild(el("div", { class: t.grand ? "total" : "" }, [t.value]));
    });
    root.appendChild(totals);
  }

  // Paid stamp + meta
  if (receipt.paid_block) {
    root.appendChild(el("div", { class: "receipt-stamp" }, [receipt.paid_block.stamp]));
    const meta = el("div", { class: "receipt-paid-meta" });
    [
      `Payment method: ${receipt.paid_block.method}`,
      `Issued by: ${receipt.paid_block.issued_by}`,
      `Customer: ${receipt.paid_block.customer}`,
      `Status: ${receipt.paid_block.status}`,
    ].forEach((line) => meta.appendChild(el("div", {}, [line])));
    root.appendChild(meta);
  }

  // Footer note
  if (receipt.footer_note) {
    root.appendChild(el("div", { class: "receipt-footer" }, [receipt.footer_note]));
  }

  // Brand sign-off (mirrors typst page-footer): tagline → Oslo mark → entity.
  const signoff = el("div", { class: "receipt-signoff" });
  signoff.appendChild(
    el("div", { class: "receipt-tagline" }, ["pleasure being in cahoots"])
  );
  signoff.appendChild(
    el("img", {
      class: "receipt-oslo",
      src: "assets/oslo.png",
      alt: "in cahoots",
    })
  );
  signoff.appendChild(
    el("div", { class: "receipt-entity-small" }, [
      "In Cahoots Group Pty Ltd · ABN 12687932949 · psst@incahoots.marketing",
    ])
  );
  root.appendChild(signoff);

  return root;
}

// ─── Tauri bridge (works in dev preview by failing soft) ─────────────
const tauri = window.__TAURI__?.core;
const isTauri = !!tauri;

async function invoke(cmd, args) {
  if (!isTauri) {
    throw new Error("Companion backend not available, running in browser preview");
  }
  return tauri.invoke(cmd, args);
}

// ─── API key banner ──────────────────────────────────────────────────
function showApiKeyBanner() {
  if (document.getElementById("api-key-banner")) return;
  const banner = el("div", { id: "api-key-banner", class: "banner banner-warn" }, [
    el("div", { class: "banner-text" }, [
      "Set your Anthropic API key to run live workflows. ",
      el("a", {
        href: "https://console.anthropic.com/settings/keys",
        target: "_blank",
        rel: "noopener",
      }, ["Grab one from console.anthropic.com"]),
      ".",
    ]),
    el("input", {
      id: "api-key-input",
      type: "password",
      placeholder: "sk-ant-...",
      class: "banner-input",
    }),
    el("button", { class: "button banner-button", id: "api-key-save" }, ["Save"]),
  ]);
  document.body.prepend(banner);
  document.getElementById("api-key-save").addEventListener("click", async () => {
    const key = document.getElementById("api-key-input").value.trim();
    if (!key) return showToast("Paste a key first");
    try {
      await invoke("save_api_key", { key });
      banner.remove();
      showToast("API key saved");
    } catch (e) {
      showToast(`Save failed: ${e}`);
    }
  });
}

async function ensureApiKey() {
  if (!isTauri) return false;
  try {
    const has = await invoke("get_api_key_status");
    if (!has) showApiKeyBanner();
    return has;
  } catch {
    return false;
  }
}

// ─── Auto-update ─────────────────────────────────────────────────────
async function checkForUpdates() {
  if (!isTauri) return;
  const updater = window.__TAURI__?.updater;
  if (!updater?.check) return;
  try {
    const update = await updater.check();
    if (update?.available) showUpdateBanner(update);
  } catch (e) {
    console.warn("Update check failed:", e);
  }
}

function showUpdateBanner(update) {
  if (document.getElementById("update-banner")) return;
  const banner = el("div", { id: "update-banner", class: "banner banner-info" }, [
    el("div", { class: "banner-text" }, [
      `Companion ${update.version} is ready. ${update.body || "Click install to update and restart."}`,
    ]),
    el("button", { class: "button banner-button", id: "update-dismiss" }, ["Later"]),
    el("button", { class: "button banner-button", id: "update-install" }, ["Install"]),
  ]);
  document.body.prepend(banner);

  document.getElementById("update-dismiss").addEventListener("click", () => banner.remove());

  document.getElementById("update-install").addEventListener("click", async () => {
    const btn = document.getElementById("update-install");
    btn.disabled = true;
    btn.textContent = "Downloading...";
    try {
      await update.downloadAndInstall();
      // Tauri 2 updater restarts the app on macOS automatically. If it
      // doesn't, the user can quit and relaunch.
    } catch (e) {
      btn.disabled = false;
      btn.textContent = "Install";
      showToast(`Update failed: ${e}`, { ttl: 6000 });
    }
  });
}

// ─── Settings modal ──────────────────────────────────────────────────
async function showSettingsModal() {
  if (document.getElementById("settings-modal")) return;
  const overlay = el("div", { id: "settings-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["Settings"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));

  let apiSet = false;
  let slackSet = false;
  let airtableSet = false;
  let granolaStatus = { connected: false, has_client_id: false, last_pull_at: null };
  let googleStatus = { connected: false, has_client_id: false, last_sync_at: null };
  let slackOauthStatus = { connected: false, has_client_id: false, has_client_secret: false, last_sync_at: null };
  // v0.38: "I am" identity. Defaults to "auto" — read OS user via whoami
  // and map it to a Subcontractor code at runtime.
  let iAmCurrent = "auto";
  let osUser = "";
  if (isTauri) {
    try { apiSet = await invoke("get_api_key_status"); } catch {}
    try { slackSet = await invoke("get_slack_status"); } catch {}
    try { airtableSet = await invoke("get_airtable_status"); } catch {}
    try { granolaStatus = await invoke("get_granola_status"); } catch {}
    try { googleStatus = await invoke("get_google_status"); } catch {}
    try { slackOauthStatus = await invoke("get_slack_oauth_status"); } catch {}
    try { iAmCurrent = await invoke("get_i_am"); } catch {}
    try { osUser = await invoke("whoami_user"); } catch {}
  }

  const body = el("div", { class: "settings-body" });

  // Anthropic API key.
  body.appendChild(el("div", { class: "settings-section" }, [
    el("div", { class: "settings-label" }, ["Anthropic API key"]),
    el("div", { class: "settings-meta" }, [
      apiSet ? "Set in Keychain. Paste again to overwrite." : "Required for Strategic Thinking and other workflows.",
    ]),
    el("div", { class: "settings-row" }, [
      el("input", {
        id: "settings-api-key",
        type: "password",
        placeholder: "sk-ant-...",
        class: "settings-input",
      }),
      el("button", { class: "button", id: "settings-api-save" }, ["Save"]),
    ]),
  ]));

  // Slack webhook.
  body.appendChild(el("div", { class: "settings-section" }, [
    el("div", { class: "settings-label" }, ["Slack webhook URL"]),
    el("div", { class: "settings-meta" }, [
      slackSet
        ? "Set in Keychain. Paste again to overwrite. on_done hooks on receipt items will post to this webhook's channel."
        : "Optional. Set this and any task with on_done='slack:#channel' will post when ticked.",
    ]),
    el("div", { class: "settings-row" }, [
      el("input", {
        id: "settings-slack-url",
        type: "url",
        placeholder: "https://hooks.slack.com/services/...",
        class: "settings-input",
      }),
      el("button", { class: "button", id: "settings-slack-save" }, ["Save"]),
    ]),
  ]));

  // Airtable.
  body.appendChild(el("div", { class: "settings-section" }, [
    el("div", { class: "settings-label" }, ["Airtable"]),
    el("div", { class: "settings-meta" }, [
      airtableSet
        ? "Connected. Sidebar pulls active clients from your base. Paste again to rotate credentials."
        : "Connect to your 'In Cahoots Ops' base. Personal access token + base ID. Companion's sidebar will pull active clients on launch.",
    ]),
    el("div", { class: "settings-row" }, [
      el("input", {
        id: "settings-airtable-key",
        type: "password",
        placeholder: "patXXXXXXXX...  (Personal Access Token)",
        class: "settings-input",
      }),
    ]),
    el("div", { class: "settings-row" }, [
      el("input", {
        id: "settings-airtable-base",
        type: "text",
        placeholder: "appXXXXXXXX  (Base ID)",
        class: "settings-input",
      }),
      el("button", { class: "button", id: "settings-airtable-save" }, ["Save"]),
    ]),
  ]));

  // Granola integration (v0.22 Block C).
  // Two-step setup: first paste the OAuth client ID (issued by
  // Granola's developer console with redirect URI
  // http://localhost:53682/callback registered), then click Connect
  // to run the browser PKCE flow. Status updates without a refresh.
  const granolaMeta = granolaStatus.connected
    ? `Connected. Pull from Granola is live on Monthly Check-in and Quarterly Review.${granolaStatus.last_pull_at ? ` Last pull: ${formatGranolaTimestamp(granolaStatus.last_pull_at)}.` : ""}`
    : granolaStatus.has_client_id
      ? "Client ID saved. Click Connect to authorise in your browser."
      : "Optional. Paste your Granola OAuth client ID (redirect URI: http://localhost:53682/callback), then click Connect.";

  const granolaActions = el("div", { class: "settings-row", style: "margin-top: 8px;" });
  if (granolaStatus.connected) {
    granolaActions.appendChild(
      el("button", { class: "button button-secondary", id: "settings-granola-disconnect" }, ["Disconnect"]),
    );
    granolaActions.appendChild(
      el("button", { class: "button", id: "settings-granola-reconnect" }, ["Re-authorise"]),
    );
  } else {
    granolaActions.appendChild(
      el("button", { class: "button", id: "settings-granola-connect" }, ["Connect Granola"]),
    );
  }

  body.appendChild(el("div", { class: "settings-section" }, [
    el("div", { class: "settings-label" }, ["Granola integration"]),
    el("div", { class: "settings-meta" }, [granolaMeta]),
    el("div", { class: "settings-row" }, [
      el("input", {
        id: "settings-granola-client-id",
        type: "text",
        placeholder: granolaStatus.has_client_id ? "(client ID set — paste again to overwrite)" : "Granola OAuth client ID",
        class: "settings-input",
      }),
      el("button", { class: "button button-secondary", id: "settings-granola-save-id" }, ["Save ID"]),
    ]),
    granolaActions,
  ]));

  // Google integration (v0.25 Block C). Same two-step setup as Granola:
  // paste OAuth client ID issued by Google Cloud Console (Desktop app
  // type, redirect URI http://localhost:53682/callback), then click
  // Connect Google to run the browser PKCE flow. v0.25 covers Calendar,
  // Gmail and Drive in a single OAuth — one client, three scopes. Users
  // upgrading from v0.24 (Calendar only) need to re-authorise once to
  // pick up the Gmail and Drive scopes.
  const googleScopes = Array.isArray(googleStatus.scopes) ? googleStatus.scopes : [];
  const allScopes = ["calendar", "gmail", "drive"];
  const missingScopes = allScopes.filter((s) => !googleScopes.includes(s));
  const scopeList = googleScopes.length ? googleScopes.join(", ") : "none";
  let googleMeta;
  if (googleStatus.connected) {
    const base = `Connected. Scopes: ${scopeList}.`;
    const lastSync = googleStatus.last_sync_at
      ? ` Last sync: ${formatGranolaTimestamp(googleStatus.last_sync_at)}.`
      : "";
    const upgrade = missingScopes.length
      ? ` Re-authorise to add ${missingScopes.join(" + ")}.`
      : "";
    googleMeta = base + lastSync + upgrade;
  } else if (googleStatus.has_client_id) {
    googleMeta = "Client ID saved. Click Connect to authorise Calendar, Gmail and Drive in your browser.";
  } else {
    googleMeta = "Optional. Paste a Google OAuth client ID (Desktop app type, redirect URI http://localhost:53682/callback), then click Connect. Enable the Calendar, Gmail and Drive APIs on the Cloud Console project first.";
  }

  const googleActions = el("div", { class: "settings-row", style: "margin-top: 8px;" });
  if (googleStatus.connected) {
    googleActions.appendChild(
      el("button", { class: "button button-secondary", id: "settings-google-disconnect" }, ["Disconnect"]),
    );
    googleActions.appendChild(
      el("button", { class: "button", id: "settings-google-reconnect" }, ["Re-authorise"]),
    );
  } else {
    googleActions.appendChild(
      el("button", { class: "button", id: "settings-google-connect" }, ["Connect Google"]),
    );
  }

  body.appendChild(el("div", { class: "settings-section" }, [
    el("div", { class: "settings-label" }, ["Google integration"]),
    el("div", { class: "settings-meta" }, [googleMeta]),
    el("div", { class: "settings-row" }, [
      el("input", {
        id: "settings-google-client-id",
        type: "text",
        placeholder: googleStatus.has_client_id ? "(client ID set — paste again to overwrite)" : "Google OAuth client ID",
        class: "settings-input",
      }),
      el("button", { class: "button button-secondary", id: "settings-google-save-id" }, ["Save ID"]),
    ]),
    googleActions,
  ]));

  // Slack read OAuth (v0.26 Block C). Separate from the Slack webhook
  // row above — that's the outbound write path used by receipt-tick
  // on_done hooks. This row authorises a workspace OAuth token that
  // Companion uses for read APIs (channels, history, users.info) so the
  // Today dashboard can show unread counts and the per-client view can
  // show recent messages in #client-{slug}. Two-step setup: paste
  // client ID + secret (Slack v2 OAuth requires both, no PKCE), then
  // click Connect.
  let slackOauthMeta;
  if (slackOauthStatus.connected) {
    const lastSync = slackOauthStatus.last_sync_at
      ? ` Last sync: ${formatGranolaTimestamp(slackOauthStatus.last_sync_at)}.`
      : "";
    slackOauthMeta = `Connected. Slack activity is live on Today and per-client views.${lastSync}`;
  } else if (slackOauthStatus.has_client_id && slackOauthStatus.has_client_secret) {
    slackOauthMeta = "Credentials saved. Click Connect Slack to authorise in your browser.";
  } else if (slackOauthStatus.has_client_id) {
    slackOauthMeta = "Client ID saved. Add the client secret below, then Connect.";
  } else {
    slackOauthMeta = "Optional. Create a Slack app at api.slack.com/apps with redirect URL http://localhost:53682/callback and User Token Scopes: channels:read, channels:history, groups:read, groups:history, users:read. Paste the Client ID + Client Secret below, then Connect.";
  }

  const slackOauthActions = el("div", { class: "settings-row", style: "margin-top: 8px;" });
  if (slackOauthStatus.connected) {
    slackOauthActions.appendChild(
      el("button", { class: "button button-secondary", id: "settings-slack-oauth-disconnect" }, ["Disconnect"]),
    );
    slackOauthActions.appendChild(
      el("button", { class: "button", id: "settings-slack-oauth-reconnect" }, ["Re-authorise"]),
    );
  } else {
    slackOauthActions.appendChild(
      el("button", { class: "button", id: "settings-slack-oauth-connect" }, ["Connect Slack"]),
    );
  }

  body.appendChild(el("div", { class: "settings-section" }, [
    el("div", { class: "settings-label" }, ["Slack integration (read)"]),
    el("div", { class: "settings-meta" }, [slackOauthMeta]),
    el("div", { class: "settings-row" }, [
      el("input", {
        id: "settings-slack-oauth-client-id",
        type: "text",
        placeholder: slackOauthStatus.has_client_id ? "(client ID set — paste again to overwrite)" : "Slack app Client ID",
        class: "settings-input",
      }),
    ]),
    el("div", { class: "settings-row" }, [
      el("input", {
        id: "settings-slack-oauth-client-secret",
        type: "password",
        placeholder: slackOauthStatus.has_client_secret ? "(client secret set — paste again to overwrite)" : "Slack app Client Secret",
        class: "settings-input",
      }),
      el("button", { class: "button button-secondary", id: "settings-slack-oauth-save-id" }, ["Save"]),
    ]),
    slackOauthActions,
  ]));

  // "I am" identity (v0.38). Companion has no user concept yet, but Rose
  // is about to start. We default to OS-user mapping (`whoami` →
  // caitlinreilly = Caitlin, rose = Rose, anything else = Other), and
  // the dropdown lets either of them override on a shared machine.
  const iAmOptions = [
    ["auto", `Auto (OS user: ${osUser || "unknown"})`],
    ["caitlin", "Caitlin"],
    ["rose", "Rose"],
    ["other", "Other"],
  ];
  const iAmSelect = el("select", { id: "settings-i-am", class: "settings-input" });
  iAmOptions.forEach(([value, label]) => {
    const opt = document.createElement("option");
    opt.value = value;
    opt.textContent = label;
    if (value === iAmCurrent) opt.selected = true;
    iAmSelect.appendChild(opt);
  });
  body.appendChild(el("div", { class: "settings-section" }, [
    el("div", { class: "settings-label" }, ["I am"]),
    el("div", { class: "settings-meta" }, [
      "Tells Companion who's logged in so receipts get tagged with the right subcontractor and the per-Subcontractor view defaults to your row. Auto reads the macOS user.",
    ]),
    el("div", { class: "settings-row" }, [
      iAmSelect,
      el("button", { class: "button", id: "settings-i-am-save" }, ["Save"]),
    ]),
  ]));

  // Forms (v0.30 Block F). Six Airtable Forms Caitlin builds once in
  // the Airtable web UI (the API doesn't expose form creation). She
  // pastes each URL here; Companion stores them in CompanionSettings
  // and surfaces them as "Send Form" buttons on the right pages.
  let formsList = [];
  if (isTauri && airtableSet) {
    try {
      const urls = await forms.loadForms();
      formsList = forms.FORM_KEYS.map((key) => ({
        key,
        meta: forms.FORM_META[key],
        value: urls.get(key)?.value || "",
        updated_at: urls.get(key)?.updated_at || null,
      }));
    } catch (e) {
      console.warn("forms.loadForms in Settings failed:", e);
    }
  }

  const formsRows = el("div", { class: "settings-forms-list" });
  if (!isTauri) {
    formsRows.appendChild(
      el("div", { class: "settings-meta" }, [
        "Open the Companion app to manage form URLs.",
      ])
    );
  } else if (!airtableSet) {
    formsRows.appendChild(
      el("div", { class: "settings-meta" }, [
        "Connect Airtable above first. Form URLs live in the CompanionSettings table.",
      ])
    );
  } else if (!formsList.length) {
    formsRows.appendChild(
      el("div", { class: "settings-meta" }, [
        "Couldn't reach Airtable. Try again once the connection is healthy.",
      ])
    );
  } else {
    formsList.forEach((row) => {
      const inputId = `settings-form-${row.key}`;
      const meta = row.meta || { label: row.key, blurb: "" };
      const labelLine = meta.label;
      const blurbLine = meta.blurb;
      const updatedLine = row.updated_at
        ? `Updated ${formatGranolaTimestamp(row.updated_at)}`
        : "Not set yet.";
      const rowEl = el("div", { class: "settings-form-row" }, [
        el("div", { class: "settings-form-row-head" }, [
          el("div", { class: "settings-form-row-label" }, [labelLine]),
          el("div", { class: "settings-form-row-meta" }, [updatedLine]),
        ]),
        el("div", { class: "settings-form-row-blurb" }, [blurbLine]),
        el("div", { class: "settings-row" }, [
          el("input", {
            id: inputId,
            type: "url",
            placeholder: row.value ? "(URL set — paste again to overwrite)" : "https://airtable.com/app.../shr...",
            class: "settings-input",
          }),
          el("button", {
            class: "button button-secondary",
            "data-form-action": "save",
            "data-form-key": row.key,
          }, ["Save"]),
          el("button", {
            class: "button button-secondary",
            "data-form-action": "open",
            "data-form-key": row.key,
            disabled: row.value ? null : "disabled",
          }, ["Open"]),
          el("button", {
            class: "button button-secondary",
            "data-form-action": "copy",
            "data-form-key": row.key,
            disabled: row.value ? null : "disabled",
          }, ["Copy"]),
        ]),
      ]);
      formsRows.appendChild(rowEl);
    });
  }

  body.appendChild(el("div", { class: "settings-section" }, [
    el("div", { class: "settings-label" }, ["Forms"]),
    el("div", { class: "settings-meta" }, [
      "Build each form once in Airtable's web UI, then paste its share URL here. See forms-setup-checklist.md in the Companion folder for click-by-click steps.",
    ]),
    formsRows,
  ]));

  // v0.40 — refresh cadence preset. Stored in localStorage so the
  // setting takes effect instantly without an Airtable round-trip on
  // every change.
  const cadenceCurrent = readCadencePreference();
  const cadenceRow = el("div", { class: "settings-row", style: "gap: 8px;" });
  Object.entries(CADENCE_PRESETS).forEach(([key, preset]) => {
    const btn = el("button", {
      class: key === cadenceCurrent ? "button" : "button button-secondary",
      type: "button",
      "data-cadence-preset": key,
    }, [preset.label]);
    btn.addEventListener("click", () => {
      writeCadencePreference(key);
      poller.applyPreset(key);
      cadenceRow.querySelectorAll("[data-cadence-preset]").forEach((b) => {
        b.className = b.dataset.cadencePreset === key
          ? "button"
          : "button button-secondary";
      });
      const meta = key === "live"
        ? "Live: data 30s, status 10s."
        : key === "battery_saver"
        ? "Battery saver: data 2 min, status 30s."
        : "Off: refresh only when the window comes back into focus.";
      cadenceMeta.textContent = meta;
      showToast(`Refresh cadence: ${preset.label}`);
    });
    cadenceRow.appendChild(btn);
  });
  const cadenceMeta = el("div", { class: "settings-meta" }, [
    cadenceCurrent === "live"
      ? "Live: data 30s, status 10s."
      : cadenceCurrent === "battery_saver"
      ? "Battery saver: data 2 min, status 30s."
      : "Off: refresh only when the window comes back into focus.",
  ]);
  body.appendChild(el("div", { class: "settings-section" }, [
    el("div", { class: "settings-label" }, ["Refresh cadence"]),
    cadenceMeta,
    cadenceRow,
  ]));

  modal.appendChild(body);
  modal.appendChild(el("div", { class: "modal-actions" }, [
    el("button", { class: "button button-secondary", id: "settings-close" }, ["Done"]),
  ]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  document.getElementById("settings-close").addEventListener("click", close);

  document.getElementById("settings-api-save").addEventListener("click", async () => {
    const key = document.getElementById("settings-api-key").value.trim();
    if (!key) return showToast("Paste a key first");
    try {
      await invoke("save_api_key", { key });
      showToast("API key saved");
      document.getElementById("settings-api-key").value = "";
    } catch (e) { showToast(`Save failed: ${e}`); }
  });

  document.getElementById("settings-slack-save").addEventListener("click", async () => {
    const url = document.getElementById("settings-slack-url").value.trim();
    if (!url) return showToast("Paste a URL first");
    try {
      await invoke("save_slack_webhook", { url });
      showToast("Slack webhook saved");
      document.getElementById("settings-slack-url").value = "";
    } catch (e) { showToast(`Save failed: ${e}`); }
  });

  document.getElementById("settings-airtable-save").addEventListener("click", async () => {
    const apiKey = document.getElementById("settings-airtable-key").value.trim();
    const baseId = document.getElementById("settings-airtable-base").value.trim();
    if (!apiKey || !baseId) return showToast("Both fields required");
    try {
      await invoke("save_airtable_credentials", { apiKey, baseId });
      showToast("Airtable connected");
      document.getElementById("settings-airtable-key").value = "";
      document.getElementById("settings-airtable-base").value = "";
      // Refresh the sidebar so the new client list shows up.
      loadStudioSidebar();
    } catch (e) { showToast(`Save failed: ${e}`); }
  });

  document.getElementById("settings-i-am-save")?.addEventListener("click", async () => {
    const value = document.getElementById("settings-i-am").value;
    if (!isTauri) {
      showToast("Open the Companion app to save settings.");
      return;
    }
    try {
      await invoke("save_i_am", { value });
      showToast("Identity saved");
    } catch (e) { showToast(`Save failed: ${e}`); }
  });

  // Granola handlers. Re-render the modal in place after state changes
  // so the connect/disconnect button swap and the meta text update
  // without requiring a manual reopen.
  const reopenSettings = () => { close(); showSettingsModal(); };

  document.getElementById("settings-granola-save-id").addEventListener("click", async () => {
    const clientId = document.getElementById("settings-granola-client-id").value.trim();
    if (!clientId) return showToast("Paste a client ID first");
    try {
      await invoke("save_granola_client_id", { clientId });
      showToast("Granola client ID saved");
      reopenSettings();
    } catch (e) { showToast(`Save failed: ${e}`); }
  });

  const connectBtn = document.getElementById("settings-granola-connect");
  const reconnectBtn = document.getElementById("settings-granola-reconnect");
  const runConnect = async (btn) => {
    if (!btn) return;
    const original = btn.textContent;
    btn.disabled = true;
    btn.textContent = "Authorising...";
    try {
      await invoke("connect_granola");
      showToast("Granola connected");
      reopenSettings();
    } catch (e) {
      btn.disabled = false;
      btn.textContent = original;
      showToast(`Connect failed: ${e.message || e}`, { ttl: 7000 });
    }
  };
  if (connectBtn) connectBtn.addEventListener("click", () => runConnect(connectBtn));
  if (reconnectBtn) reconnectBtn.addEventListener("click", () => runConnect(reconnectBtn));

  const disconnectBtn = document.getElementById("settings-granola-disconnect");
  if (disconnectBtn) {
    disconnectBtn.addEventListener("click", async () => {
      try {
        await invoke("disconnect_granola");
        showToast("Granola disconnected");
        reopenSettings();
      } catch (e) { showToast(`Disconnect failed: ${e}`); }
    });
  }

  // Google handlers (v0.24). Same shape as Granola — save client ID,
  // run the OAuth flow, or disconnect. Re-renders the modal in place
  // so the connect/disconnect button swap and the meta line update
  // without requiring a manual reopen.
  document.getElementById("settings-google-save-id").addEventListener("click", async () => {
    const clientId = document.getElementById("settings-google-client-id").value.trim();
    if (!clientId) return showToast("Paste a client ID first");
    try {
      await invoke("save_google_client_id", { clientId });
      showToast("Google client ID saved");
      reopenSettings();
    } catch (e) { showToast(`Save failed: ${e}`); }
  });

  const googleConnectBtn = document.getElementById("settings-google-connect");
  const googleReconnectBtn = document.getElementById("settings-google-reconnect");
  const runGoogleConnect = async (btn) => {
    if (!btn) return;
    const original = btn.textContent;
    btn.disabled = true;
    btn.textContent = "Authorising...";
    try {
      await invoke("connect_google");
      showToast("Google connected");
      reopenSettings();
    } catch (e) {
      btn.disabled = false;
      btn.textContent = original;
      showToast(`Connect failed: ${e.message || e}`, { ttl: 7000 });
    }
  };
  if (googleConnectBtn) googleConnectBtn.addEventListener("click", () => runGoogleConnect(googleConnectBtn));
  if (googleReconnectBtn) googleReconnectBtn.addEventListener("click", () => runGoogleConnect(googleReconnectBtn));

  const googleDisconnectBtn = document.getElementById("settings-google-disconnect");
  if (googleDisconnectBtn) {
    googleDisconnectBtn.addEventListener("click", async () => {
      try {
        await invoke("disconnect_google");
        showToast("Google disconnected");
        reopenSettings();
      } catch (e) { showToast(`Disconnect failed: ${e}`); }
    });
  }

  // Slack OAuth handlers (v0.26). Same modal-reopen pattern as Granola
  // and Google so the connect/disconnect button swap is reflected
  // without requiring a manual reopen.
  document.getElementById("settings-slack-oauth-save-id").addEventListener("click", async () => {
    const clientId = document.getElementById("settings-slack-oauth-client-id").value.trim();
    const clientSecret = document.getElementById("settings-slack-oauth-client-secret").value.trim();
    if (!clientId || !clientSecret) {
      return showToast("Both client ID and secret required");
    }
    try {
      await invoke("save_slack_oauth_credentials", { clientId, clientSecret });
      showToast("Slack credentials saved");
      reopenSettings();
    } catch (e) { showToast(`Save failed: ${e}`); }
  });

  const slackConnectBtn = document.getElementById("settings-slack-oauth-connect");
  const slackReconnectBtn = document.getElementById("settings-slack-oauth-reconnect");
  const runSlackConnect = async (btn) => {
    if (!btn) return;
    const original = btn.textContent;
    btn.disabled = true;
    btn.textContent = "Authorising...";
    try {
      await invoke("connect_slack");
      showToast("Slack connected");
      reopenSettings();
    } catch (e) {
      btn.disabled = false;
      btn.textContent = original;
      showToast(`Connect failed: ${e.message || e}`, { ttl: 7000 });
    }
  };
  if (slackConnectBtn) slackConnectBtn.addEventListener("click", () => runSlackConnect(slackConnectBtn));
  if (slackReconnectBtn) slackReconnectBtn.addEventListener("click", () => runSlackConnect(slackReconnectBtn));

  const slackDisconnectBtn = document.getElementById("settings-slack-oauth-disconnect");
  if (slackDisconnectBtn) {
    slackDisconnectBtn.addEventListener("click", async () => {
      try {
        await invoke("disconnect_slack");
        showToast("Slack disconnected");
        reopenSettings();
      } catch (e) { showToast(`Disconnect failed: ${e}`); }
    });
  }

  // Forms section button handlers (v0.30). Delegated click — each row's
  // Save / Open / Copy carries data-form-action and data-form-key.
  body.addEventListener("click", async (e) => {
    const btn = e.target.closest("button[data-form-action]");
    if (!btn) return;
    const action = btn.dataset.formAction;
    const key = btn.dataset.formKey;
    if (!key) return;
    if (action === "save") {
      const input = document.getElementById(`settings-form-${key}`);
      const value = (input?.value || "").trim();
      try {
        await forms.setFormUrl(key, value);
        showToast(value ? "Form URL saved" : "Form URL cleared");
        reopenSettings();
      } catch (err) {
        showToast(`Save failed: ${err}`, { ttl: 5000 });
      }
      return;
    }
    if (action === "open") {
      try {
        const url = await forms.getFormUrl(key);
        if (!url) return showToast("No URL set for this form");
        forms.openUrl(url);
      } catch (err) { showToast(`Open failed: ${err}`); }
      return;
    }
    if (action === "copy") {
      try {
        const url = await forms.getFormUrl(key);
        if (!url) return showToast("No URL set for this form");
        await forms.copyToClipboard(url);
        showToast("Copied to clipboard");
      } catch (err) { showToast(`Copy failed: ${err}`); }
    }
  });
}

// Render an RFC3339 timestamp as "Mon 5 May, 2:14pm" for the Granola
// last-pull indicator. Falls back to the raw string on parse failure.
function formatGranolaTimestamp(rfc3339) {
  try {
    const d = new Date(rfc3339);
    if (Number.isNaN(d.getTime())) return rfc3339;
    return d.toLocaleString(undefined, {
      weekday: "short",
      day: "numeric",
      month: "short",
      hour: "numeric",
      minute: "2-digit",
    });
  } catch (_) {
    return rfc3339;
  }
}

// ─── Sidebar dynamic load (Airtable Clients) ─────────────────────────
//
// Delegates to client/sidebar.js which handles the 60s in-memory cache.
// Called on app launch, after Airtable creds are saved, and after a new
// client is created via the New Client Onboarding receipt.
async function loadStudioSidebar() {
  clientSidebar.clearCache();
  await clientSidebar.loadClients();
}

// ─── Strategic Thinking modal ────────────────────────────────────────
// Accepts an optional client_code for parity with the other workflow
// modals (per-client workflow grid passes it). Strategic Thinking is
// not client-scoped so the value is ignored.
async function showStrategicThinkingModal(_prefillClientCode) {
  if (document.getElementById("st-modal")) return;
  const overlay = el("div", { id: "st-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["Strategic Thinking"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Drop in what you're thinking through. Claude reads it, asks back if needed, and returns a receipt with the decisions, ticks, and follow-up tasks.",
  ]));

  // v0.32: add a source-picker so the seed brief can come from Granola
  // / Gmail / Slack / Calendar / Form / Manual paste. Existing manual
  // paste (the topic textarea) still works — picker is additive.
  const available = await probeSourceAvailability();
  const pickerState = createPickerState({ clientCode: null });
  const picker = mountSourcePicker({
    container: modal,
    state: pickerState,
    available,
    label: "Optional source",
  });

  const topicPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["What you're thinking through"]),
  ]);
  const textarea = el("textarea", {
    class: "modal-textarea",
    placeholder: "I'm thinking about whether to launch COOL before the wedding or after, given the post-wedding sequencing decision yesterday...",
    rows: 8,
  });
  topicPad.appendChild(textarea);
  modal.appendChild(topicPad);
  modal.appendChild(el("div", { class: "modal-actions" }, [
    el("button", { class: "button button-secondary", id: "st-cancel" }, ["Cancel"]),
    el("button", { class: "button", id: "st-run" }, ["Run"]),
  ]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  textarea.focus();

  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) close();
  });
  modal.querySelector(".modal-close").addEventListener("click", close);
  document.getElementById("st-cancel").addEventListener("click", close);
  document.getElementById("st-run").addEventListener("click", async () => {
    const userInput = textarea.value.trim();
    if (!userInput) return showToast("Type something first");
    const runBtn = document.getElementById("st-run");
    runBtn.disabled = true;
    runBtn.textContent = "Pulling source...";
    const blob = await picker.ensureContextBlob();
    runBtn.textContent = "Thinking...";
    // Strategic thinking takes a single string — prepend the source blob
    // when present so the model has the original context.
    const composed = blob
      ? `${blob}\n\n---\n\n${userInput}`
      : userInput;
    try {
      const json = await invoke("run_strategic_thinking", { input: composed });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Receipt ready");
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// ─── airtable:create-client handler ──────────────────────────────────
//
// When a New Client Onboarding receipt's "Create client + project rows
// in Airtable" task is ticked, prompt for the canonical 3-letter code
// (Claude can suggest one in the receipt's project field but the human
// confirms) and call the backend to file it.
async function handleCreateClientFromReceipt(receiptEl) {
  const receiptId = receiptEl?.dataset?.receiptId;
  if (!receiptId) return showToast("No receipt id");

  // Pull receipt JSON back from backend so we get the latest copy.
  let receipt;
  try {
    const rows = await invoke("list_receipts", { limit: 100 });
    const match = rows.map((j) => JSON.parse(j)).find((r) => r.id === receiptId);
    if (!match) return showToast("Couldn't find that receipt to read its details");
    receipt = match;
  } catch (e) {
    return showToast(`Receipt lookup failed: ${e}`);
  }

  const suggestedCode = (receipt.project || "").toUpperCase().slice(0, 4);
  const clientName = receipt.paid_block?.customer || "";

  const code = window.prompt(
    `Confirm or enter the canonical 3-letter code for "${clientName}":`,
    suggestedCode
  );
  if (!code) return showToast("Cancelled — no client created");

  const status = "active";
  const notes = `Onboarded via Companion receipt ${receiptId}`;

  try {
    const recordId = await invoke("create_airtable_client", {
      args: {
        code: code.trim().toUpperCase(),
        name: clientName,
        status,
        primary_contact_email: null,
        notes,
      },
    });
    showToast(`${code.toUpperCase()} created in Airtable (${recordId})`, { ttl: 4000 });
    // Refresh the sidebar so the new client appears.
    loadStudioSidebar();
  } catch (e) {
    showToast(`Airtable create failed: ${e}`, { ttl: 6000 });
  }
}

// ─── New Client Onboarding modal ─────────────────────────────────────
// Accepts an optional client_code for parity with other workflow modals.
// Onboarding *creates* the client row, so the value is ignored.
function showNewClientOnboardingModal(_prefillClientCode) {
  if (document.getElementById("nco-modal")) return;
  const overlay = el("div", { id: "nco-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["New Client Onboarding"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Drop the first-call notes in. Claude returns discovery questions, a draft scope, follow-up tasks, and a first-WIP agenda. Ticking the 'create client + project rows' task files them to Airtable.",
  ]));

  // Two-column field grid for the structured inputs.
  const grid = el("div", { class: "modal-field-grid" });

  const clientName = el("input", { id: "nco-client-name", type: "text", class: "settings-input", placeholder: "e.g. Northcote Theatre" });
  const contactEmail = el("input", { id: "nco-contact-email", type: "email", class: "settings-input", placeholder: "name@example.com" });
  const projectType = el("select", { id: "nco-project-type", class: "settings-input" });
  ["festival", "venue / show", "touring", "label / artist", "PR", "comedy", "capability", "other"].forEach((t) => {
    const opt = document.createElement("option");
    opt.value = t;
    opt.textContent = t;
    projectType.appendChild(opt);
  });
  const budget = el("input", { id: "nco-budget", type: "text", class: "settings-input", placeholder: "e.g. ~10k, unsure, post-grant" });
  const timeline = el("input", { id: "nco-timeline", type: "text", class: "settings-input", placeholder: "e.g. on-sale July, flexible, urgent" });

  [
    ["Client name", clientName],
    ["Contact email", contactEmail],
    ["Project type", projectType],
    ["Budget signal", budget],
    ["Timeline signal", timeline],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });

  modal.appendChild(grid);

  const notesPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["First-call notes"]),
  ]);
  const notes = el("textarea", {
    id: "nco-notes",
    class: "modal-textarea",
    placeholder: "What they want, what's been tried, who else is involved, what success looks like, what they're worried about. Notes from your discovery call go here verbatim.",
    rows: 8,
  });
  notesPad.appendChild(notes);
  modal.appendChild(notesPad);

  modal.appendChild(el("div", { class: "modal-actions" }, [
    el("button", { class: "button button-secondary", id: "nco-cancel" }, ["Cancel"]),
    el("button", { class: "button", id: "nco-run" }, ["Run"]),
  ]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  clientName.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  document.getElementById("nco-cancel").addEventListener("click", close);

  document.getElementById("nco-run").addEventListener("click", async () => {
    const input = {
      client_name: clientName.value.trim(),
      contact_email: contactEmail.value.trim() || null,
      project_type: projectType.value,
      budget_signal: budget.value.trim() || null,
      timeline_signal: timeline.value.trim() || null,
      first_call_notes: notes.value.trim(),
    };
    if (!input.client_name) return showToast("Client name required");
    if (!input.first_call_notes) return showToast("First-call notes required");

    const runBtn = document.getElementById("nco-run");
    runBtn.disabled = true;
    runBtn.textContent = "Thinking...";
    try {
      const json = await invoke("run_new_client_onboarding", { input });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Onboarding receipt ready");
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// ─── New Campaign Scope modal ────────────────────────────────────────
async function showNewCampaignScopeModal(prefillClientCode) {
  if (document.getElementById("ncs-modal")) return;

  // Pull active clients for the picker.
  let clients = [];
  if (isTauri) {
    try {
      const raw = await invoke("list_airtable_clients");
      const data = JSON.parse(raw);
      clients = (data.records || []).map((r) => ({
        code: r.fields?.code || "",
        name: r.fields?.name || r.fields?.code || "Untitled",
      })).filter((c) => c.code);
    } catch (e) {
      console.warn("list_airtable_clients failed:", e);
    }
  }

  if (clients.length === 0) {
    showToast("No clients in Airtable yet. Add one via New Client Onboarding first.", { ttl: 6000 });
    return;
  }

  const overlay = el("div", { id: "ncs-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["New Campaign Scope"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Scope a discrete campaign for an existing client. Claude returns deliverables, timeline, reporting rhythm, assumptions, and follow-ups. Project code is auto-built from {CLIENT}-{YYYY}-{MM}-{slug}.",
  ]));

  const grid = el("div", { class: "modal-field-grid" });

  const clientPicker = el("select", { id: "ncs-client", class: "settings-input" });
  clients.forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c.code;
    opt.textContent = `${c.code} — ${c.name}`;
    clientPicker.appendChild(opt);
  });
  if (prefillClientCode && clients.some((c) => c.code === prefillClientCode)) {
    clientPicker.value = prefillClientCode;
  }

  const campaignName = el("input", { id: "ncs-name", type: "text", class: "settings-input", placeholder: "e.g. June tour launch" });
  const projectSlug = el("input", { id: "ncs-slug", type: "text", class: "settings-input", placeholder: "auto from name (e.g. tour-launch)" });
  const campaignType = el("select", { id: "ncs-type", class: "settings-input" });
  ["festival", "venue", "touring", "label", "artist", "PR", "comedy", "capability"].forEach((t) => {
    const opt = document.createElement("option");
    opt.value = t;
    opt.textContent = t;
    campaignType.appendChild(opt);
  });
  const startDate = el("input", { id: "ncs-start", type: "date", class: "settings-input" });
  const endDate = el("input", { id: "ncs-end", type: "date", class: "settings-input" });
  const budget = el("input", { id: "ncs-budget", type: "text", class: "settings-input", placeholder: "e.g. $8k, ~15k, TBC" });

  [
    ["Client", clientPicker],
    ["Campaign type", campaignType],
    ["Campaign name", campaignName],
    ["Slug (optional)", projectSlug],
    ["Start date", startDate],
    ["End date (optional)", endDate],
    ["Budget signal", budget],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });

  modal.appendChild(grid);

  const notesPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Brief notes"]),
  ]);
  const notes = el("textarea", {
    id: "ncs-notes",
    class: "modal-textarea",
    placeholder: "What the campaign is, what success looks like, what you've already discussed with the client, what you're worried about. Notes from your scoping call go here verbatim.",
    rows: 8,
  });
  notesPad.appendChild(notes);
  modal.appendChild(notesPad);

  modal.appendChild(el("div", { class: "modal-actions" }, [
    el("button", { class: "button button-secondary", id: "ncs-cancel" }, ["Cancel"]),
    el("button", { class: "button", id: "ncs-run" }, ["Run"]),
  ]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  campaignName.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  document.getElementById("ncs-cancel").addEventListener("click", close);

  document.getElementById("ncs-run").addEventListener("click", async () => {
    const input = {
      client_code: clientPicker.value,
      campaign_name: campaignName.value.trim(),
      project_slug: projectSlug.value.trim(),
      campaign_type: campaignType.value,
      start_date: startDate.value,
      end_date: endDate.value || null,
      budget_signal: budget.value.trim() || null,
      brief_notes: notes.value.trim(),
    };
    if (!input.campaign_name) return showToast("Campaign name required");
    if (!input.start_date) return showToast("Start date required");
    if (!input.brief_notes) return showToast("Brief notes required");

    const runBtn = document.getElementById("ncs-run");
    runBtn.disabled = true;
    runBtn.textContent = "Drafting scope...";
    try {
      const json = await invoke("run_new_campaign_scope", { input });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Scope draft ready");
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// ─── airtable:create-project handler ─────────────────────────────────
async function handleCreateProjectFromReceipt(receiptEl) {
  const receiptId = receiptEl?.dataset?.receiptId;
  if (!receiptId) return showToast("No receipt id");

  let receipt;
  try {
    const rows = await invoke("list_receipts", { limit: 100 });
    const match = rows.map((j) => JSON.parse(j)).find((r) => r.id === receiptId);
    if (!match) return showToast("Couldn't find that receipt");
    receipt = match;
  } catch (e) {
    return showToast(`Receipt lookup failed: ${e}`);
  }

  const projectCode = receipt.project || "";
  const clientCode = (projectCode.split("-")[0] || "").toUpperCase();

  const confirmCode = window.prompt("Confirm project code:", projectCode);
  if (!confirmCode) return showToast("Cancelled — no project created");

  // Pull campaign_name from the customer field of the receipt's paid_block,
  // fall back to the title.
  const projectName = receipt.paid_block?.customer || receipt.title || projectCode;

  try {
    const recordId = await invoke("create_airtable_project", {
      args: {
        code: confirmCode.trim(),
        name: projectName,
        client_code: clientCode,
        campaign_type: null,
        start_date: null,
        end_date: null,
        budget_total: null,
        notes: `Filed via Companion receipt ${receiptId}`,
      },
    });
    showToast(`${confirmCode} created in Airtable Projects (${recordId})`, { ttl: 4000 });
  } catch (e) {
    showToast(`Airtable create failed: ${e}`, { ttl: 6000 });
  }
}

// ─── Subcontractor Onboarding modal ──────────────────────────────────
// Accepts an optional client_code for parity with other workflow modals.
// Subcontractor onboarding isn't client-scoped (subs work across clients)
// so the value is ignored.
function showSubcontractorOnboardingModal(_prefillClientCode) {
  if (document.getElementById("sub-modal")) return;
  const overlay = el("div", { id: "sub-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["Subcontractor Onboarding"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Bring a new subcontractor onto the studio. Returns the onboarding pack: pre-start info request, role description draft, week-1 schedule, docs to share, first-WIP agenda, action items.",
  ]));

  const grid = el("div", { class: "modal-field-grid" });

  const name = el("input", { type: "text", class: "settings-input", placeholder: "e.g. Rose Gaumann" });
  const role = el("input", { type: "text", class: "settings-input", placeholder: "e.g. Marketing Coordinator" });
  const startDate = el("input", { type: "date", class: "settings-input" });
  const hourlyRate = el("input", { type: "number", step: "0.01", min: "0", class: "settings-input", placeholder: "e.g. 50.00" });
  const email = el("input", { type: "email", class: "settings-input", placeholder: "name@example.com" });

  [
    ["Name", name],
    ["Role", role],
    ["Start date", startDate],
    ["Hourly rate ($AUD)", hourlyRate],
    ["Email", email],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });

  modal.appendChild(grid);

  const notesPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Notes"]),
  ]);
  const notes = el("textarea", {
    class: "modal-textarea",
    placeholder: "Why they're joining, what they're best at, what they're new to, what hours/availability looks like, any context on previous work or interview impressions.",
    rows: 6,
  });
  notesPad.appendChild(notes);
  modal.appendChild(notesPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  name.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    const input = {
      name: name.value.trim(),
      role: role.value.trim(),
      start_date: startDate.value || null,
      hourly_rate: hourlyRate.value ? parseFloat(hourlyRate.value) : null,
      email: email.value.trim() || null,
      notes: notes.value.trim(),
    };
    if (!input.name) return showToast("Name required");
    if (!input.role) return showToast("Role required");
    if (!input.notes) return showToast("Notes required");

    runBtn.disabled = true;
    runBtn.textContent = "Drafting onboarding...";
    try {
      const json = await invoke("run_subcontractor_onboarding", { input });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Onboarding pack ready");
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

async function handleCreateSubcontractorFromReceipt(receiptEl) {
  const receiptId = receiptEl?.dataset?.receiptId;
  if (!receiptId) return showToast("No receipt id");

  let receipt;
  try {
    const rows = await invoke("list_receipts", { limit: 100 });
    const match = rows.map((j) => JSON.parse(j)).find((r) => r.id === receiptId);
    if (!match) return showToast("Couldn't find that receipt");
    receipt = match;
  } catch (e) {
    return showToast(`Receipt lookup failed: ${e}`);
  }

  const subName = receipt.paid_block?.customer || "";
  const initials = subName.split(" ").map((p) => p[0] || "").join("").toUpperCase().slice(0, 3);
  const suggestedCode = initials ? `${initials}-S` : "";

  const code = window.prompt(
    `Confirm or enter the canonical code for "${subName}" (e.g. ROS-S):`,
    suggestedCode
  );
  if (!code) return showToast("Cancelled — no subcontractor created");

  try {
    const recordId = await invoke("create_airtable_subcontractor", {
      args: {
        code: code.trim().toUpperCase(),
        name: subName,
        role: null,
        start_date: null,
        hourly_rate: null,
        email: null,
        notes: `Filed via Companion receipt ${receiptId}`,
      },
    });
    showToast(`${code.toUpperCase()} created in Subcontractors (${recordId})`, { ttl: 4000 });
  } catch (e) {
    showToast(`Airtable create failed: ${e}`, { ttl: 6000 });
  }
}

// ─── Generic client-picker review modal ──────────────────────────────
//
// Used by Monthly Check-in and Quarterly Review. Both have identical UX
// shape: pick a client, optionally add flags, run a workflow that pulls
// recent receipts as context.
async function showClientPickerReviewModal({
  modalId,
  title,
  meta,
  command,
  runningLabel,
  successToast,
  flagsPlaceholder,
  prefillClientCode,
  granolaWindowDays, // optional: enables the "Pull from Granola" button on this modal
}) {
  if (document.getElementById(modalId)) return;

  let clients = [];
  if (isTauri) {
    try {
      const raw = await invoke("list_airtable_clients");
      const data = JSON.parse(raw);
      clients = (data.records || []).map((r) => ({
        code: r.fields?.code || "",
        name: r.fields?.name || r.fields?.code || "Untitled",
      })).filter((c) => c.code);
    } catch (e) {
      console.warn("list_airtable_clients failed:", e);
    }
  }

  if (clients.length === 0) {
    showToast("No clients in Airtable yet. Add one via New Client Onboarding first.", { ttl: 6000 });
    return;
  }

  const overlay = el("div", { id: modalId, class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, [title]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [meta]));

  const grid = el("div", { class: "modal-field-grid" });
  const clientPicker = el("select", { class: "settings-input" });
  clients.forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c.code;
    opt.textContent = `${c.code} — ${c.name}`;
    clientPicker.appendChild(opt);
  });
  if (prefillClientCode && clients.some((c) => c.code === prefillClientCode)) {
    clientPicker.value = prefillClientCode;
  }
  grid.appendChild(el("label", { class: "modal-field" }, [
    el("div", { class: "settings-label" }, ["Client"]),
    clientPicker,
  ]));
  modal.appendChild(grid);

  // v0.32: source-picker — broader source palette than Granola alone.
  // The existing "Pull from Granola" button below stays — this is
  // additive. The picker hides Granola when not connected, so users
  // never see a duplicate path on the same modal.
  const available = await probeSourceAvailability();
  const pickerState = createPickerState({ clientCode: prefillClientCode || null });
  // Hide Granola from the picker since the existing pull button below
  // already handles the multi-day Granola window. Keeps the UX from
  // looking like two ways to do the same thing.
  const pickerAvailable = { ...available, granola: false };
  const picker = mountSourcePicker({
    container: modal,
    state: pickerState,
    available: pickerAvailable,
    label: "Extra source (optional)",
  });

  const flagsPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Anything to flag (optional)"]),
  ]);
  const flags = el("textarea", {
    class: "modal-textarea",
    placeholder: flagsPlaceholder,
    rows: 5,
  });
  flagsPad.appendChild(flags);
  modal.appendChild(flagsPad);

  // ── Granola transcript pull (v0.22) ─────────────────────────────────
  // Only rendered when granolaWindowDays is set (Monthly Check-in,
  // Quarterly Review). The "Pull from Granola" button calls the Tauri
  // command, gets back a plain-text transcript bundle, and appends it
  // to the textarea below. Manual paste also works — the textarea is
  // a normal editable field. The fallback is intentional, even with
  // Granola wired: useful for offline, or for transcripts that aren't
  // in Granola.
  let transcriptPad = null;
  let transcript = null;
  if (granolaWindowDays) {
    const transcriptHeader = el("div", {
      style: "display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 6px;",
    }, [
      el("div", { class: "settings-label" }, ["Call notes / transcripts (optional)"]),
      el("button", {
        class: "button button-secondary",
        id: `${modalId}-pull-granola`,
        type: "button",
        style: "padding: 4px 10px; font-size: 12px;",
      }, ["Pull from Granola"]),
    ]);
    transcript = el("textarea", {
      class: "modal-textarea",
      placeholder: "Paste call notes or transcripts here. Or click \"Pull from Granola\" to fetch the last " + granolaWindowDays + " days for this client.",
      rows: 6,
    });
    transcriptPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
      transcriptHeader,
      transcript,
    ]);
    modal.appendChild(transcriptPad);
  }

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  clientPicker.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  // Wire the Pull from Granola button (only present when transcript is set).
  if (transcript) {
    const pullBtn = document.getElementById(`${modalId}-pull-granola`);
    pullBtn.addEventListener("click", async () => {
      if (!isTauri) {
        showToast("Granola pull only works in the Companion app");
        return;
      }
      const code = clientPicker.value;
      if (!code) return showToast("Pick a client first");
      const originalLabel = pullBtn.textContent;
      pullBtn.disabled = true;
      pullBtn.textContent = "Pulling...";
      try {
        const clientName = clients.find((c) => c.code === code)?.name || null;
        const text = await invoke("pull_granola_transcripts", {
          input: {
            client_code: code,
            since_days: granolaWindowDays,
            client_name: clientName,
          },
        });
        // Append (don't replace) — Caitlin may have already pasted notes.
        const existing = transcript.value.trim();
        transcript.value = existing
          ? `${existing}\n\n--- Pulled from Granola ---\n\n${text}`
          : text;
        showToast("Transcripts pulled");
      } catch (e) {
        const msg = String(e.message || e);
        if (msg.includes("not connected") || msg.includes("client ID not")) {
          showToast(
            "Granola not connected. Open Settings → Granola to connect.",
            { ttl: 7000 },
          );
        } else {
          showToast(`Granola pull failed: ${msg}`, { ttl: 7000 });
        }
      } finally {
        pullBtn.disabled = false;
        pullBtn.textContent = originalLabel;
      }
    });
  }

  runBtn.addEventListener("click", async () => {
    runBtn.disabled = true;
    runBtn.textContent = "Pulling source...";
    // Pull picker blob (will skip silently for manual source). Append
    // the resulting context to transcript_notes so the existing backend
    // shape doesn't need to change.
    const blob = await picker.ensureContextBlob();
    const transcriptText = transcript ? transcript.value.trim() : "";
    let transcriptNotes = transcriptText;
    if (blob && blob.trim()) {
      transcriptNotes = transcriptText
        ? `${transcriptText}\n\n--- Source picker ---\n\n${blob}`
        : blob;
    }
    const input = {
      client_code: clientPicker.value,
      extra_notes: flags.value.trim() || null,
      transcript_notes: transcriptNotes || null,
    };
    runBtn.textContent = runningLabel;
    try {
      const json = await invoke(command, { input });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast(successToast);
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

async function showMonthlyCheckinModal(prefillClientCode) {
  return showClientPickerReviewModal({
    modalId: "mc-modal",
    title: "Monthly Check-in",
    meta: "Pick a client. Companion pulls their last 30 days of receipts as context, Claude returns a check-in receipt with what's done, what's open, what's next, and action items.",
    command: "run_monthly_checkin",
    runningLabel: "Reading the month...",
    successToast: "Check-in receipt ready",
    flagsPlaceholder: "e.g. payment overdue, scope creeping, key contact has changed, big upcoming on-sale. Skip if there's nothing.",
    prefillClientCode,
    granolaWindowDays: 30,
  });
}

async function showQuarterlyReviewModal(prefillClientCode) {
  return showClientPickerReviewModal({
    modalId: "qr-modal",
    title: "Quarterly Review",
    meta: "Pick a client. Companion pulls their last 90 days of receipts, Claude returns a QBR receipt with what worked, what didn't, and proposed next-quarter shape. Use this to drive the QBR call.",
    command: "run_quarterly_review",
    runningLabel: "Reading the quarter...",
    successToast: "QBR receipt ready",
    flagsPlaceholder: "e.g. renewal coming up, scope feels off, want to propose a shape change, considering wind-down. Skip if there's nothing pressing.",
    prefillClientCode,
    granolaWindowDays: 90,
  });
}

// ─── v0.31 Block F — Skills batch 1 modals ───────────────────────────
//
// Five workflows wired to the SKILL.md prompts under
// ~/.claude/skills/anthropic-skills/. Each modal collects inputs, fires
// the matching backend run_* command, prepends the receipt, and shows a
// "Next:" handoff button (per pillar 3) routing to the natural follow-up.
// monthly-checkin keeps its existing modal — only the system prompt
// changed in lib.rs.

// Show a small banner above the latest receipt with a "Next:" handoff
// action. Fires the supplied callback when clicked. Auto-dismisses once
// clicked or when the user clicks Dismiss.
function showHandoffBanner({ label, onClick, secondary }) {
  const feed = document.getElementById("feed");
  if (!feed) return;
  // Drop any prior banner so we never stack them.
  feed.querySelectorAll(".handoff-banner").forEach((b) => b.remove());

  const banner = el("div", { class: "handoff-banner" });
  const text = el("div", { class: "handoff-banner-text" }, [
    secondary ? `${secondary} · ` : "",
    el("strong", {}, [label]),
  ]);
  const actBtn = el("button", { class: "button", type: "button" }, ["Next"]);
  const dismissBtn = el("button", {
    class: "button button-secondary",
    type: "button",
  }, ["Dismiss"]);

  banner.appendChild(text);
  banner.appendChild(el("div", { class: "handoff-banner-actions" }, [
    dismissBtn,
    actBtn,
  ]));

  actBtn.addEventListener("click", () => {
    banner.remove();
    onClick?.();
  });
  dismissBtn.addEventListener("click", () => banner.remove());

  feed.prepend(banner);
}

// Pick the first item text from a receipt (used to grab the chosen
// caption/draft when handing off to Schedule social post). Receipts from
// these workflows put the variants/draft as items in sections[0].
function firstItemText(receipt) {
  const items = receipt?.sections?.[0]?.items || [];
  for (const item of items) {
    if (item && typeof item.text === "string" && item.text.trim()) {
      return item.text;
    }
  }
  return "";
}

async function showNctCaptionModal(_prefillClientCode) {
  if (document.getElementById("nct-cap-modal")) return;
  const overlay = el("div", { id: "nct-cap-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["Draft NCT social caption"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Northcote Theatre venue voice. Returns three caption variants. Pick one and hand off to Schedule social post.",
  ]));

  const grid = el("div", { class: "modal-field-grid" });

  const refUrl = el("input", {
    type: "url",
    class: "settings-input",
    placeholder: "https://... (optional)",
  });
  const voice = el("select", { class: "settings-input" });
  ["default", "playful", "serious", "urgent"].forEach((v) => {
    const opt = document.createElement("option");
    opt.value = v;
    opt.textContent = v;
    voice.appendChild(opt);
  });

  [
    ["Reference URL (optional)", refUrl],
    ["Voice override", voice],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });

  modal.appendChild(grid);

  const topicPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Topic"]),
  ]);
  const topic = el("textarea", {
    class: "modal-textarea",
    placeholder: "What the post is about. Show announcement, on-sale reminder, sold out, post-show, etc. Include artist name, date, doors/start times if relevant.",
    rows: 6,
  });
  topicPad.appendChild(topic);
  modal.appendChild(topicPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  topic.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    const input = {
      topic: topic.value.trim(),
      reference_url: refUrl.value.trim() || null,
      voice_override: voice.value,
    };
    if (!input.topic) return showToast("Topic required");
    runBtn.disabled = true;
    runBtn.textContent = "Drafting captions...";
    try {
      const json = await invoke("run_nct_caption", { input });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Three caption variants ready");
      showHandoffBanner({
        secondary: "Caption draft filed",
        label: "Schedule the post",
        onClick: () => {
          const caption = firstItemText(receipt);
          showScheduleSocialPostModal("NCT", {
            prefillCopy: caption,
            prefillPlatform: "Instagram",
          });
        },
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

async function showInCahootsSocialModal(_prefillClientCode) {
  if (document.getElementById("inc-soc-modal")) return;
  const overlay = el("div", { id: "inc-soc-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["In Cahoots social post"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Founder-led draft for the @incahoots.marketing brand. Caitlin's voice — observational, peer-to-peer, anti-corporate. Hands off to Schedule social post when you're happy with it.",
  ]));

  const grid = el("div", { class: "modal-field-grid" });

  const platform = el("select", { class: "settings-input" });
  ["Instagram", "LinkedIn", "Threads", "X", "TikTok", "Other"].forEach((p) => {
    const opt = document.createElement("option");
    opt.value = p;
    opt.textContent = p;
    platform.appendChild(opt);
  });

  const pillar = el("select", { class: "settings-input" });
  [
    "announcement",
    "case study",
    "behind-the-scenes",
    "value",
    "share",
  ].forEach((p) => {
    const opt = document.createElement("option");
    opt.value = p;
    opt.textContent = p;
    pillar.appendChild(opt);
  });

  [
    ["Platform", platform],
    ["Content pillar", pillar],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });

  modal.appendChild(grid);

  const topicPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Topic / observation"]),
  ]);
  const topic = el("textarea", {
    class: "modal-textarea",
    placeholder: "What you're noticing this week, a thing from the MBA, a campaign observation (anonymised), a sector reaction. One idea per post.",
    rows: 6,
  });
  topicPad.appendChild(topic);
  modal.appendChild(topicPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  topic.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    const input = {
      topic: topic.value.trim(),
      platform: platform.value,
      pillar: pillar.value,
    };
    if (!input.topic) return showToast("Topic required");
    runBtn.disabled = true;
    runBtn.textContent = "Drafting post...";
    try {
      const json = await invoke("run_in_cahoots_social_post", { input });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Draft post ready");
      showHandoffBanner({
        secondary: "In Cahoots draft filed",
        label: "Schedule the post",
        onClick: () => {
          const draft = firstItemText(receipt);
          showScheduleSocialPostModal(null, {
            prefillCopy: draft,
            prefillPlatform: input.platform,
            prefillChannel: "incahoots-marketing",
          });
        },
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

async function showWrapReportModal(prefillProjectCode) {
  if (document.getElementById("wrap-modal")) return;

  // Pull projects, filtered to wrap/done so the picker only offers
  // campaigns ready for a wrap report.
  let projects = [];
  if (isTauri) {
    try {
      const raw = await invoke("list_airtable_projects");
      const data = JSON.parse(raw);
      projects = (data.records || [])
        .map((r) => ({
          code: r.fields?.code || "",
          name: r.fields?.name || r.fields?.code || "Untitled",
          status: String(r.fields?.status || "").toLowerCase(),
        }))
        .filter((p) => p.code)
        .filter((p) =>
          ["wrap", "done", "wrapped", "complete", "completed"].includes(p.status)
        );
    } catch (e) {
      console.warn("list_airtable_projects failed:", e);
    }
  }

  if (projects.length === 0) {
    showToast(
      "No projects with status wrap or done. Mark a project wrap first.",
      { ttl: 6000 }
    );
    return;
  }

  const overlay = el("div", { id: "wrap-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["Draft Wrap Report"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Companion pulls receipts and notes for the project, Claude returns a wrap report. Markdown saved to Dropbox/IN CAHOOTS/08 CAMPAIGN SNAPSHOTS/{code}-wrap.md for client delivery.",
  ]));

  const grid = el("div", { class: "modal-field-grid" });

  const projectPicker = el("select", { class: "settings-input" });
  projects.forEach((p) => {
    const opt = document.createElement("option");
    opt.value = p.code;
    opt.textContent = `${p.code} — ${p.name}`;
    projectPicker.appendChild(opt);
  });
  if (
    prefillProjectCode &&
    projects.some((p) => p.code === prefillProjectCode)
  ) {
    projectPicker.value = prefillProjectCode;
  }

  grid.appendChild(el("label", { class: "modal-field" }, [
    el("div", { class: "settings-label" }, ["Project"]),
    projectPicker,
  ]));
  modal.appendChild(grid);

  const flagsPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Extra notes (optional)"]),
  ]);
  const flags = el("textarea", {
    class: "modal-textarea",
    placeholder: "Final ticket figure if not in receipts, audience read, anything that didn't show up in the receipts log.",
    rows: 5,
  });
  flagsPad.appendChild(flags);
  modal.appendChild(flagsPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  projectPicker.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    const input = {
      project_code: projectPicker.value,
      extra_notes: flags.value.trim() || null,
    };
    if (!input.project_code) return showToast("Pick a project first");
    runBtn.disabled = true;
    runBtn.textContent = "Reading the campaign...";
    try {
      const json = await invoke("run_campaign_wrap_report", { input });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Wrap report drafted and saved to Dropbox");
      showHandoffBanner({
        secondary: `Saved to Dropbox/08 CAMPAIGN SNAPSHOTS/${input.project_code}-wrap.md`,
        label: "Send to client (v0.32)",
        onClick: () => {
          showToast(
            "Client-email handoff ships in v0.32. For now, open the markdown and send manually.",
            { ttl: 6000 }
          );
        },
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// ── Campaign Launch Checklist (v0.35) ─────────────────────────────────
//
// Walks the six-phase pre-launch SOP for paid campaigns. Deterministic
// L3 workflow — no Claude call. Caitlin ticks items, adds notes, hits
// Run; the backend builds a Receipt envelope, persists, files to
// Airtable, and posts a launch-ready summary to #client-work.
const CAMPAIGN_LAUNCH_PHASES = [
  {
    title: "Strategy and approvals",
    items: [
      "Campaign objective confirmed (ticket sales / awareness / engagement / other)",
      "Target audience defined",
      "Key messaging approved by client",
      "Campaign timeline confirmed (start date, end date, key milestones)",
      "Budget confirmed (ad spend separate from service fee)",
      "Ad spend approved and transferred (or payment method confirmed)",
      "Creative brief written (if designer or photographer involved)",
      "All stakeholder approvals received",
    ],
  },
  {
    title: "Creative and content",
    items: [
      "Ad creative received or produced (images, video, copy)",
      "Creative meets platform specs (Meta, Instagram, TikTok, etc.)",
      "Copy proofread (artist names, dates, venue, ticket price, ticket link)",
      "Ticket link tested and working",
      "Landing page or event page live and correct",
      "UTMs built and applied to all links (see Ad Naming and UTM Conventions)",
      "Social posts drafted and scheduled (if part of campaign)",
      "EDM drafted and tested (if part of campaign)",
    ],
  },
  {
    title: "Paid ads setup",
    items: [
      "Campaign named using naming convention (see Ad Naming SOP)",
      "Pixel and tracking installed and firing on destination page",
      "Custom audiences built (if applicable)",
      "Targeting reviewed (location, age, interests, exclusions)",
      "Budget set correctly (daily vs lifetime)",
      "Schedule set (start and end dates)",
      "Placements reviewed (auto vs manual)",
      "Ad preview checked on mobile and desktop",
      "Conversion event selected correctly",
      "Advantage+ audience expansion setting confirmed (ON for prospecting, OFF for retargeting)",
    ],
  },
  {
    title: "Tracking and measurement",
    items: [
      "UTM parameters confirmed",
      "Google Analytics goals or events set up (if applicable)",
      "Ticket sales baseline recorded (current sales before campaign starts)",
      "Reporting cadence confirmed with client",
      "First report date scheduled",
      "/grill <client> baseline pulled (if account is known to the skill)",
    ],
  },
  {
    title: "Go live",
    items: [
      "Final review of all ads, posts, and EDMs",
      "Ads submitted for review (allow 24 hrs for Meta approval)",
      "Ads approved and live",
      "Social posts published or scheduled",
      "EDM sent or scheduled",
      "Client notified that campaign is live",
      "First 24-hour performance check scheduled",
    ],
  },
  {
    title: "Post-launch (first 48 hours)",
    items: [
      "All ads delivering correctly (no disapprovals, budget pacing OK)",
      "No broken links or tracking issues",
      "Early performance snapshot taken",
      "Any underperforming ads flagged for optimisation",
      "Client updated on launch results",
    ],
  },
];

async function showCampaignLaunchChecklistModal(prefillProjectCode) {
  if (document.getElementById("launch-checklist-modal")) return;

  // Pull projects: campaigns or active. The skill is gated to those at
  // the registry level, but we re-filter here so the picker matches.
  let projects = [];
  if (isTauri) {
    try {
      const raw = await invoke("list_airtable_projects");
      const data = JSON.parse(raw);
      projects = (data.records || [])
        .map((r) => ({
          code: r.fields?.code || "",
          name: r.fields?.name || r.fields?.code || "Untitled",
          status: String(r.fields?.status || "").toLowerCase(),
          campaign_type: String(r.fields?.campaign_type || "").toLowerCase(),
          client: r.fields?.client_code || r.fields?.client || "",
        }))
        .filter((p) => p.code)
        .filter(
          (p) => p.campaign_type === "campaign" || p.status === "active"
        );
    } catch (e) {
      console.warn("list_airtable_projects failed:", e);
    }
  }

  if (projects.length === 0) {
    showToast(
      "No campaign or active projects to launch. Mark a project type=campaign or status=active first.",
      { ttl: 6000 }
    );
    return;
  }

  const overlay = el("div", {
    id: "launch-checklist-modal",
    class: "modal-overlay",
  });
  const modal = el("div", { class: "modal modal-wide" });
  const close = () => overlay.remove();

  modal.appendChild(
    el("div", { class: "modal-header" }, [
      el("div", { class: "modal-title" }, ["Campaign Launch Checklist"]),
      el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
    ])
  );
  modal.appendChild(
    el("div", { class: "modal-meta" }, [
      "Six-phase pre-launch verification. Tick what's done, note what's outstanding. Files a Receipt, prints to Munbyn, posts a launch-ready summary to #client-work. The Publish click in Meta Ads Manager stays manual.",
    ])
  );

  // Project picker
  const grid = el("div", { class: "modal-field-grid" });
  const projectPicker = el("select", { class: "settings-input" });
  projects.forEach((p) => {
    const opt = document.createElement("option");
    opt.value = p.code;
    const meta = [p.campaign_type, p.status].filter(Boolean).join(" · ");
    opt.textContent = meta
      ? `${p.code} — ${p.name} (${meta})`
      : `${p.code} — ${p.name}`;
    projectPicker.appendChild(opt);
  });
  if (
    prefillProjectCode &&
    projects.some((p) => p.code === prefillProjectCode)
  ) {
    projectPicker.value = prefillProjectCode;
  }
  grid.appendChild(
    el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, ["Project"]),
      projectPicker,
    ])
  );
  modal.appendChild(grid);

  // Phase sections — each <details> collapsible with checkbox + note rows.
  const phasesPad = el("div", {
    class: "modal-pad",
    style: "margin-top: 16px;",
  });

  // Track tick state in plain JS so the summary can read it.
  const tickState = CAMPAIGN_LAUNCH_PHASES.map((phase) => ({
    title: phase.title,
    items: phase.items.map((text) => ({ text, ticked: false, note: "" })),
  }));

  // Re-render the launch-ready summary on any change.
  let summaryEl = null;
  function updateSummary() {
    if (!summaryEl) return;
    const totalItems = tickState.reduce((n, p) => n + p.items.length, 0);
    const tickedItems = tickState.reduce(
      (n, p) => n + p.items.filter((i) => i.ticked).length,
      0
    );
    const phasesAllDone = tickState.filter((p) =>
      p.items.every((i) => i.ticked)
    ).length;
    summaryEl.innerHTML = "";
    const headline =
      tickedItems === totalItems
        ? "Launch ready. Hit Publish in Ads Manager."
        : `${tickedItems} of ${totalItems} ticked across ${tickState.length} phases.`;
    summaryEl.appendChild(
      el("div", { class: "settings-label" }, ["Launch ready summary"])
    );
    summaryEl.appendChild(el("div", {}, [headline]));
    const phaseLines = el("div", { style: "margin-top: 6px;" });
    tickState.forEach((p) => {
      const done = p.items.filter((i) => i.ticked).length;
      const status = done === p.items.length ? "✓" : `${done}/${p.items.length}`;
      phaseLines.appendChild(
        el("div", {}, [`${status} ${p.title}`])
      );
    });
    summaryEl.appendChild(phaseLines);
    summaryEl.appendChild(
      el("div", { style: "margin-top: 6px; opacity: 0.7;" }, [
        `${phasesAllDone} of ${tickState.length} phases fully ticked.`,
      ])
    );
  }

  CAMPAIGN_LAUNCH_PHASES.forEach((phase, phaseIdx) => {
    const details = document.createElement("details");
    details.className = "modal-details";
    if (phaseIdx === 0) details.open = true;
    const summary = document.createElement("summary");
    summary.className = "settings-label";
    summary.textContent = `${phaseIdx + 1}. ${phase.title}`;
    details.appendChild(summary);

    phase.items.forEach((itemText, itemIdx) => {
      const row = el("div", {
        class: "modal-field",
        style: "margin-top: 6px;",
      });
      const cbWrap = el("label", {
        style: "display: flex; align-items: flex-start; gap: 8px;",
      });
      const cb = el("input", { type: "checkbox" });
      cb.addEventListener("change", () => {
        tickState[phaseIdx].items[itemIdx].ticked = cb.checked;
        updateSummary();
      });
      cbWrap.appendChild(cb);
      cbWrap.appendChild(el("div", {}, [itemText]));
      row.appendChild(cbWrap);

      const note = el("input", {
        type: "text",
        class: "settings-input",
        placeholder: "Note (optional)",
        style: "margin-top: 4px;",
      });
      note.addEventListener("input", () => {
        tickState[phaseIdx].items[itemIdx].note = note.value;
      });
      row.appendChild(note);

      details.appendChild(row);
    });
    phasesPad.appendChild(details);
  });
  modal.appendChild(phasesPad);

  // Extra notes
  const extraPad = el("div", {
    class: "modal-pad",
    style: "margin-top: 16px;",
  });
  extraPad.appendChild(
    el("div", { class: "settings-label" }, ["Extra notes (optional)"])
  );
  const extra = el("textarea", {
    class: "modal-textarea",
    rows: 3,
    placeholder:
      "Anything the phases didn't cover. Goes into the Receipt and the Slack post.",
  });
  extraPad.appendChild(extra);
  modal.appendChild(extraPad);

  // Live summary block
  summaryEl = el("div", {
    class: "modal-pad",
    style: "margin-top: 16px;",
  });
  modal.appendChild(summaryEl);
  updateSummary();

  const cancelBtn = el("button", { class: "button button-secondary" }, [
    "Cancel",
  ]);
  const runBtn = el("button", { class: "button" }, ["File checklist"]);
  modal.appendChild(
    el("div", { class: "modal-actions" }, [cancelBtn, runBtn])
  );

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  projectPicker.focus();

  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) close();
  });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    const project_code = projectPicker.value;
    if (!project_code) return showToast("Pick a project first");
    runBtn.disabled = true;
    runBtn.textContent = "Filing...";
    try {
      const json = await invoke("run_campaign_launch_checklist", {
        input: {
          project_code,
          phases: tickState,
          extra_notes: extra.value.trim() || null,
        },
      });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Launch checklist filed. Munbyn print queued.");
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "File checklist";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

async function showBuildScopeModal(prefillClientCode) {
  if (document.getElementById("scope-modal")) return;

  const clients = await fetchClientsForPicker();

  const overlay = el("div", { id: "scope-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["Build Scope"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Generates a Scope of Work in Caitlin's voice. Saves a JSON payload at 05 TEMPLATES/RECEIPT-DOCS/typst/data/scopes/ for typst rendering. CSA generation hands off in v0.33.",
  ]));

  const grid = el("div", { class: "modal-field-grid" });

  const clientPicker = el("select", { class: "settings-input" });
  const noClient = document.createElement("option");
  noClient.value = "";
  noClient.textContent = "(new client / lead)";
  clientPicker.appendChild(noClient);
  clients.forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c.code;
    opt.textContent = `${c.code} — ${c.name}`;
    clientPicker.appendChild(opt);
  });
  if (prefillClientCode && clients.some((c) => c.code === prefillClientCode)) {
    clientPicker.value = prefillClientCode;
  }

  const newClientName = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "Only if (new client / lead) is selected above",
  });

  const projectType = el("select", { class: "settings-input" });
  ["campaign", "retainer", "fractional", "capability building"].forEach((t) => {
    const opt = document.createElement("option");
    opt.value = t;
    opt.textContent = t;
    projectType.appendChild(opt);
  });

  const length = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "e.g. 6 weeks, 3 months, ongoing fortnightly",
  });

  const budget = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "e.g. $8k, ~$15k, TBC",
  });

  [
    ["Client", clientPicker],
    ["New client name (if lead)", newClientName],
    ["Project type", projectType],
    ["Engagement length", length],
    ["Budget range", budget],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });
  modal.appendChild(grid);

  // v0.32: source-picker for the brief (call notes, discovery form
  // submission, etc). Manual textarea below stays as the always-on
  // fallback / additional notes.
  const available = await probeSourceAvailability();
  const pickerState = createPickerState({ clientCode: prefillClientCode || null });
  const picker = mountSourcePicker({
    container: modal,
    state: pickerState,
    available,
    label: "Brief source (optional)",
  });

  const deliverablesPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Deliverables / brief notes"]),
  ]);
  const deliverables = el("textarea", {
    class: "modal-textarea",
    placeholder: "What's included, what's not, what success looks like, what the client owns. Bullet form is fine.",
    rows: 8,
  });
  deliverablesPad.appendChild(deliverables);
  modal.appendChild(deliverablesPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  deliverables.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    const baseDeliverables = deliverables.value.trim();
    const blob = await picker.ensureContextBlob();
    // Prepend the source blob as extra context to the deliverables —
    // run_scope_of_work doesn't take context_blob separately, so we
    // splice it into the deliverables string.
    const composed = blob
      ? `${baseDeliverables}\n\n---\n\nSource context:\n\n${blob}`
      : baseDeliverables;
    const input = {
      client_code: clientPicker.value || null,
      new_client_name: newClientName.value.trim() || null,
      project_type: projectType.value,
      length: length.value.trim() || null,
      deliverables: composed,
      budget_range: budget.value.trim() || null,
    };
    if (!baseDeliverables && !blob) return showToast("Deliverables or a source required");
    if (!input.client_code && !input.new_client_name) {
      return showToast("Pick a client or enter a new client name");
    }
    runBtn.disabled = true;
    runBtn.textContent = "Building scope...";
    try {
      const json = await invoke("run_scope_of_work", { input });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Scope drafted and saved to Dropbox");
      showHandoffBanner({
        secondary: "Scope JSON saved for typst rendering",
        label: "Generate CSA (v0.33)",
        onClick: () => {
          showToast(
            "CSA generation via Dropbox Sign ships in v0.33. For now, the markdown is in the receipt and the JSON is on disk.",
            { ttl: 7000 }
          );
        },
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// ─── v0.32 Block F — Skills batch 2 modals ───────────────────────────
//
// Seven new workflows wired from skill SKILL.md files. Each modal uses
// the new source-picker pattern (Granola / Gmail / Slack / Calendar /
// Form / Manual paste) for the brief input. Source-picker auto-hides
// sources whose OAuth isn't connected.

// Shared helper — opens a modal scaffold, mounts a source-picker, and
// returns the elements + picker handle. The body builder fills in the
// rest of the modal's fields. Reuses the same overlay/close pattern as
// the v0.31 skill modals.
async function buildSkillModal({
  modalId,
  title,
  meta,
  prefillClientCode = null,
  showSourcePicker = true,
  pickerLabel = "Brief source",
}) {
  if (document.getElementById(modalId)) return null;
  const overlay = el("div", { id: modalId, class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, [title]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  if (meta) modal.appendChild(el("div", { class: "modal-meta" }, [meta]));

  let picker = null;
  if (showSourcePicker) {
    const available = await probeSourceAvailability();
    const state = createPickerState({ clientCode: prefillClientCode });
    picker = mountSourcePicker({
      container: modal,
      state,
      available,
      label: pickerLabel,
    });
  }

  return { overlay, modal, close, picker };
}

async function showPressReleaseModal(prefillClientCode) {
  const clients = await fetchClientsForPicker();
  const built = await buildSkillModal({
    modalId: "pr-modal",
    title: "Draft Press Release",
    meta: "Pulls the source brief (call, email, brief etc.) and returns a press release in Caitlin's voice. Saved as a receipt for client send-off.",
    prefillClientCode,
  });
  if (!built) return;
  const { overlay, modal, close, picker } = built;

  const grid = el("div", { class: "modal-field-grid" });
  const clientPicker = el("select", { class: "settings-input" });
  const noClient = document.createElement("option");
  noClient.value = "";
  noClient.textContent = "(no client / In Cahoots)";
  clientPicker.appendChild(noClient);
  clients.forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c.code;
    opt.textContent = `${c.code} — ${c.name}`;
    clientPicker.appendChild(opt);
  });
  if (prefillClientCode && clients.some((c) => c.code === prefillClientCode)) {
    clientPicker.value = prefillClientCode;
  }
  const angle = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "e.g. on-sale announcement, casting reveal, festival lineup",
  });
  [
    ["Client", clientPicker],
    ["Angle / news hook", angle],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });
  modal.appendChild(grid);

  const notesPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Extra notes (optional)"]),
  ]);
  const notes = el("textarea", {
    class: "modal-textarea",
    placeholder: "Anything not covered by the source — quotes from the artist, key dates, embargo, etc.",
    rows: 4,
  });
  notesPad.appendChild(notes);
  modal.appendChild(notesPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));
  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  angle.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    if (!angle.value.trim()) return showToast("Angle required");
    runBtn.disabled = true;
    runBtn.textContent = "Pulling source...";
    const blob = await picker.ensureContextBlob();
    runBtn.textContent = "Drafting press release...";
    try {
      const json = await invoke("run_press_release", {
        input: {
          client_code: clientPicker.value || null,
          context_blob: blob || null,
          angle: angle.value.trim(),
          extra_notes: notes.value.trim() || null,
        },
      });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Press release drafted");
      showHandoffBanner({
        secondary: "Press release filed",
        label: "Send to media list (v0.40+)",
        onClick: () => showToast("Media-list send ships in v0.40+. For now, copy from the receipt.", { ttl: 6000 }),
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

async function showEdmWriterModal(prefillClientCode) {
  const clients = await fetchClientsForPicker();
  const built = await buildSkillModal({
    modalId: "edm-modal",
    title: "Draft EDM",
    meta: "Subject lines + full email body for client newsletters. Picks up the brief from the source-picker.",
    prefillClientCode,
  });
  if (!built) return;
  const { overlay, modal, close, picker } = built;

  const grid = el("div", { class: "modal-field-grid" });
  const clientPicker = el("select", { class: "settings-input" });
  const noClient = document.createElement("option");
  noClient.value = "";
  noClient.textContent = "(no client / In Cahoots)";
  clientPicker.appendChild(noClient);
  clients.forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c.code;
    opt.textContent = `${c.code} — ${c.name}`;
    clientPicker.appendChild(opt);
  });
  if (prefillClientCode && clients.some((c) => c.code === prefillClientCode)) {
    clientPicker.value = prefillClientCode;
  }

  const purpose = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "e.g. on-sale announcement, monthly update, post-show thank you",
  });
  const audience = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "e.g. ticket buyers, members, full list, lapsed",
  });

  [
    ["Client", clientPicker],
    ["Purpose", purpose],
    ["Audience", audience],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });
  modal.appendChild(grid);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));
  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  purpose.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    if (!purpose.value.trim()) return showToast("Purpose required");
    runBtn.disabled = true;
    runBtn.textContent = "Pulling source...";
    const blob = await picker.ensureContextBlob();
    runBtn.textContent = "Drafting EDM...";
    try {
      const json = await invoke("run_edm_writer", {
        input: {
          client_code: clientPicker.value || null,
          context_blob: blob || null,
          purpose: purpose.value.trim(),
          audience: audience.value.trim() || null,
        },
      });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("EDM drafted");
      showHandoffBanner({
        secondary: "EDM draft filed",
        label: "Schedule send (v0.40+)",
        onClick: () => showToast("Send-scheduling ships in v0.40+. For now, copy from the receipt.", { ttl: 6000 }),
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// ─── Draft dossier modal (v0.39) ──────────────────────────────────────
//
// Two modes:
//   - Existing client: pick from the Airtable-backed client list.
//   - New lead: type a name + optional slug, no Airtable record needed
//     yet. The dossier still drafts and writes to disk so Caitlin can
//     spin up the doc before formal onboarding.
// Source-picker is the v0.32 component. Run hits run_draft_dossier
// which writes the markdown to ~/Dropbox/IN CAHOOTS/03 CLIENT
// DOSSIERS/{slug}.md (or {slug}-draft-{date}.md if there's already one
// on disk). Caitlin signs off and merges manually.
function slugifyForUI(name) {
  return (name || "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

async function showDraftDossierModal(prefillClientCode) {
  const clients = await fetchClientsForPicker();
  const built = await buildSkillModal({
    modalId: "draft-dossier-modal",
    title: "Draft dossier",
    meta: "Pulls a brief from Granola, Gmail, Slack, Calendar, an intake form, or a manual paste, and drafts the 12-section client dossier. Saves to 03 CLIENT DOSSIERS. Caitlin signs off the draft before publishing.",
    prefillClientCode,
  });
  if (!built) return;
  const { overlay, modal, close, picker } = built;

  // Mode toggle: existing client or new lead.
  const grid = el("div", { class: "modal-field-grid" });

  const modeRow = el("div", { class: "modal-field" }, [
    el("div", { class: "settings-label" }, ["Mode"]),
  ]);
  const modeWrap = el("div", { style: "display: flex; gap: 8px;" });
  const modeExisting = el(
    "button",
    {
      type: "button",
      class: "button button-secondary",
      style: "padding: 4px 12px; font-size: 12px;",
    },
    ["Existing client"]
  );
  const modeNew = el(
    "button",
    {
      type: "button",
      class: "button button-secondary",
      style: "padding: 4px 12px; font-size: 12px;",
    },
    ["New lead"]
  );
  modeWrap.appendChild(modeExisting);
  modeWrap.appendChild(modeNew);
  modeRow.appendChild(modeWrap);
  grid.appendChild(modeRow);

  // Existing-client picker.
  const existingRow = el("label", { class: "modal-field" }, [
    el("div", { class: "settings-label" }, ["Client"]),
  ]);
  const clientPicker = el("select", { class: "settings-input" });
  const noClient = document.createElement("option");
  noClient.value = "";
  noClient.textContent = "Select a client";
  clientPicker.appendChild(noClient);
  clients.forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c.code;
    opt.textContent = `${c.code} — ${c.name}`;
    clientPicker.appendChild(opt);
  });
  if (prefillClientCode && clients.some((c) => c.code === prefillClientCode)) {
    clientPicker.value = prefillClientCode;
  }
  existingRow.appendChild(clientPicker);
  grid.appendChild(existingRow);

  // New-lead inputs (name + slug). Slug auto-fills from name unless the
  // user edits it.
  const nameRow = el("label", { class: "modal-field" }, [
    el("div", { class: "settings-label" }, ["Client name"]),
  ]);
  const nameInput = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "e.g. The Push, Castlemaine State Festival, Aldous Harding",
  });
  nameRow.appendChild(nameInput);
  grid.appendChild(nameRow);

  const slugRow = el("label", { class: "modal-field" }, [
    el("div", { class: "settings-label" }, ["Slug (filename)"]),
  ]);
  const slugInput = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "e.g. the-push (auto-fills from name)",
  });
  let slugTouched = false;
  slugRow.appendChild(slugInput);
  grid.appendChild(slugRow);

  modal.appendChild(grid);

  // Extra-notes pad.
  const notesPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Extra notes (optional)"]),
  ]);
  const notes = el("textarea", {
    class: "modal-textarea",
    placeholder: "Anything not in the source — your knowledge of this client, prior call summaries, things to ask about.",
    rows: 4,
  });
  notesPad.appendChild(notes);
  modal.appendChild(notesPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));
  overlay.appendChild(modal);
  document.body.appendChild(overlay);

  // Wire up the mode toggle. Default mode = existing client when a
  // prefill code came in, otherwise existing if there are clients,
  // otherwise new lead.
  let mode = prefillClientCode ? "existing" : clients.length ? "existing" : "new";
  function applyMode() {
    if (mode === "existing") {
      modeExisting.classList.remove("button-secondary");
      modeNew.classList.add("button-secondary");
      existingRow.style.display = "";
      nameRow.style.display = "none";
      slugRow.style.display = "none";
    } else {
      modeExisting.classList.add("button-secondary");
      modeNew.classList.remove("button-secondary");
      existingRow.style.display = "none";
      nameRow.style.display = "";
      slugRow.style.display = "";
    }
  }
  modeExisting.addEventListener("click", () => { mode = "existing"; applyMode(); });
  modeNew.addEventListener("click", () => { mode = "new"; applyMode(); });
  applyMode();
  if (mode === "existing") {
    setTimeout(() => clientPicker.focus(), 0);
  } else {
    setTimeout(() => nameInput.focus(), 0);
  }

  // Auto-fill slug from name unless the user has edited it.
  nameInput.addEventListener("input", () => {
    if (!slugTouched) slugInput.value = slugifyForUI(nameInput.value);
  });
  slugInput.addEventListener("input", () => { slugTouched = true; });

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    let payload = {};
    if (mode === "existing") {
      if (!clientPicker.value) return showToast("Pick a client or switch to New lead");
      payload.client_code = clientPicker.value;
    } else {
      const name = nameInput.value.trim();
      if (!name) return showToast("Client name required");
      const slug = (slugInput.value.trim() || slugifyForUI(name));
      if (!slug) return showToast("Slug required (letters and numbers only)");
      payload.new_client_name = name;
      payload.new_client_slug = slug;
    }

    runBtn.disabled = true;
    runBtn.textContent = "Pulling source...";
    const blob = await picker.ensureContextBlob();
    runBtn.textContent = "Drafting dossier...";
    try {
      const json = await invoke("run_draft_dossier", {
        input: {
          ...payload,
          context_blob: blob || null,
          extra_notes: notes.value.trim() || null,
        },
      });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Dossier drafted");
      showHandoffBanner({
        secondary: "Dossier draft saved to 03 CLIENT DOSSIERS",
        label: "Open the dossier folder",
        onClick: () => showToast("Find it at ~/Dropbox/IN CAHOOTS/03 CLIENT DOSSIERS/", { ttl: 6000 }),
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

async function showReelsScriptingModal(prefillClientCode) {
  const clients = await fetchClientsForPicker();
  const built = await buildSkillModal({
    modalId: "reels-modal",
    title: "Script a Reel",
    meta: "Reference reel + topic in, full script out (hook, beats, captions). Hands off to a Subcontractor brief Receipt for the videographer.",
    prefillClientCode,
  });
  if (!built) return;
  const { overlay, modal, close, picker } = built;

  const grid = el("div", { class: "modal-field-grid" });
  const clientPicker = el("select", { class: "settings-input" });
  const noClient = document.createElement("option");
  noClient.value = "";
  noClient.textContent = "(no client / In Cahoots)";
  clientPicker.appendChild(noClient);
  clients.forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c.code;
    opt.textContent = `${c.code} — ${c.name}`;
    clientPicker.appendChild(opt);
  });
  if (prefillClientCode && clients.some((c) => c.code === prefillClientCode)) {
    clientPicker.value = prefillClientCode;
  }
  const topic = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "What the reel's about — show, artist, behind-the-scenes etc.",
  });
  const refUrl = el("input", {
    type: "url",
    class: "settings-input",
    placeholder: "https://www.instagram.com/reel/... (optional reference reel)",
  });

  [
    ["Client", clientPicker],
    ["Topic", topic],
    ["Reference reel URL", refUrl],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });
  modal.appendChild(grid);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));
  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  topic.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    if (!topic.value.trim()) return showToast("Topic required");
    runBtn.disabled = true;
    runBtn.textContent = "Pulling source...";
    const blob = await picker.ensureContextBlob();
    runBtn.textContent = "Scripting reel...";
    try {
      const json = await invoke("run_reels_scripting", {
        input: {
          client_code: clientPicker.value || null,
          context_blob: blob || null,
          topic: topic.value.trim(),
          reference_url: refUrl.value.trim() || null,
        },
      });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Reel script ready");
      showHandoffBanner({
        secondary: "Reel script filed",
        label: "Brief videographer (subcontractor)",
        onClick: () => showToast("Subcontractor brief receipt — open Subcontractor Onboarding from Today.", { ttl: 7000 }),
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// Hook generator — inline tool. No source-picker (just a topic). Returns
// 6 variants; receipt rendering shows them as items the user can pick
// from. L2 autonomy.
async function showHookGeneratorModal(prefillTopic = "") {
  const built = await buildSkillModal({
    modalId: "hook-modal",
    title: "Generate hooks",
    meta: "Six two-line hook variants for any topic. Pick one to drop into a social composer.",
    showSourcePicker: false,
  });
  if (!built) return;
  const { overlay, modal, close } = built;

  const topicPad = el("div", { class: "modal-pad" }, [
    el("div", { class: "settings-label" }, ["Topic"]),
  ]);
  const topic = el("textarea", {
    class: "modal-textarea",
    placeholder: "What you want hooks for. One topic per run — short and specific.",
    rows: 3,
  });
  if (prefillTopic) topic.value = prefillTopic;
  topicPad.appendChild(topic);
  modal.appendChild(topicPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));
  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  topic.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    if (!topic.value.trim()) return showToast("Topic required");
    runBtn.disabled = true;
    runBtn.textContent = "Generating hooks...";
    try {
      const json = await invoke("run_hook_generator", {
        input: { topic: topic.value.trim() },
      });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Six hook variants ready");
      showHandoffBanner({
        secondary: "Hooks filed",
        label: "Pick one and drop into the social composer",
        onClick: () => {
          const hook = firstItemText(receipt);
          showInCahootsSocialModal();
          // Best-effort: prefill the topic textarea once the modal mounts.
          setTimeout(() => {
            const ta = document.querySelector("#inc-soc-modal .modal-textarea");
            if (ta && hook) ta.value = hook;
          }, 50);
        },
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

async function showClientEmailModal(prefillClientCode) {
  const clients = await fetchClientsForPicker();
  if (clients.length === 0) {
    return showToast("No clients in Airtable. Onboard a client first.", { ttl: 6000 });
  }
  const built = await buildSkillModal({
    modalId: "ce-modal",
    title: "Draft email to client",
    meta: "Composes a client-facing email in Caitlin's voice. Hand off to humaniser + copy-editor before sending.",
    prefillClientCode,
  });
  if (!built) return;
  const { overlay, modal, close, picker } = built;

  const grid = el("div", { class: "modal-field-grid" });
  const clientPicker = el("select", { class: "settings-input" });
  clients.forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c.code;
    opt.textContent = `${c.code} — ${c.name}`;
    clientPicker.appendChild(opt);
  });
  if (prefillClientCode && clients.some((c) => c.code === prefillClientCode)) {
    clientPicker.value = prefillClientCode;
  }
  const purpose = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "e.g. follow-up to discovery call, scope confirmation, meeting reschedule",
  });

  [
    ["Client", clientPicker],
    ["Purpose", purpose],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });
  modal.appendChild(grid);

  const notesPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Extra notes (optional)"]),
  ]);
  const notes = el("textarea", {
    class: "modal-textarea",
    placeholder: "Things the source doesn't cover — tone preference, specific ask, deadline, etc.",
    rows: 4,
  });
  notesPad.appendChild(notes);
  modal.appendChild(notesPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));
  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  purpose.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    if (!clientPicker.value) return showToast("Pick a client first");
    if (!purpose.value.trim()) return showToast("Purpose required");
    runBtn.disabled = true;
    runBtn.textContent = "Pulling source...";
    const blob = await picker.ensureContextBlob();
    runBtn.textContent = "Drafting email...";
    try {
      const json = await invoke("run_client_email", {
        input: {
          client_code: clientPicker.value,
          context_blob: blob || null,
          purpose: purpose.value.trim(),
          extra_notes: notes.value.trim() || null,
        },
      });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Email drafted");
      showHandoffBanner({
        secondary: "Client email filed",
        label: "Run through humaniser + copy-editor",
        onClick: () => {
          const draft = firstItemText(receipt);
          showHumaniserModal(draft);
        },
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// Inline edit-pass modals (humaniser + copy-editor). Both take a single
// text input and return the rewritten version. Used as inline buttons on
// any draft text — see openInlineEditPass below.
async function showHumaniserModal(prefillText = "") {
  const built = await buildSkillModal({
    modalId: "hum-modal",
    title: "Edit pass: humaniser",
    meta: "Removes signs of AI writing. Plain prose, Australian spelling, no em dashes.",
    showSourcePicker: false,
  });
  if (!built) return;
  const { overlay, modal, close } = built;

  const pad = el("div", { class: "modal-pad" }, [
    el("div", { class: "settings-label" }, ["Text to humanise"]),
  ]);
  const textArea = el("textarea", {
    class: "modal-textarea",
    placeholder: "Paste the draft text here. Whole emails, posts, paragraphs — all fine.",
    rows: 12,
  });
  if (prefillText) textArea.value = prefillText;
  pad.appendChild(textArea);
  modal.appendChild(pad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));
  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  textArea.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    if (!textArea.value.trim()) return showToast("Paste text first");
    runBtn.disabled = true;
    runBtn.textContent = "Humanising...";
    try {
      const json = await invoke("run_humanizer", {
        input: { text: textArea.value },
      });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Humanised draft ready");
      showHandoffBanner({
        secondary: "Humanised draft filed",
        label: "Apply changes (copy from receipt)",
        onClick: () => {
          const rewritten = firstItemText(receipt);
          if (rewritten && navigator.clipboard?.writeText) {
            navigator.clipboard.writeText(rewritten);
            showToast("Rewritten copy copied to clipboard");
          }
        },
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

async function showCopyEditorModal(prefillText = "") {
  const built = await buildSkillModal({
    modalId: "copy-modal",
    title: "Edit pass: copy editor",
    meta: "In Cahoots copy-editor pass. Cleans up grammar, tightens sentences, matches Caitlin's voice.",
    showSourcePicker: false,
  });
  if (!built) return;
  const { overlay, modal, close } = built;

  const pad = el("div", { class: "modal-pad" }, [
    el("div", { class: "settings-label" }, ["Text to edit"]),
  ]);
  const textArea = el("textarea", {
    class: "modal-textarea",
    placeholder: "Paste the draft text here.",
    rows: 12,
  });
  if (prefillText) textArea.value = prefillText;
  pad.appendChild(textArea);
  modal.appendChild(pad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));
  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  textArea.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    if (!textArea.value.trim()) return showToast("Paste text first");
    runBtn.disabled = true;
    runBtn.textContent = "Editing...";
    try {
      const json = await invoke("run_copy_editor", {
        input: { text: textArea.value },
      });
      const receipt = JSON.parse(json);
      document.getElementById("feed").prepend(renderReceipt(receipt));
      close();
      showToast("Edited draft ready");
      showHandoffBanner({
        secondary: "Copy-editor pass filed",
        label: "Apply changes (copy from receipt)",
        onClick: () => {
          const edited = firstItemText(receipt);
          if (edited && navigator.clipboard?.writeText) {
            navigator.clipboard.writeText(edited);
            showToast("Edited copy copied to clipboard");
          }
        },
      });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// ─── Today: commitment detail modal ──────────────────────────────────
//
// Shown when a commitment row in Today is clicked. Two actions: tick-done
// (set status='done') and push-to-time (bump next_check_at by 4 hours so
// the hourly scheduled-task ping shifts).
function showCommitmentModal(commitment) {
  if (!commitment) return;
  if (document.getElementById("today-cmt-modal")) return;
  const overlay = el("div", { id: "today-cmt-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, [commitment.title || "Commitment"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));

  const meta = [];
  if (commitment.due_at) meta.push(`Due: ${commitment.due_at}`);
  if (commitment.priority) meta.push(`Priority: ${commitment.priority}`);
  if (commitment.surface) meta.push(`Captured from: ${commitment.surface}`);
  if (meta.length) {
    modal.appendChild(el("div", { class: "modal-meta" }, [meta.join(" · ")]));
  }
  if (commitment.notes) {
    modal.appendChild(el("div", { class: "modal-pad" }, [
      el("div", { class: "settings-meta" }, [commitment.notes]),
    ]));
  }

  const tickBtn = el("button", { class: "button" }, ["Mark done"]);
  const pushBtn = el("button", { class: "button button-secondary" }, ["Push 4 hours"]);
  const cancelBtn = el("button", { class: "button button-secondary" }, ["Close"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, pushBtn, tickBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  tickBtn.addEventListener("click", () => {
    // v0.40 — optimistic: drop the row from local state, close modal,
    // fire backend in background. On failure, revert and toast.
    const snapshot = optimisticRemoveCommitment(commitment.id);
    close();
    showToast("Commitment ticked");
    invoke("update_airtable_record", {
      args: {
        table: "Commitments",
        record_id: commitment.id,
        fields: { status: "done" },
      },
    }).catch((e) => {
      restoreOptimisticSnapshot(snapshot);
      showToast(`Failed to save. Try again. (${e})`, { ttl: 5000 });
    });
  });

  pushBtn.addEventListener("click", () => {
    const next = new Date(Date.now() + 4 * 60 * 60 * 1000).toISOString();
    close();
    showToast("Pushed 4 hours");
    invoke("update_airtable_record", {
      args: {
        table: "Commitments",
        record_id: commitment.id,
        fields: { next_check_at: next },
      },
    }).catch((e) => {
      showToast(`Failed to save. Try again. (${e})`, { ttl: 5000 });
    });
  });
}

// v0.40 — local optimistic state helpers. Each returns a snapshot that
// restoreOptimisticSnapshot can rewind on failure. Renders happen
// synchronously so the UI updates before the network call returns.
function optimisticRemoveCommitment(recordId) {
  const before = {
    today_due: _state.today.due_today?.commitments
      ? [..._state.today.due_today.commitments]
      : null,
    today_overdue: _state.today.overdue ? [..._state.today.overdue] : null,
    client_commitments: _state.client?.commitments
      ? [..._state.client.commitments]
      : null,
  };
  if (_state.today.due_today?.commitments) {
    _state.today.due_today.commitments = _state.today.due_today.commitments
      .filter((c) => c.id !== recordId);
  }
  if (_state.today.overdue) {
    _state.today.overdue = _state.today.overdue.filter((c) => c.id !== recordId);
  }
  if (_state.client?.commitments) {
    _state.client.commitments = _state.client.commitments.filter(
      (c) => c.id !== recordId
    );
  }
  todayRender.drawSection(_state.today, "due-today");
  todayRender.drawSection(_state.today, "overdue");
  if (_state.client?.code) clientRender.drawSection(_state.client, "commitments");
  return { kind: "commitment-remove", recordId, before };
}

function optimisticRemoveDecision(recordId) {
  const before = {
    today_open: _state.today.decisions_open ? [..._state.today.decisions_open] : null,
    today_due: _state.today.due_today?.decisions
      ? [..._state.today.due_today.decisions]
      : null,
    client_decisions: _state.client?.decisions
      ? [..._state.client.decisions]
      : null,
  };
  if (_state.today.decisions_open) {
    _state.today.decisions_open = _state.today.decisions_open.filter(
      (d) => d.id !== recordId
    );
  }
  if (_state.today.due_today?.decisions) {
    _state.today.due_today.decisions = _state.today.due_today.decisions.filter(
      (d) => d.id !== recordId
    );
  }
  if (_state.client?.decisions) {
    _state.client.decisions = _state.client.decisions.filter(
      (d) => d.id !== recordId
    );
  }
  todayRender.drawSection(_state.today, "decisions");
  todayRender.drawSection(_state.today, "due-today");
  if (_state.client?.code) clientRender.drawSection(_state.client, "decisions");
  return { kind: "decision-remove", recordId, before };
}

function optimisticRemoveWorkstream(recordId) {
  const before = {
    today_workstreams: _state.today.workstreams ? [..._state.today.workstreams] : null,
    client_workstreams: _state.client?.workstreams
      ? [..._state.client.workstreams]
      : null,
  };
  if (_state.today.workstreams) {
    _state.today.workstreams = _state.today.workstreams.filter(
      (w) => w.id !== recordId
    );
  }
  if (_state.client?.workstreams) {
    _state.client.workstreams = _state.client.workstreams.filter(
      (w) => w.id !== recordId
    );
  }
  todayRender.drawSection(_state.today, "workstreams");
  if (_state.client?.code) clientRender.drawSection(_state.client, "workstreams");
  return { kind: "workstream-remove", recordId, before };
}

function restoreOptimisticSnapshot(snapshot) {
  if (!snapshot) return;
  const { kind, before } = snapshot;
  if (kind === "commitment-remove") {
    if (before.today_due) _state.today.due_today.commitments = before.today_due;
    if (before.today_overdue) _state.today.overdue = before.today_overdue;
    if (before.client_commitments && _state.client) {
      _state.client.commitments = before.client_commitments;
    }
    todayRender.drawSection(_state.today, "due-today");
    todayRender.drawSection(_state.today, "overdue");
    if (_state.client?.code) clientRender.drawSection(_state.client, "commitments");
  } else if (kind === "decision-remove") {
    if (before.today_open) _state.today.decisions_open = before.today_open;
    if (before.today_due) _state.today.due_today.decisions = before.today_due;
    if (before.client_decisions && _state.client) {
      _state.client.decisions = before.client_decisions;
    }
    todayRender.drawSection(_state.today, "decisions");
    todayRender.drawSection(_state.today, "due-today");
    if (_state.client?.code) clientRender.drawSection(_state.client, "decisions");
  } else if (kind === "workstream-remove") {
    if (before.today_workstreams) _state.today.workstreams = before.today_workstreams;
    if (before.client_workstreams && _state.client) {
      _state.client.workstreams = before.client_workstreams;
    }
    todayRender.drawSection(_state.today, "workstreams");
    if (_state.client?.code) clientRender.drawSection(_state.client, "workstreams");
  }
}

// ─── Today: decision capture modal ───────────────────────────────────
//
// Lets Caitlin convert an open decision to a made decision in two clicks
// and capture the decision text + reasoning at the same time.
function showDecisionCaptureModal(decision) {
  if (!decision) return;
  if (document.getElementById("today-dec-modal")) return;
  const overlay = el("div", { id: "today-dec-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, [decision.title || "Decision"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));

  if (decision.due_date || decision.decision_type) {
    const meta = [];
    if (decision.due_date) meta.push(`Due ${decision.due_date}`);
    if (decision.decision_type) meta.push(`Type: ${decision.decision_type}`);
    modal.appendChild(el("div", { class: "modal-meta" }, [meta.join(" · ")]));
  }

  const decisionPad = el("div", { class: "modal-pad" }, [
    el("div", { class: "settings-label" }, ["Decision"]),
  ]);
  const decisionInput = el("textarea", {
    class: "modal-textarea",
    placeholder: "What did you decide?",
    rows: 3,
  });
  if (decision.decision) decisionInput.value = decision.decision;
  decisionPad.appendChild(decisionInput);
  modal.appendChild(decisionPad);

  const reasoningPad = el("div", { class: "modal-pad", style: "margin-top: 12px;" }, [
    el("div", { class: "settings-label" }, ["Reasoning"]),
  ]);
  const reasoningInput = el("textarea", {
    class: "modal-textarea",
    placeholder: "Why? What scenario does this assume?",
    rows: 4,
  });
  if (decision.reasoning) reasoningInput.value = decision.reasoning;
  reasoningPad.appendChild(reasoningInput);
  modal.appendChild(reasoningPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const deferBtn = el("button", { class: "button button-secondary" }, ["Defer"]);
  const saveBtn = el("button", { class: "button" }, ["Mark made"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, deferBtn, saveBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);
  decisionInput.focus();

  saveBtn.addEventListener("click", () => {
    const fields = {
      status: "made",
      decided_at: new Date().toISOString(),
      decision: decisionInput.value.trim(),
      reasoning: reasoningInput.value.trim(),
    };
    // v0.40 — optimistic: drop from open list, close, fire backend.
    const snapshot = optimisticRemoveDecision(decision.id);
    close();
    showToast("Decision filed");
    invoke("update_airtable_record", {
      args: {
        table: "Decisions",
        record_id: decision.id,
        fields,
      },
    }).catch((e) => {
      restoreOptimisticSnapshot(snapshot);
      showToast(`Failed to save. Try again. (${e})`, { ttl: 5000 });
    });
  });

  deferBtn.addEventListener("click", () => {
    const snapshot = optimisticRemoveDecision(decision.id);
    close();
    showToast("Deferred");
    invoke("update_airtable_record", {
      args: {
        table: "Decisions",
        record_id: decision.id,
        fields: { status: "deferred" },
      },
    }).catch((e) => {
      restoreOptimisticSnapshot(snapshot);
      showToast(`Failed to save. Try again. (${e})`, { ttl: 5000 });
    });
  });
}

// ─── Workstream detail modal ─────────────────────────────────────────
//
// Small modal: title, code, description, last_touch, next_action, blocker.
// Single action: mark done (sets status='done' and bumps last_touch_at).
function showWorkstreamDetailModal(workstream) {
  if (!workstream) return;
  if (document.getElementById("ws-detail-modal")) return;
  const overlay = el("div", { id: "ws-detail-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, [workstream.title || "Workstream"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));

  const metaParts = [];
  if (workstream.code) metaParts.push(workstream.code);
  if (workstream.phase) metaParts.push(`Phase: ${workstream.phase}`);
  if (workstream.status) metaParts.push(`Status: ${workstream.status}`);
  if (workstream.last_touch_at) metaParts.push(`Last touch: ${workstream.last_touch_at}`);
  if (metaParts.length) {
    modal.appendChild(el("div", { class: "modal-meta" }, [metaParts.join(" · ")]));
  }

  if (workstream.description) {
    modal.appendChild(el("div", { class: "modal-pad" }, [
      el("div", { class: "settings-meta" }, [workstream.description]),
    ]));
  }
  if (workstream.next_action) {
    modal.appendChild(el("div", { class: "modal-pad", style: "margin-top: 12px;" }, [
      el("div", { class: "settings-label" }, ["Next action"]),
      el("div", { class: "settings-meta" }, [workstream.next_action]),
    ]));
  }
  if (workstream.blocker) {
    modal.appendChild(el("div", { class: "modal-pad", style: "margin-top: 12px;" }, [
      el("div", { class: "settings-label" }, ["Blocker"]),
      el("div", { class: "settings-meta" }, [workstream.blocker]),
    ]));
  }

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Close"]);
  const doneBtn = el("button", { class: "button" }, ["Mark done"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, doneBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  doneBtn.addEventListener("click", () => {
    if (!isTauri) {
      showToast("Open the Companion app to update Airtable from here.");
      return;
    }
    // v0.40 — optimistic: drop the workstream from local state, close,
    // fire backend in background. Best-effort archive runs after the
    // primary write succeeds.
    const snapshot = optimisticRemoveWorkstream(workstream.id);
    close();
    showToast("Workstream marked done");
    invoke("update_airtable_record", {
      args: {
        table: "Workstreams",
        record_id: workstream.id,
        fields: {
          status: "done",
          last_touch_at: new Date().toISOString(),
        },
      },
    }).then(async () => {
      // Best-effort archive of any bound chat. Never reverts the
      // mark-done — if archive fails we just log and move on.
      if (!workstream.code) return;
      try {
        await invoke("archive_conversation", { workstreamCode: workstream.code });
      } catch (e) {
        console.warn("archive_conversation failed:", e);
      }
      if (
        _chatActive &&
        _state.conversation.workstream_code === workstream.code
      ) {
        try {
          await convFetch.loadConversation(_state.conversation, workstream.code);
          convRender.draw(_state.conversation, _state.client.workstreams || []);
        } catch (e) {
          console.warn("conversation reload after archive failed:", e);
        }
      }
    }).catch((e) => {
      restoreOptimisticSnapshot(snapshot);
      showToast(`Failed to save. Try again. (${e})`, { ttl: 5000 });
    });
  });
}

// ─── Project detail modal ────────────────────────────────────────────
//
// Read-only summary: code, name, type, status, dates, budget, brief link.
// To edit, use the Edit project workflow on the per-client view.
function showProjectDetailModal(project) {
  if (!project) return;
  if (document.getElementById("project-detail-modal")) return;
  const overlay = el("div", { id: "project-detail-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, [project.name || project.code || "Project"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));

  const meta = [];
  if (project.code) meta.push(project.code);
  if (project.campaign_type) meta.push(project.campaign_type);
  if (project.status) meta.push(project.status);
  if (meta.length) {
    modal.appendChild(el("div", { class: "modal-meta" }, [meta.join(" · ")]));
  }

  const grid = el("div", { class: "modal-pad" });
  function row(label, value) {
    if (!value) return null;
    return el("div", { class: "settings-row", style: "margin-bottom: 6px;" }, [
      el("div", { class: "settings-label", style: "min-width: 120px;" }, [label]),
      el("div", { class: "settings-meta" }, [String(value)]),
    ]);
  }
  [
    row("Start", project.start_date),
    row("End", project.end_date),
    row("Budget", project.budget_total),
    row("Notes", project.notes),
  ].forEach((r) => { if (r) grid.appendChild(r); });
  modal.appendChild(grid);

  if (project.brief_link) {
    modal.appendChild(el("div", { class: "modal-pad" }, [
      el("a", {
        href: project.brief_link,
        target: "_blank",
        rel: "noopener",
      }, ["Open brief"]),
    ]));
  }

  const closeBtn = el("button", { class: "button" }, ["Close"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [closeBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  closeBtn.addEventListener("click", close);
}

// ─── v0.21 pure-Airtable workflows ──────────────────────────────────
//
// Three workflows that don't hit Anthropic — straight forms that file rows
// to Airtable and leave a Receipt for traceability. Modal shape mirrors
// showSubcontractorOnboardingModal etc., but the submit handler invokes
// create_social_post / create_time_log / update_project on the Rust side.

// Pull active clients from Airtable for picker dropdowns.
async function fetchClientsForPicker() {
  if (!isTauri) return [];
  try {
    const raw = await invoke("list_airtable_clients");
    const data = JSON.parse(raw);
    return (data.records || [])
      .map((r) => ({
        code: r.fields?.code || "",
        name: r.fields?.name || r.fields?.code || "Untitled",
      }))
      .filter((c) => c.code);
  } catch (e) {
    console.warn("list_airtable_clients failed:", e);
    return [];
  }
}

async function fetchProjectsForPicker(clientCode) {
  if (!isTauri) return [];
  try {
    const raw = await invoke("list_airtable_projects");
    const data = JSON.parse(raw);
    const all = (data.records || []).map((r) => ({
      record_id: r.id,
      code: r.fields?.code || "",
      name: r.fields?.name || r.fields?.code || "Untitled",
      status: r.fields?.status || "",
      campaign_type: r.fields?.campaign_type || "",
      start_date: r.fields?.start_date || "",
      end_date: r.fields?.end_date || "",
      budget_total: r.fields?.budget_total ?? null,
      notes: r.fields?.notes || "",
    }));
    if (!clientCode) return all;
    const upper = clientCode.toUpperCase();
    return all.filter((p) => (p.code || "").toUpperCase().startsWith(upper));
  } catch (e) {
    console.warn("list_airtable_projects failed:", e);
    return [];
  }
}

async function fetchSubcontractorsForPicker() {
  if (!isTauri) return [];
  try {
    const raw = await invoke("list_airtable_subcontractors");
    const data = JSON.parse(raw);
    return (data.records || []).map((r) => ({
      code: r.fields?.code || "",
      name: r.fields?.name || r.fields?.code || "Untitled",
      hourly_rate: r.fields?.hourly_rate ?? null,
    })).filter((s) => s.code);
  } catch (e) {
    console.warn("list_airtable_subcontractors failed:", e);
    return [];
  }
}

// ─── Schedule social post modal (v0.21) ───────────────────────────────
async function showScheduleSocialPostModal(prefillClientCode, opts = {}) {
  if (document.getElementById("ssp-modal")) return;

  const clients = await fetchClientsForPicker();

  const overlay = el("div", { id: "ssp-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["Schedule social post"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Drafts a row in SocialPosts. No Claude call — just files the post and a Receipt for the trail.",
  ]));

  const grid = el("div", { class: "modal-field-grid" });

  const platform = el("select", { class: "settings-input" });
  ["Instagram", "LinkedIn", "Threads", "Facebook", "TikTok", "X", "Other"].forEach((p) => {
    const opt = document.createElement("option");
    opt.value = p;
    opt.textContent = p;
    platform.appendChild(opt);
  });

  const channel = el("select", { class: "settings-input" });
  ["incahoots-marketing", "personal", "client"].forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c;
    opt.textContent = c;
    channel.appendChild(opt);
  });
  if (prefillClientCode) channel.value = "client";

  const scheduledAt = el("input", { type: "datetime-local", class: "settings-input" });
  const status = el("select", { class: "settings-input" });
  ["draft", "scheduled", "published", "cancelled"].forEach((s) => {
    const opt = document.createElement("option");
    opt.value = s;
    opt.textContent = s;
    status.appendChild(opt);
  });
  status.value = "draft";

  const approval = el("select", { class: "settings-input" });
  ["pending", "approved", "rejected"].forEach((s) => {
    const opt = document.createElement("option");
    opt.value = s;
    opt.textContent = s;
    approval.appendChild(opt);
  });

  const clientPicker = el("select", { class: "settings-input" });
  const noClient = document.createElement("option");
  noClient.value = "";
  noClient.textContent = "(none)";
  clientPicker.appendChild(noClient);
  clients.forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c.code;
    opt.textContent = `${c.code} — ${c.name}`;
    clientPicker.appendChild(opt);
  });
  if (prefillClientCode && clients.some((c) => c.code === prefillClientCode)) {
    clientPicker.value = prefillClientCode;
  }

  const imagePath = el("input", {
    type: "url",
    class: "settings-input",
    placeholder: "https://... or Dropbox link (optional)",
  });

  [
    ["Platform", platform],
    ["Channel", channel],
    ["Scheduled at", scheduledAt],
    ["Status", status],
    ["Approval", approval],
    ["Client (optional)", clientPicker],
    ["Image path (optional)", imagePath],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });
  modal.appendChild(grid);

  const copyPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Copy"]),
  ]);
  const copy = el("textarea", {
    class: "modal-textarea",
    placeholder: "Full caption. First 60 chars become the title if you don't set one.",
    rows: 6,
  });
  if (typeof opts.prefillCopy === "string" && opts.prefillCopy.trim()) {
    copy.value = opts.prefillCopy;
  }
  copyPad.appendChild(copy);
  modal.appendChild(copyPad);

  if (typeof opts.prefillPlatform === "string") {
    const want = opts.prefillPlatform;
    for (const o of platform.options) {
      if (o.value.toLowerCase() === want.toLowerCase()) {
        platform.value = o.value;
        break;
      }
    }
  }
  if (opts.prefillChannel === "incahoots-marketing") {
    channel.value = "incahoots-marketing";
  }

  const notesPad = el("div", { class: "modal-pad", style: "margin-top: 12px;" }, [
    el("div", { class: "settings-label" }, ["Notes (optional)"]),
  ]);
  const notes = el("textarea", { class: "modal-textarea", rows: 3 });
  notesPad.appendChild(notes);
  modal.appendChild(notesPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Schedule"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  copy.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    if (!isTauri) {
      showToast("Open the Companion app to file workflows. Preview is read-only.");
      return;
    }
    const copyText = copy.value.trim();
    if (!copyText) return showToast("Copy required");

    const payload = {
      title: null,
      platform: platform.value,
      // datetime-local is ISO without timezone — Airtable accepts it as
      // the configured (Australia/Melbourne) zone.
      scheduled_at: scheduledAt.value || null,
      status: status.value,
      channel: channel.value,
      client_code: clientPicker.value || null,
      copy: copyText,
      image_path: imagePath.value.trim() || null,
      approval_status: approval.value,
      notes: notes.value.trim() || null,
    };

    runBtn.disabled = true;
    runBtn.textContent = "Filing...";
    try {
      const recordId = await invoke("create_social_post", { payload });
      close();
      showToast(`Filed to SocialPosts (${recordId})`, { ttl: 4000 });
      // Refresh receipts on whichever surface is visible.
      if (_state.client?.code) {
        await clientFetch.loadReceipts(_state.client);
        clientRender.draw(_state.client);
      }
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Schedule";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// ─── Log time modal (v0.21) ──────────────────────────────────────────
async function showLogTimeModal(prefillClientCode) {
  if (document.getElementById("logtime-modal")) return;

  const [clients, subs, allProjects] = await Promise.all([
    fetchClientsForPicker(),
    fetchSubcontractorsForPicker(),
    fetchProjectsForPicker(null),
  ]);

  const overlay = el("div", { id: "logtime-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["Log time"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Hours worked → TimeLogs. Used for billing reconciliation.",
  ]));

  const grid = el("div", { class: "modal-field-grid" });

  const today = new Date();
  const isoToday = today.toISOString().slice(0, 10);
  const dateInput = el("input", { type: "date", class: "settings-input", value: isoToday });
  const hours = el("input", {
    type: "number",
    step: "0.25",
    min: "0",
    max: "24",
    class: "settings-input",
    placeholder: "e.g. 1.5",
  });

  const subPicker = el("select", { class: "settings-input" });
  const noSub = document.createElement("option");
  noSub.value = "";
  noSub.textContent = "(none)";
  subPicker.appendChild(noSub);
  subs.forEach((s) => {
    const opt = document.createElement("option");
    opt.value = s.code;
    opt.textContent = `${s.code} — ${s.name}`;
    if (s.hourly_rate != null) opt.dataset.rate = String(s.hourly_rate);
    subPicker.appendChild(opt);
  });
  // Default to Caitlin if she's in the list (any code starting with CR or CAI).
  const caitlinSub = subs.find((s) => /^(CR|CAI)/i.test(s.code));
  if (caitlinSub) subPicker.value = caitlinSub.code;

  const clientPicker = el("select", { class: "settings-input" });
  const noClient = document.createElement("option");
  noClient.value = "";
  noClient.textContent = "(none)";
  clientPicker.appendChild(noClient);
  clients.forEach((c) => {
    const opt = document.createElement("option");
    opt.value = c.code;
    opt.textContent = `${c.code} — ${c.name}`;
    clientPicker.appendChild(opt);
  });
  if (prefillClientCode && clients.some((c) => c.code === prefillClientCode)) {
    clientPicker.value = prefillClientCode;
  }

  const projectPicker = el("select", { class: "settings-input" });
  function refreshProjects() {
    projectPicker.innerHTML = "";
    const noProj = document.createElement("option");
    noProj.value = "";
    noProj.textContent = "(none)";
    projectPicker.appendChild(noProj);
    const code = clientPicker.value;
    const filtered = code
      ? allProjects.filter((p) => (p.code || "").toUpperCase().startsWith(code.toUpperCase()))
      : allProjects;
    filtered.forEach((p) => {
      const opt = document.createElement("option");
      opt.value = p.code;
      opt.textContent = `${p.code} — ${p.name}`;
      projectPicker.appendChild(opt);
    });
  }
  refreshProjects();
  clientPicker.addEventListener("change", refreshProjects);

  const billable = el("input", { type: "checkbox", checked: true });
  const billableWrap = el("label", { style: "display:flex;align-items:center;gap:8px;" }, [
    billable,
    el("span", {}, ["Billable"]),
  ]);

  const rate = el("input", {
    type: "number",
    step: "0.01",
    min: "0",
    class: "settings-input",
    placeholder: "auto from picker",
  });

  function setRateFromSub() {
    const opt = subPicker.options[subPicker.selectedIndex];
    if (!opt) return;
    if (opt.dataset.rate) {
      rate.value = opt.dataset.rate;
    } else if (/^(CR|CAI)/i.test(opt.value)) {
      rate.value = "110";
    } else if (/^(ROS|RG)/i.test(opt.value)) {
      rate.value = "66";
    }
  }
  setRateFromSub();
  subPicker.addEventListener("change", setRateFromSub);

  [
    ["Date", dateInput],
    ["Hours", hours],
    ["Who", subPicker],
    ["Client", clientPicker],
    ["Project (optional)", projectPicker],
    ["Rate ($AUD/hr)", rate],
    ["", billableWrap],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label || " "]),
      input,
    ]));
  });
  modal.appendChild(grid);

  const taskPad = el("div", { class: "modal-pad", style: "margin-top: 16px;" }, [
    el("div", { class: "settings-label" }, ["Task description"]),
  ]);
  const task = el("textarea", {
    class: "modal-textarea",
    placeholder: "What you did. Bullet points are fine.",
    rows: 4,
  });
  taskPad.appendChild(task);
  modal.appendChild(taskPad);

  const notesPad = el("div", { class: "modal-pad", style: "margin-top: 12px;" }, [
    el("div", { class: "settings-label" }, ["Notes (optional)"]),
  ]);
  const notes = el("textarea", { class: "modal-textarea", rows: 2 });
  notesPad.appendChild(notes);
  modal.appendChild(notesPad);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Log"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  hours.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    if (!isTauri) {
      showToast("Open the Companion app to file workflows. Preview is read-only.");
      return;
    }
    const hoursVal = parseFloat(hours.value);
    if (!hoursVal || hoursVal <= 0) return showToast("Hours required (> 0)");

    const payload = {
      date: dateInput.value || null,
      hours: hoursVal,
      subcontractor_code: subPicker.value || null,
      client_code: clientPicker.value || null,
      project_code: projectPicker.value || null,
      task_description: task.value.trim() || null,
      billable: billable.checked,
      rate: rate.value ? parseFloat(rate.value) : null,
      notes: notes.value.trim() || null,
    };

    runBtn.disabled = true;
    runBtn.textContent = "Filing...";
    try {
      const recordId = await invoke("create_time_log", { payload });
      close();
      showToast(`Filed ${hoursVal}h to TimeLogs (${recordId})`, { ttl: 4000 });
      if (_state.client?.code) {
        await clientFetch.loadReceipts(_state.client);
        clientRender.draw(_state.client);
      }
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Log";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// ─── Edit project modal (v0.21) ──────────────────────────────────────
//
// Two-step flow: first pick a project, then edit fields. The diff is
// captured into the Receipt so a future audit can replay what changed.
async function showEditProjectModal(prefillClientCode) {
  if (document.getElementById("editproj-modal")) return;

  const projects = await fetchProjectsForPicker(prefillClientCode);
  if (projects.length === 0) {
    showToast(
      prefillClientCode
        ? `No projects on ${prefillClientCode}. Create one first.`
        : "No projects in Airtable yet.",
      { ttl: 5000 }
    );
    return;
  }

  const overlay = el("div", { id: "editproj-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, ["Edit project"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));
  modal.appendChild(el("div", { class: "modal-meta" }, [
    "Pick a project, change what's changed. Diff is captured into a Receipt.",
  ]));

  // Step 1 container: project picker.
  const pickerWrap = el("div", { class: "modal-pad" });
  pickerWrap.appendChild(el("div", { class: "settings-label" }, ["Project"]));
  const picker = el("select", { class: "settings-input" });
  projects.forEach((p) => {
    const opt = document.createElement("option");
    opt.value = p.record_id;
    opt.textContent = `${p.code} — ${p.name}`;
    picker.appendChild(opt);
  });
  pickerWrap.appendChild(picker);
  modal.appendChild(pickerWrap);

  // Step 2 container: editable fields. Populated by repopulate().
  const editWrap = el("div", { class: "modal-pad", style: "margin-top: 12px;" });
  modal.appendChild(editWrap);

  const nameInput = el("input", { type: "text", class: "settings-input" });
  const statusSel = el("select", { class: "settings-input" });
  ["scoping", "active", "paused", "complete", "archive"].forEach((s) => {
    const opt = document.createElement("option");
    opt.value = s;
    opt.textContent = s;
    statusSel.appendChild(opt);
  });
  const typeSel = el("select", { class: "settings-input" });
  ["", "festival", "venue", "touring", "label", "artist", "PR", "comedy", "capability"].forEach((t) => {
    const opt = document.createElement("option");
    opt.value = t;
    opt.textContent = t || "(unset)";
    typeSel.appendChild(opt);
  });
  const startInput = el("input", { type: "date", class: "settings-input" });
  const endInput = el("input", { type: "date", class: "settings-input" });
  const budgetInput = el("input", { type: "number", step: "0.01", min: "0", class: "settings-input" });
  const notesInput = el("textarea", { class: "modal-textarea", rows: 4 });

  const grid = el("div", { class: "modal-field-grid" });
  [
    ["Name", nameInput],
    ["Status", statusSel],
    ["Campaign type", typeSel],
    ["Start date", startInput],
    ["End date", endInput],
    ["Budget total", budgetInput],
  ].forEach(([label, input]) => {
    grid.appendChild(el("label", { class: "modal-field" }, [
      el("div", { class: "settings-label" }, [label]),
      input,
    ]));
  });
  editWrap.appendChild(grid);

  const notesPad = el("div", { style: "margin-top: 12px;" }, [
    el("div", { class: "settings-label" }, ["Notes"]),
    notesInput,
  ]);
  editWrap.appendChild(notesPad);

  function repopulate() {
    const project = projects.find((p) => p.record_id === picker.value);
    if (!project) return;
    nameInput.value = project.name || "";
    statusSel.value = project.status || "scoping";
    typeSel.value = project.campaign_type || "";
    startInput.value = project.start_date || "";
    endInput.value = project.end_date || "";
    budgetInput.value = project.budget_total != null ? project.budget_total : "";
    notesInput.value = project.notes || "";
  }
  repopulate();
  picker.addEventListener("change", repopulate);

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Save"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  picker.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    if (!isTauri) {
      showToast("Open the Companion app to file workflows. Preview is read-only.");
      return;
    }
    const recordId = picker.value;
    if (!recordId) return showToast("Pick a project first");

    const fields = {
      name: nameInput.value.trim(),
      status: statusSel.value,
      campaign_type: typeSel.value,
      start_date: startInput.value,
      end_date: endInput.value,
      budget_total: budgetInput.value ? parseFloat(budgetInput.value) : null,
      notes: notesInput.value,
    };

    runBtn.disabled = true;
    runBtn.textContent = "Saving...";
    try {
      await invoke("update_project", { recordId, fields });
      close();
      showToast("Project updated, diff filed", { ttl: 4000 });
      if (_state.client?.code) {
        await clientFetch.loadProjects(_state.client);
        await clientFetch.loadReceipts(_state.client);
        clientRender.draw(_state.client);
      }
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Save";
      showToast(`Error: ${e.message || e}`, { ttl: 6000 });
    }
  });
}

// ─── Receipt JSON viewer modal ───────────────────────────────────────
//
// Used by the per-client view's recent receipts list. Receipts table rows
// include the raw JSON payload — show it in a pre block so Caitlin can
// inspect the receipt without leaving Studio.
function showReceiptJsonModal(receipt) {
  if (!receipt) return;
  if (document.getElementById("receipt-json-modal")) return;
  const overlay = el("div", { id: "receipt-json-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal modal-wide" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, [receipt.title || "Receipt"]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));

  const meta = [];
  if (receipt.workflow) meta.push(receipt.workflow);
  if (receipt.date) meta.push(receipt.date);
  meta.push(`${receipt.ticked} of ${receipt.total} ticked`);
  modal.appendChild(el("div", { class: "modal-meta" }, [meta.join(" · ")]));

  let pretty = receipt.json || "(no payload)";
  try {
    if (receipt.json) pretty = JSON.stringify(JSON.parse(receipt.json), null, 2);
  } catch {
    // leave as-is
  }
  const pre = el("pre", { class: "modal-pre" }, [pretty]);
  modal.appendChild(pre);

  const closeBtn = el("button", { class: "button" }, ["Close"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [closeBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  closeBtn.addEventListener("click", close);
}

// v0.36 Block F — search result → existing surface dispatch.
//
// Each result carries a `jump_to` envelope shaped on the Rust side. We
// route by kind: receipts open the receipt JSON modal, decisions and
// commitments open their respective capture/detail modals, project
// notes load the per-project view (filtered scroll lands in v0.37 if
// the per-project feed grows that capability), and conversations load
// the chat surface for their workstream code.
async function handleSearchJump(result) {
  if (!result || !result.jump_to) return;
  const j = result.jump_to;
  const kind = j.kind;

  if (kind === "receipt") {
    // showReceiptJsonModal expects { title, workflow, date, ticked, total, json }.
    showReceiptJsonModal({
      title: j.title || result.title,
      workflow: j.workflow,
      date: j.date,
      ticked: j.ticked || 0,
      total: j.total || 0,
      json: j.json,
      summary: j.summary,
    });
    return;
  }

  if (kind === "decision") {
    showDecisionCaptureModal({
      id: j.record_id,
      title: j.title || result.title,
      decision: j.decision,
      reasoning: j.reasoning,
      due_date: j.due_date,
      decision_type: j.decision_type,
      status: j.status,
    });
    return;
  }

  if (kind === "commitment") {
    showCommitmentModal({
      id: j.record_id,
      title: j.title || result.title,
      notes: j.notes,
      due_at: j.due_at,
      priority: j.priority,
      surface: j.surface,
    });
    return;
  }

  if (kind === "conversation") {
    const wsCode = j.workstream_code;
    const clientCode = j.client_code;
    if (!wsCode) {
      showToast("Conversation has no workstream code — can't jump.");
      return;
    }
    if (clientCode) {
      activateClientSidebar(clientCode);
      await loadClientView(clientCode);
    }
    // loadChatView wants a workstream-shaped object. Title falls back to code.
    loadChatView({ code: wsCode, title: result.title || wsCode });
    return;
  }

  if (kind === "project_note") {
    const projectCode = j.project_code;
    if (!projectCode) {
      showToast("Note has no project code — can't jump.");
      return;
    }
    await loadProjectView(projectCode, { preloadClient: true });
    return;
  }

  showToast(`Unknown jump target: ${kind}`);
}

// Rebuild the client-id → code lookup. Kept tiny so post-mutation refreshes
// don't pay a full reload cost.
async function rebuildClientLookup() {
  if (!isTauri) return {};
  try {
    const raw = await invoke("list_airtable_clients");
    const data = JSON.parse(raw);
    const lookup = {};
    (data.records || []).forEach((r) => {
      const code = r.fields?.code;
      if (code) lookup[r.id] = code;
    });
    return lookup;
  } catch {
    return {};
  }
}

function activateClientSidebar(clientCode) {
  document.querySelectorAll(".sidebar-item").forEach((i) => {
    i.classList.remove("active");
    i.removeAttribute("aria-current");
  });
  const target = document.querySelector(`.sidebar-item[data-client-code="${clientCode}"]`);
  if (target) {
    target.classList.add("active");
    target.setAttribute("aria-current", "page");
  }
}

// ─── Form helpers (v0.30) ────────────────────────────────────────────
//
// pickProject: cheap modal that returns the chosen project. Auto-picks
// when there's only one. Returns null on cancel.
function pickProject(projects, formLabel) {
  return new Promise((resolve) => {
    if (!Array.isArray(projects) || projects.length === 0) {
      resolve(null);
      return;
    }
    if (projects.length === 1) {
      resolve(projects[0]);
      return;
    }
    const overlay = el("div", { class: "modal-overlay" });
    const modal = el("div", { class: "modal" });
    const close = (val) => {
      overlay.remove();
      resolve(val);
    };
    modal.appendChild(el("div", { class: "modal-header" }, [
      el("div", { class: "modal-title" }, [`Pick a project for ${formLabel}`]),
      el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
    ]));
    const list = el("div", { class: "modal-body modal-body-list" });
    projects.forEach((p) => {
      const row = el("button", {
        class: "client-shortcut",
        type: "button",
      }, [
        el("div", { class: "client-shortcut-title" }, [p.name || p.code || "(untitled)"]),
        el("div", { class: "client-shortcut-meta" }, [
          [p.code, p.status, p.campaign_type].filter(Boolean).join(" · "),
        ]),
      ]);
      row.addEventListener("click", () => close(p));
      list.appendChild(row);
    });
    modal.appendChild(list);
    modal.appendChild(el("div", { class: "modal-actions" }, [
      el("button", { class: "button button-secondary", id: "form-pick-cancel" }, ["Cancel"]),
    ]));
    overlay.appendChild(modal);
    document.body.appendChild(overlay);
    overlay.addEventListener("click", (e) => { if (e.target === overlay) close(null); });
    modal.querySelector(".modal-close").addEventListener("click", () => close(null));
    document.getElementById("form-pick-cancel").addEventListener("click", () => close(null));
  });
}

// Body of the email draft for a given form. Plain text, signed off
// with Caitlin's voice — short, friendly, no marketing fluff. Mailto
// will percent-encode line breaks for us.
function buildFormEmailBody(formKey, ctx) {
  const projectLabel = ctx.project?.name || ctx.project?.code || "the project";
  if (formKey === "form_discovery_pre_brief") {
    return [
      "Hi,",
      "",
      `Before our discovery call I'd love a few quick answers — should take 5 minutes. It helps me come in already up to speed for ${projectLabel}.`,
      "",
      ctx.url,
      "",
      "Thanks!",
      "Caitlin",
    ].join("\n");
  }
  if (formKey === "form_post_campaign_feedback") {
    return [
      "Hi,",
      "",
      `Now that ${projectLabel} has wrapped, would you mind dropping me a quick bit of feedback? It helps me improve the next one and shape what we do together going forward.`,
      "",
      ctx.url,
      "",
      "Thanks for the run, much appreciated.",
      "Caitlin",
    ].join("\n");
  }
  if (formKey === "form_content_approval") {
    return [
      "Hi,",
      "",
      "A draft post for your sign-off. The form has the copy and image attached — tick approve, request changes, or leave a comment.",
      "",
      ctx.url,
      "",
      "Cheers,",
      "Caitlin",
    ].join("\n");
  }
  return ctx.url || "";
}

// Inline toast (replaces alert()).
function showToast(message, opts = {}) {
  let root = document.getElementById("toast-root");
  if (!root) {
    root = document.createElement("div");
    root.id = "toast-root";
    document.body.appendChild(root);
  }
  const toast = document.createElement("div");
  toast.className = "toast";
  toast.textContent = message;
  root.appendChild(toast);
  requestAnimationFrame(() => toast.classList.add("toast-show"));
  setTimeout(() => {
    toast.classList.remove("toast-show");
    setTimeout(() => toast.remove(), 200);
  }, opts.ttl ?? 2800);
}

// ─── View switcher (Today vs per-client) ─────────────────────────────
function showTodayView() {
  // Drop chat / project surface so re-entering #client-view later starts
  // from the standard layout.
  _chatActive = false;
  _projectActive = false;
  convRender.exitChat();
  projectRender.exitProject();

  document.getElementById("today-view")?.removeAttribute("hidden");
  document.getElementById("client-view")?.setAttribute("hidden", "");
  // Drop any pipeline view if it was open.
  document.getElementById("pipeline-view")?.setAttribute("hidden", "");
  // Drop the Studio CFO view if it was open (v0.37).
  document.getElementById("studio-cfo-view")?.setAttribute("hidden", "");
  // Drop the per-Subcontractor view if it was open (v0.38).
  document.getElementById("subcontractor-view")?.setAttribute("hidden", "");
  // Drop any skills-only views (Team, Social launch) if open.
  document
    .querySelectorAll('[data-view-kind="skills-only"]')
    .forEach((n) => n.setAttribute("hidden", ""));
  const titleEl = document.querySelector(".main-title");
  if (titleEl) titleEl.textContent = "Today";
  // v0.40 — drop any client/project pollers from the previous view.
  poller.unregisterPrefix("client.");
  poller.unregisterPrefix("project.");
  // Re-render with whatever's already in state, then refresh stale slices.
  todayRender.draw(_state.today);
  todayFetch.refreshStale(_state.today).then(() => todayRender.draw(_state.today));
  registerTodayPollers();
}

// v0.34 Skills restructure: minimal "Skills only" view used for Team
// and Personal: Social launch. Both hold a single Skills section pulled
// from the registry (filtered by context). No live data slots yet —
// future versions can add team summaries, post performance, etc.
function showSkillsOnlyView({ id, title, emoji, intro, context }) {
  _chatActive = false;
  _projectActive = false;
  convRender.exitChat();
  projectRender.exitProject();
  clearLiveStatusTimer();
  clearCalendarTimer();

  document.getElementById("today-view")?.setAttribute("hidden", "");
  document.getElementById("client-view")?.setAttribute("hidden", "");
  document.getElementById("pipeline-view")?.setAttribute("hidden", "");
  document.getElementById("studio-cfo-view")?.setAttribute("hidden", "");
  document.getElementById("subcontractor-view")?.setAttribute("hidden", "");

  const viewId = `skills-only-${id}`;
  // Hide every previously-rendered skills-only view container so we don't
  // stack them.
  document
    .querySelectorAll('[data-view-kind="skills-only"]')
    .forEach((n) => n.setAttribute("hidden", ""));

  let view = document.getElementById(viewId);
  if (!view) {
    view = document.createElement("section");
    view.id = viewId;
    view.dataset.viewKind = "skills-only";
    document.querySelector("main.main")?.appendChild(view);
  }
  view.removeAttribute("hidden");
  view.innerHTML = "";

  const header = document.createElement("header");
  header.className = "main-header";
  header.appendChild(
    Object.assign(document.createElement("div"), {
      className: "main-title",
      textContent: title,
    })
  );
  view.appendChild(header);

  if (intro) {
    const introEl = document.createElement("section");
    introEl.className = "today-section";
    const labelEl = document.createElement("div");
    labelEl.className = "today-section-meta";
    labelEl.textContent = intro;
    introEl.appendChild(labelEl);
    view.appendChild(introEl);
  }

  const skillsSection = document.createElement("section");
  skillsSection.className = "today-section";
  const sectionLabel = document.createElement("div");
  sectionLabel.className = "section-label";
  sectionLabel.textContent = `${emoji || "🛠"} SKILLS`;
  skillsSection.appendChild(sectionLabel);

  const skills = skillsForContext(context);
  if (skills.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "No skills available here yet.";
    skillsSection.appendChild(empty);
  } else {
    const grid = document.createElement("div");
    grid.className = "client-shortcut-grid";
    skills.forEach((s) => {
      const card = document.createElement("button");
      card.type = "button";
      card.className =
        "client-shortcut" + (s.placeholder ? " client-shortcut-placeholder" : "");
      card.dataset.skillId = s.id;
      const cardTitle = document.createElement("div");
      cardTitle.className = "client-shortcut-title";
      cardTitle.textContent = s.label;
      const cardMeta = document.createElement("div");
      cardMeta.className = "client-shortcut-meta";
      cardMeta.textContent = s.description || "";
      card.appendChild(cardTitle);
      card.appendChild(cardMeta);
      card.addEventListener("click", () => {
        document.dispatchEvent(
          new CustomEvent("skill:dispatch", { detail: { skill_id: s.id } })
        );
      });
      grid.appendChild(card);
    });
    skillsSection.appendChild(grid);
  }
  view.appendChild(skillsSection);
}

// v0.30 Block F: minimal Pipeline view. Shows the "Share Lead Intake
// form" button as the headline action. Future versions will surface
// active leads, lead status, and value estimates.
async function showPipelineView() {
  _chatActive = false;
  _projectActive = false;
  convRender.exitChat();
  projectRender.exitProject();
  clearLiveStatusTimer();
  clearCalendarTimer();

  document.getElementById("today-view")?.setAttribute("hidden", "");
  document.getElementById("client-view")?.setAttribute("hidden", "");
  document.getElementById("studio-cfo-view")?.setAttribute("hidden", "");
  document.getElementById("subcontractor-view")?.setAttribute("hidden", "");
  document
    .querySelectorAll('[data-view-kind="skills-only"]')
    .forEach((n) => n.setAttribute("hidden", ""));

  let view = document.getElementById("pipeline-view");
  if (!view) {
    view = document.createElement("section");
    view.id = "pipeline-view";
    document.querySelector("main.main")?.appendChild(view);
  }
  view.removeAttribute("hidden");
  view.innerHTML = "";

  const header = document.createElement("header");
  header.className = "main-header";
  header.appendChild(
    Object.assign(document.createElement("div"), {
      className: "main-title",
      textContent: "Pipeline",
    })
  );
  view.appendChild(header);

  const intro = document.createElement("section");
  intro.className = "today-section";
  const label = document.createElement("div");
  label.className = "section-label";
  label.textContent = "📥 LEAD INTAKE";
  intro.appendChild(label);

  const blurb = document.createElement("div");
  blurb.className = "today-section-meta";
  blurb.textContent =
    "Public Airtable Form. Share with prospects. New entries land in Leads at status = cold, ready for triage.";
  intro.appendChild(blurb);

  const buttonRow = document.createElement("div");
  buttonRow.className = "today-list pipeline-actions";
  buttonRow.style.flexDirection = "row";
  buttonRow.style.gap = "8px";

  const shareBtn = document.createElement("button");
  shareBtn.className = "button";
  shareBtn.type = "button";
  shareBtn.textContent = "Share Lead Intake form";

  const openBtn = document.createElement("button");
  openBtn.className = "button button-secondary";
  openBtn.type = "button";
  openBtn.textContent = "Open form";

  buttonRow.appendChild(shareBtn);
  buttonRow.appendChild(openBtn);
  intro.appendChild(buttonRow);

  const status = document.createElement("div");
  status.className = "today-section-meta";
  status.style.marginTop = "8px";
  intro.appendChild(status);
  view.appendChild(intro);

  // v0.33 Block F: live Leads list with per-row Promote button.
  const leadsSection = document.createElement("section");
  leadsSection.className = "today-section";
  leadsSection.dataset.section = "leads";
  leadsSection.appendChild(
    Object.assign(document.createElement("div"), {
      className: "section-label",
      textContent: "🌱 ACTIVE LEADS",
    })
  );
  const leadsList = document.createElement("div");
  leadsList.className = "today-list";
  leadsList.appendChild(
    Object.assign(document.createElement("div"), {
      className: "empty",
      textContent: "Loading leads...",
    })
  );
  leadsSection.appendChild(leadsList);
  view.appendChild(leadsSection);

  // Wire buttons. Read the URL eagerly so we can disable when unset.
  let url = "";
  try {
    if (isTauri) url = await forms.getFormUrl("form_lead_intake");
  } catch (e) {
    console.warn("getFormUrl(form_lead_intake) failed:", e);
  }
  if (!url) {
    shareBtn.disabled = true;
    openBtn.disabled = true;
    status.textContent =
      "No URL set yet. Add the Lead Intake form URL in Settings → Forms.";
  } else {
    status.textContent = "URL ready. Share copies to clipboard; Open opens it in your browser.";
  }

  shareBtn.addEventListener("click", async () => {
    if (!url) return showToast("No URL set. Add it in Settings → Forms.");
    try {
      await forms.copyToClipboard(url);
      showToast("Lead Intake URL copied to clipboard");
    } catch (e) { showToast(`Copy failed: ${e}`); }
  });
  openBtn.addEventListener("click", () => {
    if (!url) return;
    forms.openUrl(url);
  });

  // Load leads + render rows.
  await renderLeadsList(leadsList);
}

// v0.37 Block F — Studio CFO surface. Lives under the Personal sidebar
// section. Renders monthly financial intelligence (totals, per-client
// breakdown, hour-creep alerts, next-month outlook) from existing
// Airtable tables. Read-only; no writes.
// v0.38: per-Subcontractor view. Click a Subcontractor in the sidebar to
// open a view scoped to that person — header, assigned workstreams, open
// commitments, hours-this-month, recent receipts, and a Skills strip.
async function showSubcontractorView(code) {
  const upper = (code || "").trim().toUpperCase();
  if (!upper) return;

  _chatActive = false;
  _projectActive = false;
  convRender.exitChat();
  projectRender.exitProject();
  clearLiveStatusTimer();
  clearCalendarTimer();

  document.getElementById("today-view")?.setAttribute("hidden", "");
  document.getElementById("client-view")?.setAttribute("hidden", "");
  document.getElementById("pipeline-view")?.setAttribute("hidden", "");
  document.getElementById("studio-cfo-view")?.setAttribute("hidden", "");
  document
    .querySelectorAll('[data-view-kind="skills-only"]')
    .forEach((n) => n.setAttribute("hidden", ""));

  let view = document.getElementById("subcontractor-view");
  if (!view) {
    view = document.createElement("section");
    view.id = "subcontractor-view";
    document.querySelector("main.main")?.appendChild(view);
  }
  view.removeAttribute("hidden");

  _state.subcontractor = emptySubcontractorState(upper);
  subcontractorRender.drawLoading(view, upper);

  const titleEl = document.querySelector(".main-title");
  if (titleEl) titleEl.textContent = upper;

  if (!isTauri) {
    _state.subcontractor.error =
      "Open the Companion app to load this view. Preview is read-only.";
    subcontractorRender.draw(view, _state.subcontractor);
    return;
  }
  await subcontractorFetch.loadAll(_state.subcontractor, upper);
  subcontractorRender.draw(view, _state.subcontractor);
  if (titleEl && _state.subcontractor.header?.name) {
    titleEl.textContent = _state.subcontractor.header.name;
  }
}

async function showStudioCfoView() {
  _chatActive = false;
  _projectActive = false;
  convRender.exitChat();
  projectRender.exitProject();
  clearLiveStatusTimer();
  clearCalendarTimer();

  document.getElementById("today-view")?.setAttribute("hidden", "");
  document.getElementById("client-view")?.setAttribute("hidden", "");
  document.getElementById("pipeline-view")?.setAttribute("hidden", "");
  document.getElementById("subcontractor-view")?.setAttribute("hidden", "");
  document
    .querySelectorAll('[data-view-kind="skills-only"]')
    .forEach((n) => n.setAttribute("hidden", ""));

  let view = document.getElementById("studio-cfo-view");
  if (!view) {
    view = document.createElement("section");
    view.id = "studio-cfo-view";
    document.querySelector("main.main")?.appendChild(view);
  }
  view.removeAttribute("hidden");

  const titleEl = document.querySelector(".main-title");
  if (titleEl) titleEl.textContent = "Studio CFO";

  // Initial paint with whatever's cached, then load + repaint.
  cfoRender.draw(view, _state.cfo);
  if (isTauri) {
    _state.cfo.loading = true;
    cfoRender.draw(view, _state.cfo);
    await cfoFetch.loadAll(_state.cfo);
    cfoRender.draw(view, _state.cfo);
  } else {
    _state.cfo.error =
      "Open the Companion app to load CFO data. Preview is read-only.";
    cfoRender.draw(view, _state.cfo);
  }
}

// Render the list of active leads into the supplied container. Each row
// has client code, name, status pill and a "Promote Lead" button. Empty
// states fall back gracefully when Airtable isn't reachable.
async function renderLeadsList(container) {
  container.innerHTML = "";
  if (!isTauri) {
    container.appendChild(
      Object.assign(document.createElement("div"), {
        className: "empty",
        textContent: "Open the Companion app to see leads.",
      })
    );
    return;
  }
  let leads = [];
  try {
    const raw = await invoke("list_airtable_leads");
    const data = JSON.parse(raw);
    leads = (data.records || []).map((r) => ({
      record_id: r.id,
      code: r.fields?.code || "",
      name: r.fields?.name || "(untitled)",
      status: r.fields?.status || "",
      primary_contact_name: r.fields?.primary_contact_name || "",
      primary_contact_email: r.fields?.primary_contact_email || "",
      source: r.fields?.source || "",
      notes: r.fields?.notes || "",
    }));
  } catch (e) {
    container.appendChild(
      Object.assign(document.createElement("div"), {
        className: "empty",
        textContent: `Couldn't load leads (${e}). Check Airtable in Settings.`,
      })
    );
    return;
  }
  if (leads.length === 0) {
    container.appendChild(
      Object.assign(document.createElement("div"), {
        className: "empty",
        textContent: "No active leads. Share the intake form to fill the pipeline.",
      })
    );
    return;
  }
  leads.forEach((lead) => container.appendChild(renderLeadRow(lead)));
}

function renderLeadRow(lead) {
  const row = el("div", { class: "today-row today-row-lead" });
  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [`${lead.code || "(no code)"} — ${lead.name}`]),
    el("div", { class: "today-row-meta" }, [
      [
        lead.primary_contact_name,
        lead.primary_contact_email,
        lead.source ? `via ${lead.source}` : "",
      ].filter(Boolean).join(" · ") || "No contact details on file yet",
    ]),
  ]);
  const right = el("div", { class: "today-row-side" });
  if (lead.status) {
    right.appendChild(el("span", { class: "client-status-pill status-active" }, [lead.status]));
  }
  const btn = el("button", { class: "button button-secondary", type: "button" }, ["Promote Lead"]);
  btn.addEventListener("click", () => showPromoteLeadModal(lead));
  right.appendChild(btn);
  row.appendChild(left);
  row.appendChild(right);
  return row;
}

// ─── Promote Lead modal (v0.33 Block F) ──────────────────────────────
//
// Surfaces the cascade summary, three calendar slot suggestions, and the
// L5 confirmation buttons (calendar invite send, mark Lead won). The
// CSA upload to Dropbox Sign stays manual — the modal links to the
// drafted file.
async function showPromoteLeadModal(lead) {
  if (!isTauri) {
    showToast("Open the Companion app to promote leads.");
    return;
  }
  if (document.getElementById("promote-lead-modal")) return;

  const overlay = el("div", { id: "promote-lead-modal", class: "modal-overlay" });
  const modal = el("div", { class: "modal" });
  const close = () => overlay.remove();

  modal.appendChild(el("div", { class: "modal-header" }, [
    el("div", { class: "modal-title" }, [`Promote Lead: ${lead.name}`]),
    el("button", { class: "modal-close", "aria-label": "Close" }, ["×"]),
  ]));

  const meta = el("div", { class: "modal-meta" }, [
    "Cascades five steps. Files Receipts at L4. The calendar invite, Dropbox Sign upload and Lead status flip stay gated by your explicit click.",
  ]);
  modal.appendChild(meta);

  const status = el("div", { class: "modal-pad", style: "min-height: 60px;" }, [
    el("div", { class: "today-section-meta" }, ["Ready to run."]),
  ]);
  modal.appendChild(status);

  // Body holds the cascade summary + slot picker once the run completes.
  const body = el("div", { class: "modal-pad", style: "margin-top: 12px;" });
  modal.appendChild(body);

  const actions = el("div", { class: "modal-actions" });
  const cancelBtn = el("button", { class: "button button-secondary", type: "button" }, ["Cancel"]);
  cancelBtn.addEventListener("click", close);
  const runBtn = el("button", { class: "button", type: "button" }, ["Run cascade"]);
  actions.appendChild(cancelBtn);
  actions.appendChild(runBtn);
  modal.appendChild(actions);

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    runBtn.disabled = true;
    runBtn.textContent = "Running cascade...";
    cancelBtn.disabled = true;
    status.innerHTML = "";
    status.appendChild(el("div", { class: "today-section-meta" }, [
      "1/5 Creating Client row → 2/5 Discovery Project → 3/5 Folder template → 4/5 CSA draft → 5/5 Slot suggestions",
    ]));

    let result;
    try {
      result = await invoke("promote_lead", { args: { lead_record_id: lead.record_id } });
    } catch (e) {
      runBtn.disabled = false;
      runBtn.textContent = "Run cascade";
      cancelBtn.disabled = false;
      status.innerHTML = "";
      status.appendChild(el("div", { class: "banner banner-warn" }, [
        el("div", { class: "banner-text" }, [`Cascade halted: ${e}`]),
      ]));
      return;
    }

    runBtn.remove();
    cancelBtn.disabled = false;
    cancelBtn.textContent = "Close";
    status.innerHTML = "";
    status.appendChild(el("div", { class: "today-section-meta" }, [
      `Lead promoted: ${result.client_code} created with discovery project ${result.project_code}.`,
    ]));

    body.innerHTML = "";
    body.appendChild(renderPromoteLeadSummary(result, lead));

    // Refresh sidebar so the new client appears immediately.
    try { await loadStudioSidebar(); } catch {}
  });
}

function renderPromoteLeadSummary(result, lead) {
  const wrap = el("div", { class: "today-list" });

  // Dropbox folder.
  wrap.appendChild(el("div", { class: "today-row" }, [
    el("div", { class: "today-row-main" }, [
      el("div", { class: "today-row-title" }, ["Dropbox folder"]),
      el("div", { class: "today-row-meta" }, [result.dropbox_folder_path]),
    ]),
    el("div", { class: "today-row-side" }, [
      buildOpenLinkButton(result.dropbox_folder_url, "Open"),
    ]),
  ]));

  // CSA draft.
  wrap.appendChild(el("div", { class: "today-row" }, [
    el("div", { class: "today-row-main" }, [
      el("div", { class: "today-row-title" }, ["CSA drafted"]),
      el("div", { class: "today-row-meta" }, [
        `${result.csa_file_path} — fill scope details, then upload to Dropbox Sign and send.`,
      ]),
    ]),
  ]));

  // Slot picker.
  const slotsRow = el("div", { class: "today-row" });
  const slotsLeft = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, ["Discovery call"]),
    el("div", { class: "today-row-meta" }, [
      result.calendar_slots.length === 0
        ? "Google Calendar wasn't reachable — pick a slot manually and email the contact."
        : "Pick a 30-minute slot. Confirming opens Google Calendar with the event prefilled — click Save to send the invite (L5).",
    ]),
  ]);
  slotsRow.appendChild(slotsLeft);
  wrap.appendChild(slotsRow);

  if (result.calendar_slots.length > 0) {
    const slotButtons = el("div", { class: "today-list", style: "flex-direction: row; flex-wrap: wrap; gap: 8px;" });
    result.calendar_slots.forEach((slot) => {
      const slotBtn = el("button", { class: "button button-secondary", type: "button" }, [slot.label]);
      slotBtn.addEventListener("click", async () => {
        slotBtn.disabled = true;
        slotBtn.textContent = "Opening calendar...";
        try {
          const out = await invoke("confirm_discovery_slot", {
            args: {
              client_code: result.client_code,
              client_name: result.client_name,
              primary_contact_email: lead.primary_contact_email || result.primary_contact_email || null,
              start: slot.start,
              end: slot.end,
            },
          });
          openExternal(out.calendar_url);
          showToast("Google Calendar opened — click Save to send the invite", { ttl: 4500 });
          slotBtn.textContent = `${slot.label} (opened)`;
        } catch (e) {
          slotBtn.disabled = false;
          slotBtn.textContent = slot.label;
          showToast(`Couldn't open calendar: ${e}`, { ttl: 5000 });
        }
      });
      slotButtons.appendChild(slotBtn);
    });
    wrap.appendChild(slotButtons);
  }

  // Mark Lead as won — the final L5 action.
  const wonRow = el("div", { class: "today-row" });
  const wonBtn = el("button", { class: "button", type: "button" }, ["Mark Lead as won"]);
  wonBtn.addEventListener("click", async () => {
    wonBtn.disabled = true;
    wonBtn.textContent = "Updating...";
    try {
      await invoke("mark_lead_won", { args: { lead_record_id: lead.record_id } });
      wonBtn.textContent = "Lead marked won ✓";
      showToast(`${result.client_code} — lead marked won`, { ttl: 3500 });
    } catch (e) {
      wonBtn.disabled = false;
      wonBtn.textContent = "Mark Lead as won";
      showToast(`Couldn't mark lead won: ${e}`, { ttl: 5000 });
    }
  });
  wonRow.appendChild(el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, ["Lead status"]),
    el("div", { class: "today-row-meta" }, [
      "Once you're confident the cascade is good, flip the Lead to won. Removes it from the pipeline (L5).",
    ]),
  ]));
  wonRow.appendChild(el("div", { class: "today-row-side" }, [wonBtn]));
  wrap.appendChild(wonRow);

  return wrap;
}

function buildOpenLinkButton(url, label) {
  const btn = el("button", { class: "button button-secondary", type: "button" }, [label]);
  btn.addEventListener("click", () => openExternal(url));
  return btn;
}

function openExternal(url) {
  if (!url) return;
  if (window.__TAURI__?.opener?.openUrl) {
    window.__TAURI__.opener.openUrl(url).catch(() => window.open(url, "_blank"));
  } else {
    window.open(url, "_blank");
  }
}

async function loadClientView(clientCode) {
  const todayEl = document.getElementById("today-view");
  const clientEl = document.getElementById("client-view");
  if (!clientEl) return;

  // Stop the live-status + calendar intervals — they only matter on the Today view.
  clearLiveStatusTimer();
  clearCalendarTimer();
  // v0.40 — drop any project pollers from a previous project view.
  poller.unregisterPrefix("project.");

  // Switching clients drops any open chat or project surface so the new
  // client's standard view paints from a clean slot.
  _chatActive = false;
  _projectActive = false;
  convRender.exitChat();
  projectRender.exitProject();

  todayEl?.setAttribute("hidden", "");
  clientEl.removeAttribute("hidden");
  // Hide pipeline + skills-only + subcontractor views if any were
  // previously visible.
  document.getElementById("pipeline-view")?.setAttribute("hidden", "");
  document.getElementById("studio-cfo-view")?.setAttribute("hidden", "");
  document.getElementById("subcontractor-view")?.setAttribute("hidden", "");
  document
    .querySelectorAll('[data-view-kind="skills-only"]')
    .forEach((n) => n.setAttribute("hidden", ""));

  // Brand-new client view: blow away state and show a loading placeholder.
  _state.client = emptyClientState();
  _state.client.code = (clientCode || "").toUpperCase();
  clientRender.drawLoading(_state.client.code);

  const titleEl = document.querySelector(".main-title");
  if (titleEl) titleEl.textContent = _state.client.code;

  // Fetch + render. fetch.loadAll never throws — it logs and writes empty
  // arrays into the slots so render always has something to read.
  await clientFetch.loadAll(_state.client, _state.client.code);
  clientRender.draw(_state.client);
  // v0.40 — start live polling for this client. Poller pauses on blur,
  // backs off on errors, and unregisters when we switch view.
  registerClientPollers();

  // Update title to the resolved client name (loadHeader filled it in).
  if (titleEl && _state.client.header?.name) {
    titleEl.textContent = _state.client.header.name;
  }
}

// ─── Chat surface (v0.27 Block D) ────────────────────────────────────
//
// Replaces the per-client right pane with a workstream rail + chat pane.
// Caller is responsible for the client view being in the right state
// (workstreams loaded). The Back button on the rail returns to the
// standard per-client view by re-rendering it.

async function loadChatView(workstream) {
  if (!isTauri) {
    showToast("Open the Companion app to use the chat surface.");
    return;
  }
  if (!workstream?.code) return;

  // Make sure the per-client view container is visible. If we got here
  // from the Today dashboard via a client-bound workstream click, the
  // sidebar/loadClientView path already handled the switch. From the
  // per-client view it's a no-op.
  document.getElementById("today-view")?.setAttribute("hidden", "");
  document.getElementById("client-view")?.removeAttribute("hidden");

  _chatActive = true;
  _state.conversation = emptyConversationState();
  _state.conversation.workstream_code = workstream.code;
  _state.conversation.workstream_title = workstream.title || workstream.code;
  convRender.drawLoading();

  try {
    await convFetch.loadConversation(_state.conversation, workstream.code);
  } catch (e) {
    showToast(`Open conversation failed: ${e}`);
  }

  // Workstream rail draws from the per-client view's already-loaded list
  // when available. If it's empty (chat opened from Today before client
  // view loaded) we still render with the single workstream so the user
  // has something.
  const rail = (_state.client.workstreams && _state.client.workstreams.length > 0)
    ? _state.client.workstreams
    : [workstream];
  convRender.draw(_state.conversation, rail);
}

function exitChatToClientView() {
  _chatActive = false;
  convRender.exitChat();
  if (_state.client?.code) {
    clientRender.draw(_state.client);
  }
}

// ─── Per-project view (v0.28 Block E) ────────────────────────────────
//
// Replaces the per-client right pane with a project header, free-text
// composer, and an aggregated update feed (Notes + Receipts +
// Conversations + Calendar + Slack + Gmail + Drive). Same mount-on-
// top-of-#client-view pattern as the chat surface.

async function loadProjectView(projectCode, opts = {}) {
  if (!isTauri) {
    showToast("Open the Companion app to use the project view.");
    return;
  }
  const code = (projectCode || "").trim();
  if (!code) return;

  // Stop dashboard timers; this view is a per-client child.
  clearLiveStatusTimer();
  clearCalendarTimer();
  // v0.40 — pause client pollers while we're inside a project view.
  // Project pollers register below.
  poller.unregisterPrefix("client.");

  // Drop chat if it was open. Project view replaces #client-view.
  _chatActive = false;
  convRender.exitChat();

  document.getElementById("today-view")?.setAttribute("hidden", "");
  document.getElementById("client-view")?.removeAttribute("hidden");

  _projectActive = true;
  _state.project = emptyProjectState();
  _state.project.project_code = code;
  projectRender.drawLoading(code);

  await projectFetch.loadProject(_state.project, code);
  projectRender.draw(_state.project);
  // v0.40 — register the per-project Updates poller. The project view
  // is otherwise a single feed; the poller refresh keeps it lively.
  registerProjectPollers();

  const titleEl = document.querySelector(".main-title");
  if (titleEl) {
    titleEl.textContent =
      _state.project.header?.name || _state.project.header?.code || code;
  }

  // If we opened from outside a client view, optionally pre-load the
  // matching client view so a Back press lands on the right surface.
  if (opts.preloadClient && _state.project.header?.client_code) {
    // Best-effort, fire and forget — we don't await this so the
    // project view stays responsive.
    const clientCode = _state.project.header.client_code;
    if (_state.client?.code !== clientCode) {
      _state.client = emptyClientState();
      _state.client.code = clientCode;
      clientFetch.loadAll(_state.client, clientCode).catch((e) =>
        console.warn("background client preload failed:", e)
      );
    }
  }
}

function exitProjectToClientView() {
  _projectActive = false;
  projectRender.exitProject();
  // v0.40 — drop the per-project poller and re-arm client pollers if
  // we land on a client view.
  poller.unregisterPrefix("project.");
  // Fall back to Today if we have no client context (e.g. opened from
  // a deep link, then back-pressed).
  if (_state.client?.code) {
    clientRender.draw(_state.client);
    registerClientPollers();
    const titleEl = document.querySelector(".main-title");
    if (titleEl && _state.client.header?.name) {
      titleEl.textContent = _state.client.header.name;
    }
  } else {
    showTodayView();
  }
}

async function loadFeed() {
  const feed = document.getElementById("feed");
  if (!feed) return;
  feed.innerHTML = "";

  let persisted = [];
  if (isTauri) {
    try {
      const rows = await invoke("list_receipts", { limit: 50 });
      persisted = rows.map((j) => JSON.parse(j));
    } catch (e) {
      console.warn("list_receipts failed:", e);
    }
  }

  if (persisted.length === 0) {
    // Empty state: render the seed Strategic Thinking receipt so the feed
    // never looks blank on first launch.
    feed.appendChild(renderReceipt(STRATEGIC_THINKING));
  } else {
    persisted.forEach((r) => feed.appendChild(renderReceipt(r)));
  }
}

document.addEventListener("DOMContentLoaded", () => {
  const feed = document.getElementById("feed");
  loadFeed();

  // ── Today dashboard: initial mount + handlers ──────────────────────
  // Render an empty skeleton immediately so the user sees structure, then
  // populate as fetchers resolve. Each fetcher updates _state.today and
  // we redraw once everything settles.
  todayRender.draw(_state.today);
  todayFetch.loadAll(_state.today).then(() => {
    todayRender.draw(_state.today);
    registerTodayPollers();
  });

  document.getElementById("today-refresh-btn")?.addEventListener("click", async () => {
    const btn = document.getElementById("today-refresh-btn");
    btn.disabled = true;
    const prev = btn.innerHTML;
    btn.innerHTML = "<span aria-hidden=\"true\">…</span>";
    try {
      await poller.refreshAll();
      showToast("Refreshed");
    } catch (e) {
      showToast(`Refresh failed: ${e}`);
    } finally {
      btn.disabled = false;
      btn.innerHTML = prev;
    }
  });

  // v0.40 — Cmd-R: window-level keydown shortcut for manual refresh of
  // every registered section. Window-level (not OS-global) so it only
  // fires when Companion is the focused window. Fires only when the
  // user isn't typing into a field — letting Cmd-R reload an Airtable
  // textarea would be hostile.
  window.addEventListener("keydown", (e) => {
    const meta = e.metaKey || e.ctrlKey;
    if (!meta) return;
    if ((e.key || "").toLowerCase() !== "r") return;
    const t = e.target;
    const tag = t?.tagName;
    const editable = tag === "INPUT" || tag === "TEXTAREA" || t?.isContentEditable;
    if (editable) return;
    e.preventDefault();
    poller.refreshAll().then(() => showToast("Refreshed"));
  });

  // Click handlers dispatched by the Today render layer.
  // v0.37 Block F — clicking the margin row in Today's live-status grid
  // jumps straight into the Studio CFO view.
  document.addEventListener("today:open-cfo", () => {
    const item = document.querySelector('.sidebar-item[data-view="studio-cfo"]');
    if (item) {
      document.querySelectorAll(".sidebar-item").forEach((i) => {
        i.classList.remove("active");
        i.removeAttribute("aria-current");
      });
      item.classList.add("active");
      item.setAttribute("aria-current", "page");
    }
    showStudioCfoView();
  });

  document.addEventListener("today:open-url", (e) => {
    const url = e.detail?.url;
    if (!url) return;
    if (window.__TAURI__?.opener?.openUrl) {
      window.__TAURI__.opener.openUrl(url).catch(() => window.open(url, "_blank"));
    } else {
      window.open(url, "_blank");
    }
  });

  document.addEventListener("today:commitment-click", (e) => {
    showCommitmentModal(e.detail?.commitment);
  });

  // v0.33 Block F — Pipeline section's "Promote Lead" button.
  document.addEventListener("today:promote-lead", (e) => {
    const lead = e.detail?.lead;
    if (lead) showPromoteLeadModal(lead);
  });

  document.addEventListener("today:decision-click", (e) => {
    showDecisionCaptureModal(e.detail?.decision);
  });

  document.addEventListener("today:workstream-click", async (e) => {
    const w = e.detail?.workstream;
    if (w?._client_codes?.length) {
      // Tagged to a client — load that client view, then drop the user
      // straight into the chat surface for the workstream they clicked.
      const code = w._client_codes[0];
      activateClientSidebar(code);
      await loadClientView(code);
      loadChatView(w);
      return;
    }
    // Untagged workstream — open the detail modal in place.
    showWorkstreamDetailModal(w);
  });

  // ─── Per-client view event handlers ─────────────────────────────────
  document.addEventListener("client:workstream-click", (e) => {
    const w = e.detail?.workstream;
    if (!w) return;
    // v0.27: workstream click from per-client view opens the chat surface
    // directly. The standalone metadata modal is still available via the
    // workstream-info button (today/decision flows still use it).
    loadChatView(w);
  });

  document.addEventListener("client:decision-click", (e) => {
    showDecisionCaptureModal(e.detail?.decision);
  });

  document.addEventListener("client:commitment-click", (e) => {
    showCommitmentModal(e.detail?.commitment);
  });

  document.addEventListener("client:project-click", (e) => {
    const p = e.detail?.project;
    if (!p?.code) return;
    // v0.28 Block E: clicking a project from the per-client view opens
    // the per-project Updates feed. The detail modal is still reachable
    // from the project header for fields-only edits.
    loadProjectView(p.code);
  });

  // ─── Per-project view event handlers (v0.28 Block E) ─────────────────
  document.addEventListener("project:back-click", () => {
    exitProjectToClientView();
  });

  document.addEventListener("project:client-link-click", async (e) => {
    const code = e.detail?.client_code;
    if (!code) return;
    _projectActive = false;
    projectRender.exitProject();
    activateClientSidebar(code);
    await loadClientView(code);
  });

  document.addEventListener("project:open-url", (e) => {
    const url = e.detail?.url;
    if (!url) return;
    if (window.__TAURI__?.opener?.openUrl) {
      window.__TAURI__.opener.openUrl(url).catch(() => window.open(url, "_blank"));
    } else {
      window.open(url, "_blank");
    }
  });

  // v0.30 Forms layer: per-project "Send Form" buttons. Discovery and
  // Post-Campaign Feedback prefill with the current project's record
  // id. Content Approval prompts for a SocialPost record id (the
  // proper SocialPost picker lands in v0.32 alongside the email-draft
  // polish).
  // v0.31 Block F: Draft Wrap Report button on a wrapped project view.
  // Centralised here so the modal catalogue stays in main.js.
  document.addEventListener("project:wrap-report-click", async (e) => {
    if (!isTauri) {
      showToast("Open the Companion app to run live workflows. Preview is read-only.");
      return;
    }
    const ready = await invoke("get_api_key_status");
    if (!ready) {
      showApiKeyBanner();
      showToast("Set your Anthropic API key first");
      return;
    }
    const projectCode = e.detail?.project_code || _state.project?.project_code;
    showWrapReportModal(projectCode);
  });

  document.addEventListener("project:form-send", async (e) => {
    const formKey = e.detail?.form_key;
    if (!formKey) return;
    if (!isTauri) return showToast("Open the Companion app to send forms.");
    const header = _state.project?.header;
    if (!header) return showToast("Project not loaded yet.");

    const meta = forms.FORM_META[formKey];
    if (!meta) return showToast(`Unknown form: ${formKey}`);

    let url;
    try {
      url = await forms.getFormUrl(formKey);
    } catch (err) {
      return showToast(`Couldn't read form URL: ${err}`, { ttl: 5000 });
    }
    if (!url) {
      return showToast(
        `${meta.label} URL isn't set yet. Add it in Settings → Forms.`,
        { ttl: 5000 }
      );
    }

    let recordId = header.project_record_id || null;
    if (formKey === "form_content_approval") {
      const promptVal = window.prompt(
        "Paste the SocialPost record id (rec...) to bind this approval to a draft post:",
        ""
      );
      const trimmed = (promptVal || "").trim();
      if (!trimmed) return; // cancelled
      if (!/^rec[A-Za-z0-9]{14}$/.test(trimmed)) {
        return showToast("That doesn't look like a record id (rec...)");
      }
      recordId = trimmed;
    }

    if (!recordId) return showToast("Missing record id for prefill.");

    const prefilled = forms.buildPrefillUrl(url, meta.prefill, recordId);
    const projectLabel = header.name || header.code || "this project";
    const subject =
      formKey === "form_post_campaign_feedback"
        ? `${header.client_name || header.client_code || "Hi"}: Post-campaign feedback (${projectLabel})`
        : formKey === "form_content_approval"
        ? `Quick approval needed: ${projectLabel}`
        : `${header.client_name || header.client_code || "Hi"}: A few quick questions before our discovery call`;
    const body = buildFormEmailBody(formKey, {
      project: { name: header.name, code: header.code },
      url: prefilled,
    });
    // We don't always have an email address on the project header. The
    // user fills it in their mail client.
    const mailto = forms.buildMailto({ to: "", subject, body });
    forms.openUrl(mailto);
    showToast(`Email draft opened for ${meta.label}`);
  });

  document.addEventListener("project:note-save", async (e) => {
    const body = (e.detail?.body || "").trim();
    const tags = Array.isArray(e.detail?.tags) ? e.detail.tags : [];
    if (!body) return;
    if (!_state.project?.project_code) return;
    if (_state.project.composer.saving) return;

    // v0.40 — optimistic: drop a fake "note" row at the top of the feed
    // immediately, clear the composer, fire the backend write. On
    // failure, drop the optimistic row and surface a toast.
    const optimisticId = `optimistic-${Date.now()}`;
    const optimisticRow = {
      kind: "note",
      id: optimisticId,
      body,
      tags,
      created_at: new Date().toISOString(),
      _optimistic: true,
    };
    const updatesBefore = Array.isArray(_state.project.updates)
      ? [..._state.project.updates]
      : [];
    _state.project.updates = [optimisticRow, ...updatesBefore];
    _state.project.composer = { body: "", tags: [], saving: false, error: null };
    projectRender.draw(_state.project);

    try {
      await projectFetch.createNote(_state.project.project_code, body, tags);
      // Refresh the feed so the optimistic row gets replaced by the
      // server-canonical record (with proper id and timestamps).
      await projectFetch.refreshUpdates(_state.project);
      projectRender.drawUpdatesSection(_state.project);
      showToast("Note saved");
    } catch (err) {
      // Rewind the optimistic insert and re-populate the composer.
      _state.project.updates = updatesBefore;
      _state.project.composer = {
        body,
        tags,
        saving: false,
        error: String(err),
      };
      projectRender.draw(_state.project);
      showToast(`Failed to save. Try again. (${err})`, { ttl: 5000 });
    }
  });

  document.addEventListener("client:receipt-click", (e) => {
    showReceiptJsonModal(e.detail?.receipt);
  });

  // v0.37 Block F — Studio CFO month picker. Shift the visible month and
  // re-fetch all four CFO slices.
  document.addEventListener("cfo:shift-month", async (e) => {
    const delta = Number(e.detail?.delta) || 0;
    if (delta === 0) return;
    cfoShiftMonth(_state.cfo, delta);
    const view = document.getElementById("studio-cfo-view");
    if (!view) return;
    _state.cfo.totals = null;
    _state.cfo.per_client = null;
    _state.cfo.alerts = null;
    _state.cfo.outlook = null;
    _state.cfo.loading = true;
    cfoRender.draw(view, _state.cfo);
    if (isTauri) {
      await cfoFetch.loadAll(_state.cfo);
    } else {
      _state.cfo.loading = false;
      _state.cfo.error =
        "Open the Companion app to load CFO data. Preview is read-only.";
    }
    cfoRender.draw(view, _state.cfo);
  });

  document.addEventListener("client:refresh-click", async () => {
    if (!_state.client?.code) return;
    await clientFetch.loadAll(_state.client, _state.client.code);
    clientRender.draw(_state.client);
  });

  // v0.30 Forms layer: per-client "Send Form" buttons. Both Discovery
  // Pre-Brief and Post-Campaign Feedback prefill the Project record id,
  // so we ask the user to pick a project from the active list when
  // there's more than one.
  document.addEventListener("client:form-send", async (e) => {
    const formKey = e.detail?.form_key;
    if (!formKey) return;
    if (!isTauri) return showToast("Open the Companion app to send forms.");
    if (!_state.client?.recordId) return showToast("Load a client first.");

    const meta = forms.FORM_META[formKey];
    if (!meta) return showToast(`Unknown form: ${formKey}`);

    let url;
    try {
      url = await forms.getFormUrl(formKey);
    } catch (err) {
      return showToast(`Couldn't read form URL: ${err}`, { ttl: 5000 });
    }
    if (!url) {
      return showToast(
        `${meta.label} URL isn't set yet. Add it in Settings → Forms.`,
        { ttl: 5000 }
      );
    }

    // Pick a project. For Post-Campaign Feedback we filter to wrapped/done.
    const allProjects = _state.client.projects || [];
    const wantWrapped = formKey === "form_post_campaign_feedback";
    const candidates = wantWrapped
      ? allProjects.filter((p) =>
          ["wrap", "done", "wrapped", "complete", "completed"].includes(
            String(p.status || "").toLowerCase()
          )
        )
      : allProjects;

    if (candidates.length === 0) {
      return showToast("No projects to attach the form to.");
    }

    const project = await pickProject(candidates, meta.label);
    if (!project) return; // user cancelled

    const prefilled = forms.buildPrefillUrl(url, meta.prefill, project.id);
    const clientName = _state.client.header?.name || _state.client.code;
    const contactEmail = _state.client.header?.primary_contact_email || "";
    const subject =
      formKey === "form_post_campaign_feedback"
        ? `${clientName}: Post-campaign feedback (${project.code || project.name || "project"})`
        : `${clientName}: A few quick questions before our discovery call`;
    const body = buildFormEmailBody(formKey, {
      clientName,
      project,
      url: prefilled,
    });
    const mailto = forms.buildMailto({ to: contactEmail, subject, body });
    forms.openUrl(mailto);
    showToast(`Email draft opened for ${meta.label}`);
  });

  // ─── Chat surface event handlers (v0.27 Block D) ─────────────────────
  document.addEventListener("conversation:back-click", () => {
    exitChatToClientView();
  });

  document.addEventListener("conversation:info-click", (e) => {
    const w = e.detail?.workstream;
    if (w) showWorkstreamDetailModal(w);
  });

  document.addEventListener("conversation:rail-click", (e) => {
    const w = e.detail?.workstream;
    if (!w) return;
    if (_state.conversation.sending) {
      showToast("Hang on, still sending the previous message.");
      return;
    }
    loadChatView(w);
  });

  document.addEventListener("conversation:send", async (e) => {
    const text = (e.detail?.text || "").trim();
    if (!text) return;
    if (!_state.conversation.workstream_code) return;
    if (_state.conversation.sending) return;
    if (_state.conversation.status === "archived") {
      showToast("This conversation is archived.");
      return;
    }

    // Optimistic append: show the user's message immediately, mark
    // sending=true so the composer disables and the "Thinking…" bubble
    // appears. The Rust call returns the full updated transcript, which
    // overwrites our optimistic state.
    const nowIso = new Date().toISOString();
    _state.conversation.messages.push({ role: "user", content: text, ts: nowIso });
    _state.conversation.sending = true;
    convRender.draw(_state.conversation, _state.client.workstreams || []);

    try {
      await convFetch.sendMessage(_state.conversation, text);
    } catch (err) {
      _state.conversation.sending = false;
      // Pop the optimistic user message so we don't double up if she
      // retries — the next successful send will append it cleanly.
      _state.conversation.messages.pop();
      showToast(`Send failed: ${err}`);
    }

    convRender.draw(_state.conversation, _state.client.workstreams || []);
  });

  // v0.34 Skills restructure: single dispatch entry point. Skill cards
  // from any view (Today quick strip, per-client, per-project, Team,
  // Social launch, the More skills overflow modal) emit "skill:dispatch"
  // with { skill_id, client_code?, project_code? }. The dispatcher in
  // src/skills/dispatch.js routes to the right modal opener.
  const skillDispatchCtx = {
    modals: {
      showStrategicThinkingModal,
      showMonthlyCheckinModal,
      showQuarterlyReviewModal,
      showNewClientOnboardingModal,
      showSubcontractorOnboardingModal,
      showScheduleSocialPostModal,
      showLogTimeModal,
      showEditProjectModal,
      showNctCaptionModal,
      showBuildScopeModal,
      showWrapReportModal,
      showInCahootsSocialModal,
      showPressReleaseModal,
      showEdmWriterModal,
      showReelsScriptingModal,
      showHookGeneratorModal,
      showClientEmailModal,
      showHumaniserModal,
      showCopyEditorModal,
      showCampaignLaunchChecklistModal,
      // v0.39 Block F — Draft dossier (multi-source).
      showDraftDossierModal,
    },
    toast: showToast,
    requireApiKey: async () => {
      if (!isTauri) {
        showToast("Open the Companion app to run live workflows. Preview is read-only.");
        return false;
      }
      const ready = await invoke("get_api_key_status");
      if (!ready) {
        showApiKeyBanner();
        showToast("Set your Anthropic API key first");
        return false;
      }
      return true;
    },
    requireTauri: async () => {
      if (!isTauri) {
        showToast("Open the Companion app to file workflows. Preview is read-only.");
        return false;
      }
      return true;
    },
  };

  document.addEventListener("skill:dispatch", (e) => {
    const { skill_id, client_code, project_code } = e.detail || {};
    if (!skill_id) return;
    dispatchSkill(skill_id, { client_code, project_code }, skillDispatchCtx);
  });

  document.addEventListener("client:workflow-click", (e) => {
    const { key, client_code } = e.detail || {};
    clientWorkflows.launch(key, client_code, {
      modals: {
        showStrategicThinkingModal,
        showMonthlyCheckinModal,
        showQuarterlyReviewModal,
        showNewCampaignScopeModal,
        showScheduleSocialPostModal,
        showLogTimeModal,
        showEditProjectModal,
        // v0.31 Block F — Skills batch 1 modals.
        showNctCaptionModal,
        showBuildScopeModal,
        // v0.32 Block F — Skills batch 2 modals.
        showPressReleaseModal,
        showEdmWriterModal,
        showReelsScriptingModal,
        showHookGeneratorModal,
        showClientEmailModal,
        showHumaniserModal,
        showCopyEditorModal,
      },
      toast: showToast,
      requireApiKey: async () => {
        if (!isTauri) {
          showToast("Open the Companion app to run live workflows. Preview is read-only.");
          return false;
        }
        const ready = await invoke("get_api_key_status");
        if (!ready) {
          showApiKeyBanner();
          showToast("Set your Anthropic API key first");
          return false;
        }
        return true;
      },
    });
  });

  // Refocus refresh on the client view too.
  window.addEventListener("focus", async () => {
    if (document.getElementById("client-view")?.hasAttribute("hidden")) return;
    if (!_state.client?.code) return;
    await clientFetch.refreshStale(_state.client);
    clientRender.draw(_state.client);
  });

  // Receipt action buttons (currently: reprint).
  feed?.addEventListener("click", async (e) => {
    const btn = e.target.closest('.receipt-action-btn[data-action="reprint"]');
    if (!btn) return;
    if (!isTauri) {
      showToast("Reprint runs in the Companion app, not the browser preview.");
      return;
    }
    const receiptEl = btn.closest(".receipt");
    const receiptId = receiptEl?.dataset.receiptId;
    if (!receiptId) return showToast("No receipt id");

    btn.disabled = true;
    const original = btn.textContent;
    btn.textContent = "Printing...";
    try {
      await invoke("reprint_receipt", { receiptId });
      showToast("Sent to Munbyn");
    } catch (err) {
      showToast(`Reprint failed: ${err}`, { ttl: 6000 });
    } finally {
      btn.disabled = false;
      btn.textContent = original;
    }
  });

  // Tick handler: update UI optimistically, fire backend tick_item to
  // persist + run on_done hook (Slack post, etc.).
  feed?.addEventListener("change", async (e) => {
    if (!e.target.matches('input[type="checkbox"]')) return;
    const row = e.target.closest(".receipt-item-task");
    if (!row) return;

    const ticking = e.target.checked;
    row.classList.toggle("done", ticking);
    if (!ticking || !isTauri) return;

    const receiptEl = row.closest(".receipt");
    const receiptId = receiptEl?.dataset.receiptId;
    const itemIndex = parseInt(row.dataset.itemIndex || "-1", 10);
    if (!receiptId || itemIndex < 0) return;

    e.target.disabled = true;
    try {
      await invoke("tick_item", { receiptId, itemIndex });
      const onDone = row.dataset.onDone;

      // airtable:create-client → prompt for canonical code, then create the row.
      // airtable:create-project → confirm the project code, then file to Projects.
      // airtable:create-subcontractor → confirm code, file to Subcontractors.
      if (onDone === "airtable:create-client") {
        await handleCreateClientFromReceipt(receiptEl);
      } else if (onDone === "airtable:create-project") {
        await handleCreateProjectFromReceipt(receiptEl);
      } else if (onDone === "airtable:create-subcontractor") {
        await handleCreateSubcontractorFromReceipt(receiptEl);
      } else {
        showToast(onDone ? `Ticked, hook fired (${onDone})` : "Ticked");
      }
    } catch (err) {
      e.target.disabled = false;
      e.target.checked = false;
      row.classList.remove("done");
      showToast(`Tick failed: ${err}`, { ttl: 5000 });
    }
  });

  // Sidebar routing.
  const titleEl = document.querySelector(".main-title");

  function activateSidebar(item) {
    document.querySelectorAll(".sidebar-item").forEach((i) => {
      i.classList.remove("active");
      i.removeAttribute("aria-current");
    });
    item.classList.add("active");
    item.setAttribute("aria-current", "page");
  }

  document.body.addEventListener("click", (e) => {
    // Project sub-items inside the sidebar take priority — they sit
    // inside an expanded client item but render with their own class.
    const projectItem = e.target.closest(".sidebar-subitem[data-project-code]");
    if (projectItem) {
      e.preventDefault();
      e.stopPropagation();
      const projectCode = projectItem.dataset.projectCode;
      if (!projectCode) return;
      // Activate the parent client item visually so the user knows
      // which client they're inside.
      const parentClient = projectItem.closest("[data-client-code]");
      if (parentClient) activateSidebar(parentClient);
      loadProjectView(projectCode, { preloadClient: true });
      return;
    }

    // v0.38: Subcontractor sub-items under the Team entry. Clicking opens
    // the per-Subcontractor view scoped to that person.
    const subcontractorItem = e.target.closest(
      ".sidebar-subitem[data-subcontractor-code]"
    );
    if (subcontractorItem) {
      e.preventDefault();
      e.stopPropagation();
      const subCode = subcontractorItem.dataset.subcontractorCode;
      if (!subCode) return;
      // Activate the parent Team entry visually.
      const teamItem = document.querySelector(
        '.sidebar-item[data-view="team"]'
      );
      if (teamItem) activateSidebar(teamItem);
      showSubcontractorView(subCode);
      return;
    }

    const item = e.target.closest(".sidebar-item");
    if (!item) return;
    const view = item.dataset.view || "today";

    if (view === "settings") {
      showSettingsModal();
      return;
    }

    activateSidebar(item);

    if (view === "today") {
      showTodayView();
      return;
    }

    // Per-client routes have data-client-code on the sidebar item, OR they
    // start with "client-" (added dynamically by loadStudioSidebar).
    const clientCode =
      item.dataset.clientCode ||
      (view.startsWith("client-") ? view.slice("client-".length).toUpperCase() : null);

    if (clientCode) {
      loadClientView(clientCode);
      return;
    }

    // v0.30: Pipeline gets a tiny dedicated view with the Lead Intake
    // form button at the top. Everything else (Products, Personal) is
    // still a future surface.
    if (view === "pipeline") {
      showPipelineView();
      if (titleEl) titleEl.textContent = "Pipeline";
      return;
    }

    // v0.34: Team view holds Subcontractor Onboarding (and future team
    // skills). Social launch under Personal holds the In Cahoots social
    // post skill. Both are minimal Skills-only surfaces for now.
    if (view === "team") {
      showSkillsOnlyView({
        id: "team",
        title: "Team",
        emoji: "👥",
        intro: "Team operations. Run subcontractor onboarding from here.",
        context: "team",
      });
      if (titleEl) titleEl.textContent = "Team";
      return;
    }
    if (view === "social-launch") {
      showSkillsOnlyView({
        id: "social-launch",
        title: "Social launch",
        emoji: "📣",
        intro: "Founder-led launch for @incahoots.marketing.",
        context: "social_launch",
      });
      if (titleEl) titleEl.textContent = "Social launch";
      return;
    }

    // v0.37 Block F — Studio CFO under Personal.
    if (view === "studio-cfo") {
      showStudioCfoView();
      return;
    }

    // Anything else (Products, Personal items): keep on Today and toast.
    showTodayView();
    if (titleEl) titleEl.textContent = item.textContent.trim();
    showToast(`${item.textContent.trim()} isn't wired yet. Today is the live view this build.`);
  });

  // First-launch API key check (Tauri only).
  ensureApiKey();

  // Airtable: hydrate sidebar with active clients if connected.
  loadStudioSidebar();

  // Background: check for updates on launch.
  checkForUpdates();

  // v0.36 Block F — global search bar in the header. Cmd-K focuses.
  globalSearch.mount({
    invoke,
    isTauri,
    showToast,
    onJump: handleSearchJump,
  });

  async function openStrategicThinking() {
    if (!isTauri) {
      showToast("Open the Companion app (DMG) to run live workflows. Preview is read-only.");
      return;
    }
    const ready = await invoke("get_api_key_status");
    if (!ready) {
      showApiKeyBanner();
      showToast("Set your Anthropic API key first");
      return;
    }
    showStrategicThinkingModal();
  }

  // Workflow cards: route by title to the right modal.
  async function openNewClientOnboarding() {
    if (!isTauri) {
      showToast("Open the Companion app to run live workflows. Preview is read-only.");
      return;
    }
    const ready = await invoke("get_api_key_status");
    if (!ready) {
      showApiKeyBanner();
      showToast("Set your Anthropic API key first");
      return;
    }
    showNewClientOnboardingModal();
  }

  async function openNewCampaignScope() {
    if (!isTauri) {
      showToast("Open the Companion app to run live workflows. Preview is read-only.");
      return;
    }
    const ready = await invoke("get_api_key_status");
    if (!ready) {
      showApiKeyBanner();
      showToast("Set your Anthropic API key first");
      return;
    }
    showNewCampaignScopeModal();
  }

  async function openMonthlyCheckin() {
    if (!isTauri) {
      showToast("Open the Companion app to run live workflows. Preview is read-only.");
      return;
    }
    const ready = await invoke("get_api_key_status");
    if (!ready) {
      showApiKeyBanner();
      showToast("Set your Anthropic API key first");
      return;
    }
    showMonthlyCheckinModal();
  }

  async function openSubcontractorOnboarding() {
    if (!isTauri) {
      showToast("Open the Companion app to run live workflows. Preview is read-only.");
      return;
    }
    const ready = await invoke("get_api_key_status");
    if (!ready) {
      showApiKeyBanner();
      showToast("Set your Anthropic API key first");
      return;
    }
    showSubcontractorOnboardingModal();
  }

  async function openQuarterlyReview() {
    if (!isTauri) {
      showToast("Open the Companion app to run live workflows. Preview is read-only.");
      return;
    }
    const ready = await invoke("get_api_key_status");
    if (!ready) {
      showApiKeyBanner();
      showToast("Set your Anthropic API key first");
      return;
    }
    showQuarterlyReviewModal();
  }

  // v0.31 Block F: shared opener for the new skill-backed workflows.
  // Same gate as the older Anthropic-backed workflows above — Tauri
  // only, API key set, then call the supplied modal.
  async function openSkillWorkflow(modalFn, ...args) {
    if (!isTauri) {
      showToast("Open the Companion app to run live workflows. Preview is read-only.");
      return;
    }
    const ready = await invoke("get_api_key_status");
    if (!ready) {
      showApiKeyBanner();
      showToast("Set your Anthropic API key first");
      return;
    }
    modalFn(...args);
  }

  document.querySelectorAll(".workflow-card").forEach((card) => {
    const name = card.querySelector(".workflow-card-title")?.textContent || "Workflow";
    card.addEventListener("click", () => {
      if (name === "Strategic Thinking") {
        openStrategicThinking();
      } else if (name === "New Client Onboarding") {
        openNewClientOnboarding();
      } else if (name === "Monthly Check-in") {
        openMonthlyCheckin();
      } else if (name === "New Campaign Scope") {
        openNewCampaignScope();
      } else if (name === "Quarterly Review") {
        openQuarterlyReview();
      } else if (name === "Subcontractor Onboarding") {
        openSubcontractorOnboarding();
      } else if (name === "Schedule social post") {
        // Pure-Airtable workflows — no Anthropic key needed.
        if (!isTauri) {
          showToast("Open the Companion app to file workflows. Preview is read-only.");
          return;
        }
        showScheduleSocialPostModal();
      } else if (name === "Log time") {
        if (!isTauri) {
          showToast("Open the Companion app to file workflows. Preview is read-only.");
          return;
        }
        showLogTimeModal();
      } else if (name === "Edit project") {
        if (!isTauri) {
          showToast("Open the Companion app to file workflows. Preview is read-only.");
          return;
        }
        showEditProjectModal();
      } else if (name === "Draft NCT social caption") {
        openSkillWorkflow(showNctCaptionModal);
      } else if (name === "In Cahoots social post") {
        openSkillWorkflow(showInCahootsSocialModal);
      } else if (name === "Draft Wrap Report") {
        openSkillWorkflow(showWrapReportModal);
      } else if (name === "Build Scope") {
        openSkillWorkflow(showBuildScopeModal);
      } else if (name === "Draft Press Release") {
        openSkillWorkflow(showPressReleaseModal);
      } else if (name === "Draft EDM") {
        openSkillWorkflow(showEdmWriterModal);
      } else if (name === "Script a Reel") {
        openSkillWorkflow(showReelsScriptingModal);
      } else if (name === "Generate hooks") {
        openSkillWorkflow(showHookGeneratorModal);
      } else if (name === "Draft email to client") {
        openSkillWorkflow(showClientEmailModal);
      } else if (name === "Edit pass: humanise") {
        openSkillWorkflow(showHumaniserModal);
      } else if (name === "Edit pass: copy editor") {
        openSkillWorkflow(showCopyEditorModal);
      } else {
        showToast(`${name}: not wired yet.`);
      }
    });
  });

  document.getElementById("strategic-thinking-btn")?.addEventListener("click", openStrategicThinking);

  document.getElementById("workflows-btn")?.addEventListener("click", () => {
    // v0.34: header "Skills" button opens the overflow modal directly.
    showSkillsOverflow((skillId) =>
      dispatchSkill(skillId, {}, skillDispatchCtx)
    );
  });

  // v0.34: "More skills..." button under the Today quick-skills strip.
  document.getElementById("more-skills-btn")?.addEventListener("click", () => {
    showSkillsOverflow((skillId) =>
      dispatchSkill(skillId, {}, skillDispatchCtx)
    );
  });
});
