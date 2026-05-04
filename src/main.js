// In Cahoots Studio — v0 spike
//
// Renders the Strategic Thinking session receipt as the first
// item in the feed. AI workflow runner wires up in the next build.

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
async function loadStudioSidebar() {
  if (!isTauri) return;
  const studioSection = document.querySelector(".sidebar-section[data-section='studio']");
  if (!studioSection) return;

  let configured = false;
  try { configured = await invoke("get_airtable_status"); } catch {}
  if (!configured) return; // keep static placeholders

  let raw;
  try {
    raw = await invoke("list_airtable_clients");
  } catch (e) {
    console.warn("list_airtable_clients failed:", e);
    return;
  }
  const data = JSON.parse(raw);
  const records = data.records || [];

  // Replace static client items, keep the "Studio" label and Pipeline link.
  // Click handling is delegated globally — items just need data-client-code
  // and data-view attributes set correctly.
  studioSection.innerHTML = "";
  studioSection.appendChild(el("div", { class: "sidebar-label" }, ["Studio"]));
  records.forEach((r) => {
    const f = r.fields || {};
    const code = (f.code || "").toUpperCase();
    if (!code) return;
    const label = `${code} — ${f.name || code}`;
    const item = el("a", {
      class: "sidebar-item",
      "data-view": `client-${code}`,
      "data-client-code": code,
    }, [label]);
    studioSection.appendChild(item);
  });
  studioSection.appendChild(el("a", { class: "sidebar-item", "data-view": "pipeline" }, ["Pipeline"]));
}

