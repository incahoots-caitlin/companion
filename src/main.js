// In Cahoots Studio — v0 spike
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

// Global state container. Today dashboard owns _state.today; per-client
// view owns _state.client. Switching clients overwrites _state.client.
const _state = {
  today: emptyTodayState(),
  client: emptyClientState(),
};

// 60s timer for live-status row, reset on each Today mount.
let _liveStatusTimer = null;

function clearLiveStatusTimer() {
  if (_liveStatusTimer) {
    clearInterval(_liveStatusTimer);
    _liveStatusTimer = null;
  }
}

function startLiveStatusTimer() {
  clearLiveStatusTimer();
  _liveStatusTimer = setInterval(async () => {
    if (document.hidden) return; // pause while tab not visible
    if (document.getElementById("today-view")?.hasAttribute("hidden")) return;
    await todayFetch.loadLiveStatus(_state.today);
    todayRender.drawLiveStatusOnly(_state.today);
  }, 60_000);
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
    throw new Error("Studio backend not available, running in browser preview");
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
      `Studio ${update.version} is ready. ${update.body || "Click install to update and restart."}`,
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
  if (isTauri) {
    try { apiSet = await invoke("get_api_key_status"); } catch {}
    try { slackSet = await invoke("get_slack_status"); } catch {}
    try { airtableSet = await invoke("get_airtable_status"); } catch {}
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
        : "Connect to your 'In Cahoots Ops' base. Personal access token + base ID. Studio's sidebar will pull active clients on launch.",
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
function showStrategicThinkingModal(_prefillClientCode) {
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
  const textarea = el("textarea", {
    class: "modal-textarea",
    placeholder: "I'm thinking about whether to launch COOL before the wedding or after, given the post-wedding sequencing decision yesterday...",
    rows: 10,
  });
  modal.appendChild(textarea);
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
    const input = textarea.value.trim();
    if (!input) return showToast("Type something first");
    const runBtn = document.getElementById("st-run");
    runBtn.disabled = true;
    runBtn.textContent = "Thinking...";
    try {
      const json = await invoke("run_strategic_thinking", { input });
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
  const notes = `Onboarded via Studio receipt ${receiptId}`;

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
        notes: `Filed via Studio receipt ${receiptId}`,
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
        notes: `Filed via Studio receipt ${receiptId}`,
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

  const cancelBtn = el("button", { class: "button button-secondary" }, ["Cancel"]);
  const runBtn = el("button", { class: "button" }, ["Run"]);
  modal.appendChild(el("div", { class: "modal-actions" }, [cancelBtn, runBtn]));

  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  clientPicker.focus();

  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  modal.querySelector(".modal-close").addEventListener("click", close);
  cancelBtn.addEventListener("click", close);

  runBtn.addEventListener("click", async () => {
    const input = {
      client_code: clientPicker.value,
      extra_notes: flags.value.trim() || null,
    };
    runBtn.disabled = true;
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
    meta: "Pick a client. Studio pulls their last 30 days of receipts as context, Claude returns a check-in receipt with what's done, what's open, what's next, and action items.",
    command: "run_monthly_checkin",
    runningLabel: "Reading the month...",
    successToast: "Check-in receipt ready",
    flagsPlaceholder: "e.g. payment overdue, scope creeping, key contact has changed, big upcoming on-sale. Skip if there's nothing.",
    prefillClientCode,
  });
}

async function showQuarterlyReviewModal(prefillClientCode) {
  return showClientPickerReviewModal({
    modalId: "qr-modal",
    title: "Quarterly Review",
    meta: "Pick a client. Studio pulls their last 90 days of receipts, Claude returns a QBR receipt with what worked, what didn't, and proposed next-quarter shape. Use this to drive the QBR call.",
    command: "run_quarterly_review",
    runningLabel: "Reading the quarter...",
    successToast: "QBR receipt ready",
    flagsPlaceholder: "e.g. renewal coming up, scope feels off, want to propose a shape change, considering wind-down. Skip if there's nothing pressing.",
    prefillClientCode,
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

  tickBtn.addEventListener("click", async () => {
    tickBtn.disabled = true;
    tickBtn.textContent = "Saving...";
    try {
      await invoke("update_airtable_record", {
        args: {
          table: "Commitments",
          record_id: commitment.id, // commitment.id is the Airtable record id
          fields: { status: "done" },
        },
      });
      showToast("Commitment ticked");
      close();
      // Refresh commitments slot.
      await todayFetch.loadCommitments(_state.today, await rebuildClientLookup());
      todayRender.draw(_state.today);
    } catch (e) {
      tickBtn.disabled = false;
      tickBtn.textContent = "Mark done";
      showToast(`Update failed: ${e}`);
    }
  });

  pushBtn.addEventListener("click", async () => {
    pushBtn.disabled = true;
    pushBtn.textContent = "Saving...";
    try {
      const next = new Date(Date.now() + 4 * 60 * 60 * 1000).toISOString();
      await invoke("update_airtable_record", {
        args: {
          table: "Commitments",
          record_id: commitment.id,
          fields: { next_check_at: next },
        },
      });
      showToast("Pushed 4 hours");
      close();
    } catch (e) {
      pushBtn.disabled = false;
      pushBtn.textContent = "Push 4 hours";
      showToast(`Update failed: ${e}`);
    }
  });
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

  saveBtn.addEventListener("click", async () => {
    const fields = {
      status: "made",
      decided_at: new Date().toISOString(),
      decision: decisionInput.value.trim(),
      reasoning: reasoningInput.value.trim(),
    };
    saveBtn.disabled = true;
    saveBtn.textContent = "Saving...";
    try {
      await invoke("update_airtable_record", {
        args: {
          table: "Decisions",
          record_id: decision.id,
          fields,
        },
      });
      showToast("Decision filed");
      close();
      await todayFetch.loadDecisions(_state.today, await rebuildClientLookup());
      todayRender.draw(_state.today);
    } catch (e) {
      saveBtn.disabled = false;
      saveBtn.textContent = "Mark made";
      showToast(`Update failed: ${e}`);
    }
  });

  deferBtn.addEventListener("click", async () => {
    deferBtn.disabled = true;
    deferBtn.textContent = "Saving...";
    try {
      await invoke("update_airtable_record", {
        args: {
          table: "Decisions",
          record_id: decision.id,
          fields: { status: "deferred" },
        },
      });
      showToast("Deferred");
      close();
      await todayFetch.loadDecisions(_state.today, await rebuildClientLookup());
      todayRender.draw(_state.today);
    } catch (e) {
      deferBtn.disabled = false;
      deferBtn.textContent = "Defer";
      showToast(`Update failed: ${e}`);
    }
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

  doneBtn.addEventListener("click", async () => {
    if (!isTauri) {
      showToast("Open the Studio app to update Airtable from here.");
      return;
    }
    doneBtn.disabled = true;
    doneBtn.textContent = "Saving...";
    try {
      await invoke("update_airtable_record", {
        args: {
          table: "Workstreams",
          record_id: workstream.id,
          fields: {
            status: "done",
            last_touch_at: new Date().toISOString(),
          },
        },
      });
      showToast("Workstream marked done");
      close();
      // Refresh both surfaces — the workstream may show up on either.
      if (_state.client?.code) {
        await clientFetch.loadWorkstreams(_state.client);
        clientRender.draw(_state.client);
      }
      await todayFetch.loadWorkstreams(_state.today, await rebuildClientLookup());
      todayRender.draw(_state.today);
    } catch (e) {
      doneBtn.disabled = false;
      doneBtn.textContent = "Mark done";
      showToast(`Update failed: ${e}`);
    }
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
async function showScheduleSocialPostModal(prefillClientCode) {
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
  copyPad.appendChild(copy);
  modal.appendChild(copyPad);

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
      showToast("Open the Studio app to file workflows. Preview is read-only.");
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
      showToast("Open the Studio app to file workflows. Preview is read-only.");
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
      showToast("Open the Studio app to file workflows. Preview is read-only.");
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
  document.getElementById("today-view")?.removeAttribute("hidden");
  document.getElementById("client-view")?.setAttribute("hidden", "");
  const titleEl = document.querySelector(".main-title");
  if (titleEl) titleEl.textContent = "Today";
  // Re-render with whatever's already in state, then refresh stale slices.
  todayRender.draw(_state.today);
  todayFetch.refreshStale(_state.today).then(() => todayRender.draw(_state.today));
  startLiveStatusTimer();
}

async function loadClientView(clientCode) {
  const todayEl = document.getElementById("today-view");
  const clientEl = document.getElementById("client-view");
  if (!clientEl) return;

  // Stop the live-status interval — it only matters on the Today view.
  clearLiveStatusTimer();

  todayEl?.setAttribute("hidden", "");
  clientEl.removeAttribute("hidden");

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

  // Update title to the resolved client name (loadHeader filled it in).
  if (titleEl && _state.client.header?.name) {
    titleEl.textContent = _state.client.header.name;
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
  todayFetch.loadAll(_state.today).then(() => todayRender.draw(_state.today));
  startLiveStatusTimer();

  document.getElementById("today-refresh-btn")?.addEventListener("click", async () => {
    const btn = document.getElementById("today-refresh-btn");
    btn.disabled = true;
    const prev = btn.innerHTML;
    btn.innerHTML = "<span aria-hidden=\"true\">…</span>";
    try {
      await todayFetch.loadAll(_state.today);
      todayRender.draw(_state.today);
      showToast("Today refreshed");
    } catch (e) {
      showToast(`Refresh failed: ${e}`);
    } finally {
      btn.disabled = false;
      btn.innerHTML = prev;
    }
  });

  // Refocus refresh: if the user comes back after >5min, refresh stale
  // slices. Cheap because each fetcher only fires when stale.
  window.addEventListener("focus", async () => {
    if (document.getElementById("today-view")?.hasAttribute("hidden")) return;
    await todayFetch.refreshStale(_state.today);
    todayRender.draw(_state.today);
  });

  // Click handlers dispatched by the Today render layer.
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

  document.addEventListener("today:decision-click", (e) => {
    showDecisionCaptureModal(e.detail?.decision);
  });

  document.addEventListener("today:workstream-click", (e) => {
    const w = e.detail?.workstream;
    if (w?._client_codes?.length) {
      // Tagged to a client — jump to that client view.
      const code = w._client_codes[0];
      activateClientSidebar(code);
      loadClientView(code);
      return;
    }
    // Untagged workstream — open the detail modal in place.
    showWorkstreamDetailModal(w);
  });

  // ─── Per-client view event handlers ─────────────────────────────────
  document.addEventListener("client:workstream-click", (e) => {
    showWorkstreamDetailModal(e.detail?.workstream);
  });

  document.addEventListener("client:decision-click", (e) => {
    showDecisionCaptureModal(e.detail?.decision);
  });

  document.addEventListener("client:commitment-click", (e) => {
    showCommitmentModal(e.detail?.commitment);
  });

  document.addEventListener("client:project-click", (e) => {
    showProjectDetailModal(e.detail?.project);
  });

  document.addEventListener("client:receipt-click", (e) => {
    showReceiptJsonModal(e.detail?.receipt);
  });

  document.addEventListener("client:refresh-click", async () => {
    if (!_state.client?.code) return;
    await clientFetch.loadAll(_state.client, _state.client.code);
    clientRender.draw(_state.client);
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
      },
      toast: showToast,
      requireApiKey: async () => {
        if (!isTauri) {
          showToast("Open the Studio app to run live workflows. Preview is read-only.");
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
      showToast("Reprint runs in the Studio app, not the browser preview.");
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

    // Anything else (Pipeline, Products, Personal items): keep on Today and toast.
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

  async function openStrategicThinking() {
    if (!isTauri) {
      showToast("Open the Studio app (DMG) to run live workflows. Preview is read-only.");
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
      showToast("Open the Studio app to run live workflows. Preview is read-only.");
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
      showToast("Open the Studio app to run live workflows. Preview is read-only.");
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
      showToast("Open the Studio app to run live workflows. Preview is read-only.");
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
      showToast("Open the Studio app to run live workflows. Preview is read-only.");
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
      showToast("Open the Studio app to run live workflows. Preview is read-only.");
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
          showToast("Open the Studio app to file workflows. Preview is read-only.");
          return;
        }
        showScheduleSocialPostModal();
      } else if (name === "Log time") {
        if (!isTauri) {
          showToast("Open the Studio app to file workflows. Preview is read-only.");
          return;
        }
        showLogTimeModal();
      } else if (name === "Edit project") {
        if (!isTauri) {
          showToast("Open the Studio app to file workflows. Preview is read-only.");
          return;
        }
        showEditProjectModal();
      } else {
        showToast(`${name}: not wired yet.`);
      }
    });
  });

  document.getElementById("strategic-thinking-btn")?.addEventListener("click", openStrategicThinking);

  document.getElementById("workflows-btn")?.addEventListener("click", () => {
    document.querySelector(".workflow-starters")?.scrollIntoView({ behavior: "smooth", block: "start" });
  });
});
