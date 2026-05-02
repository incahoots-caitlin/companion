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
  studioSection.innerHTML = "";
  studioSection.appendChild(el("div", { class: "sidebar-label" }, ["Studio"]));
  records.forEach((r) => {
    const f = r.fields || {};
    const label = f.code ? `${f.code} — ${f.name || f.code}` : (f.name || "Untitled");
    const item = el("a", {
      class: "sidebar-item",
      "data-view": `client-${(f.code || "").toLowerCase()}`,
    }, [label]);
    item.addEventListener("click", () => {
      document.querySelectorAll(".sidebar-item").forEach((i) => {
        i.classList.remove("active");
        i.removeAttribute("aria-current");
      });
      item.classList.add("active");
      item.setAttribute("aria-current", "page");
      const titleEl = document.querySelector(".main-title");
      if (titleEl) titleEl.textContent = label;
      showToast(`${f.code || f.name}: per-client view lands in v0.7+`);
    });
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
      showToast(onDone ? `Ticked, hook fired (${onDone})` : "Ticked");
    } catch (err) {
      e.target.disabled = false;
      e.target.checked = false;
      row.classList.remove("done");
      showToast(`Tick failed: ${err}`, { ttl: 5000 });
    }
  });

  // Sidebar routing.
  const titleEl = document.querySelector(".main-title");
  document.querySelectorAll(".sidebar-item").forEach((item) => {
    item.addEventListener("click", () => {
      const view = item.dataset.view || "today";

      // Settings opens a modal instead of routing.
      if (view === "settings") {
        showSettingsModal();
        return;
      }

      const label = item.textContent.trim();
      document.querySelectorAll(".sidebar-item").forEach((i) => {
        i.classList.remove("active");
        i.removeAttribute("aria-current");
      });
      item.classList.add("active");
      item.setAttribute("aria-current", "page");
      if (titleEl) titleEl.textContent = label;
      if (view !== "today") {
        showToast(`${label} isn't wired yet. Today is the live view this build.`);
      }
    });
  });

  // First-launch API key check (Tauri only).
  ensureApiKey();

  // Airtable: hydrate sidebar with active clients if connected.
  loadStudioSidebar();

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

  // Workflow cards: Strategic Thinking opens the modal, others toast.
  document.querySelectorAll(".workflow-card").forEach((card) => {
    const name = card.querySelector(".workflow-card-title")?.textContent || "Workflow";
    card.addEventListener("click", () => {
      if (name === "Strategic Thinking") {
        openStrategicThinking();
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