// ─── Strategic Thinking modal ────────────────────────────────────────
function showStrategicThinkingModal() {
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
function showNewClientOnboardingModal() {
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

  modal.appendChild(el("div", { class: "settings-label", style: "margin-top: 16px;" }, ["First-call notes"]));
  const notes = el("textarea", {
    id: "nco-notes",
    class: "modal-textarea",
    placeholder: "What they want, what's been tried, who else is involved, what success looks like, what they're worried about. Notes from your discovery call go here verbatim.",
    rows: 8,
  });
  modal.appendChild(notes);

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

  modal.appendChild(el("div", { class: "settings-label", style: "margin-top: 16px;" }, ["Brief notes"]));
  const notes = el("textarea", {
    id: "ncs-notes",
    class: "modal-textarea",
    placeholder: "What the campaign is, what success looks like, what you've already discussed with the client, what you're worried about. Notes from your scoping call go here verbatim.",
    rows: 8,
  });
  modal.appendChild(notes);

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
function showSubcontractorOnboardingModal() {
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

  modal.appendChild(el("div", { class: "settings-label", style: "margin-top: 16px;" }, ["Notes"]));
  const notes = el("textarea", {
    class: "modal-textarea",
    placeholder: "Why they're joining, what they're best at, what they're new to, what hours/availability looks like, any context on previous work or interview impressions.",
    rows: 6,
  });
  modal.appendChild(notes);

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

  modal.appendChild(el("div", { class: "settings-label", style: "margin-top: 16px;" }, ["Anything to flag (optional)"]));
  const flags = el("textarea", {
    class: "modal-textarea",
    placeholder: flagsPlaceholder,
    rows: 5,
  });
  modal.appendChild(flags);

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
}

async function loadClientView(clientCode) {
  const todayEl = document.getElementById("today-view");
  const clientEl = document.getElementById("client-view");
  if (!clientEl) return;

  todayEl?.setAttribute("hidden", "");
  clientEl.removeAttribute("hidden");
  clientEl.innerHTML = "";
  clientEl.appendChild(el("div", { class: "client-empty" }, [`Loading ${clientCode}...`]));

  // Pull client metadata, projects, and local receipts in parallel.
  let clientFields = {};
  let projects = [];
  let localReceipts = [];

  if (isTauri) {
    const tasks = [
      invoke("list_airtable_clients").then((raw) => {
        const data = JSON.parse(raw);
        const match = (data.records || []).find((r) => r.fields?.code === clientCode);
        if (match) clientFields = match.fields;
      }).catch((e) => console.warn("client lookup failed:", e)),

      invoke("list_airtable_projects").then((raw) => {
        const data = JSON.parse(raw);
        projects = (data.records || [])
          .map((r) => r.fields || {})
          .filter((f) => (f.code || "").toUpperCase().startsWith(`${clientCode}-`));
      }).catch((e) => console.warn("projects lookup failed:", e)),

      invoke("list_receipts", { limit: 200 }).then((rows) => {
        const all = rows.map((j) => {
          try { return JSON.parse(j); } catch { return null; }
        }).filter(Boolean);
        localReceipts = all.filter((r) => {
          const proj = (r.project || "").toUpperCase();
          return proj === clientCode || proj.startsWith(`${clientCode}-`);
        });
      }).catch((e) => console.warn("local receipts lookup failed:", e)),
    ];
    await Promise.all(tasks);
  }

  const clientName = clientFields.name || clientCode;
  const status = clientFields.status || "active";

  // Re-render the view.
  clientEl.innerHTML = "";
  clientEl.appendChild(renderClientHeader(clientCode, clientName, status, clientFields));
  clientEl.appendChild(el("div", { class: "section-label" }, ["Run for this client"]));
  clientEl.appendChild(renderClientShortcuts(clientCode));

  clientEl.appendChild(el("div", { class: "section-label" }, ["Active projects"]));
  if (projects.length === 0) {
    clientEl.appendChild(el("div", { class: "client-empty" }, [
      `No projects in Airtable yet for ${clientCode}. Run New Campaign Scope to create one.`,
    ]));
  } else {
    const list = el("div", { class: "client-projects" });
    projects.forEach((p) => {
      list.appendChild(el("div", { class: "client-project" }, [
        el("div", {}, [
          el("div", { class: "client-project-code" }, [p.code || ""]),
          el("div", { class: "client-project-name" }, [p.name || ""]),
        ]),
        el("div", { class: "client-project-status" }, [p.status || ""]),
      ]));
    });
    clientEl.appendChild(list);
  }

  clientEl.appendChild(el("div", { class: "section-label" }, [`Receipts for ${clientCode}`]));
  if (localReceipts.length === 0) {
    clientEl.appendChild(el("div", { class: "client-empty" }, [
      "No receipts for this client yet. Run a workflow above to generate the first one.",
    ]));
  } else {
    const feed = el("section", { class: "feed" });
    localReceipts.forEach((r) => feed.appendChild(renderReceipt(r)));
    clientEl.appendChild(feed);
  }

  const titleEl = document.querySelector(".main-title");
  if (titleEl) titleEl.textContent = clientName;
}

function renderClientHeader(code, name, status, fields) {
  const header = el("div", { class: "client-header" });
  header.appendChild(el("div", { class: "client-code" }, [code]));
  header.appendChild(el("div", { class: "client-name" }, [name]));

  const meta = el("div", { class: "client-meta" });
  meta.appendChild(el("span", { class: `client-status-pill status-${status}` }, [status]));
  if (fields.primary_contact_email) {
    meta.appendChild(el("a", { href: `mailto:${fields.primary_contact_email}` }, [fields.primary_contact_email]));
  }
  if (fields.abn) {
    meta.appendChild(el("span", {}, [`ABN ${fields.abn}`]));
  }
  if (fields.dropbox_folder) {
    meta.appendChild(el("a", { href: fields.dropbox_folder, target: "_blank", rel: "noopener" }, ["Dropbox"]));
  }
  header.appendChild(meta);
  return header;
}

function renderClientShortcuts(clientCode) {
  const grid = el("div", { class: "client-shortcut-grid" });

  const shortcuts = [
    {
      title: "Strategic Thinking",
      meta: "Open thinking session, no client lock-in",
      action: () => showStrategicThinkingModal(),
    },
    {
      title: "Monthly Check-in",
      meta: "Pre-fill this client",
      action: () => showMonthlyCheckinModal(clientCode),
    },
    {
      title: "Quarterly Review",
      meta: "Pre-fill this client",
      action: () => showQuarterlyReviewModal(clientCode),
    },
    {
      title: "New Campaign Scope",
      meta: "Pre-fill this client",
      action: () => showNewCampaignScopeModal(clientCode),
    },
  ];

  shortcuts.forEach((s) => {
    const card = el("button", { class: "client-shortcut" }, [
      el("div", { class: "client-shortcut-title" }, [s.title]),
      el("div", { class: "client-shortcut-meta" }, [s.meta]),
    ]);
    card.addEventListener("click", async () => {
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
      s.action();
    });
    grid.appendChild(card);
  });

  return grid;
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
      } else {
        showToast(`${name}: workflow runner lands next build.`);
      }
    });
  });

  document.getElementById("strategic-thinking-btn")?.addEventListener("click", openStrategicThinking);

  document.getElementById("workflows-btn")?.addEventListener("click", () => {
    document.querySelector(".workflow-starters")?.scrollIntoView({ behavior: "smooth", block: "start" });
  });
});
