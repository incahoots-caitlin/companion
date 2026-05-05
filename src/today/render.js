// Today dashboard — rendering.
//
// Pure-ish render function: read _state.today, mount markup into the
// section containers in index.html. No fetch in here. Click handlers
// dispatch CustomEvents on the section root so main.js can wire modals
// without us reaching into the global namespace.
//
// All emoji prefixes match Caitlin's scheduled-task digest style.

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v == null) continue;
    if (k === "class") node.className = v;
    else if (k === "style") node.setAttribute("style", v);
    else if (k === "data") {
      for (const [dk, dv] of Object.entries(v)) {
        if (dv != null) node.dataset[dk] = dv;
      }
    } else if (k in node) node[k] = v;
    else node.setAttribute(k, v);
  }
  for (const c of children) {
    if (c == null || c === false) continue;
    if (typeof c === "string") node.appendChild(document.createTextNode(c));
    else node.appendChild(c);
  }
  return node;
}

function fmtDateTime(s) {
  if (!s) return "";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
  // "Tue 5 May, 5:30pm" feel — short and human.
  const day = d.toLocaleDateString("en-AU", { weekday: "short", day: "numeric", month: "short" });
  const time = d.toLocaleTimeString("en-AU", { hour: "numeric", minute: "2-digit", hour12: true })
    .replace(/\s/g, "")
    .toLowerCase();
  return `${day}, ${time}`;
}

function fmtTime(s) {
  if (!s) return "";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return "";
  return d
    .toLocaleTimeString("en-AU", { hour: "numeric", minute: "2-digit", hour12: true })
    .replace(/\s/g, "")
    .toLowerCase();
}

function fmtDate(s) {
  if (!s) return "";
  // Date-only or datetime — shorten either way.
  const d = new Date(s.length === 10 ? `${s}T00:00:00` : s);
  if (Number.isNaN(d.getTime())) return s;
  return d.toLocaleDateString("en-AU", { weekday: "short", day: "numeric", month: "short" });
}

function dispatch(name, detail) {
  document.dispatchEvent(new CustomEvent(name, { detail }));
}

// ── Section: due today ────────────────────────────────────────────────

function renderDueToday(state) {
  const root = el("section", { class: "today-section", "data-section": "due-today" });
  root.appendChild(el("div", { class: "section-label" }, ["⚠ DUE TODAY"]));

  const commitments = state.due_today?.commitments || [];
  const decisions = state.due_today?.decisions || [];

  if (commitments.length === 0 && decisions.length === 0) {
    root.appendChild(el("div", { class: "empty" }, ["Nothing due today. Clear day."]));
    return root;
  }

  const counts = el("div", { class: "today-section-meta" }, [
    `${commitments.length} commitment${commitments.length === 1 ? "" : "s"} · ${decisions.length} decision${decisions.length === 1 ? "" : "s"}`,
  ]);
  root.appendChild(counts);

  const list = el("div", { class: "today-list" });
  commitments.forEach((c) => list.appendChild(renderCommitmentRow(c)));
  decisions.forEach((d) => list.appendChild(renderDecisionRow(d, { compact: true })));
  root.appendChild(list);
  return root;
}

function renderCommitmentRow(c) {
  const row = el("button", {
    class: "today-row today-row-commitment",
    "data-commitment-id": c.id || "",
    "data-record-id": c.id || "",
    type: "button",
  });
  row.addEventListener("click", () => dispatch("today:commitment-click", { commitment: c }));

  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [c.title || "(untitled)"]),
    c.notes ? el("div", { class: "today-row-meta" }, [c.notes]) : null,
  ]);
  const right = el("div", { class: "today-row-side" });
  (c._client_codes || []).forEach((code) => {
    right.appendChild(el("span", { class: "client-status-pill status-active" }, [code]));
  });
  if (c.due_at) right.appendChild(el("span", { class: "today-row-time" }, [fmtTime(c.due_at)]));

  row.appendChild(left);
  row.appendChild(right);
  return row;
}

function renderDecisionRow(d, { compact = false } = {}) {
  const row = el("button", {
    class: "today-row today-row-decision",
    "data-decision-id": d.id || "",
    "data-record-id": d.id || "",
    type: "button",
  });
  row.addEventListener("click", () => dispatch("today:decision-click", { decision: d }));

  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [d.title || "(untitled)"]),
    !compact && d.due_date
      ? el("div", { class: "today-row-meta" }, [`Due ${fmtDate(d.due_date)}`])
      : null,
  ]);
  const right = el("div", { class: "today-row-side" });
  if (d.decision_type) {
    right.appendChild(el("span", { class: "pill" }, [d.decision_type]));
  }
  (d._client_codes || []).forEach((code) => {
    right.appendChild(el("span", { class: "client-status-pill status-active" }, [code]));
  });
  if (compact && d.due_date) {
    right.appendChild(el("span", { class: "today-row-time" }, ["today"]));
  }

  row.appendChild(left);
  row.appendChild(right);
  return row;
}

// ── Section: overdue ──────────────────────────────────────────────────

function renderOverdue(state) {
  const overdue = state.overdue || [];
  if (overdue.length === 0) return null; // hide section entirely

  const root = el("section", { class: "today-section", "data-section": "overdue" });
  root.appendChild(el("div", { class: "section-label section-label-warn" }, ["🔥 OVERDUE"]));
  const list = el("div", { class: "today-list" });
  overdue.forEach((c) => {
    const row = renderCommitmentRow(c);
    // Override the time-of-day chip with a "X days overdue" tag.
    const sideChips = row.querySelector(".today-row-side");
    const dueAt = new Date(c.due_at);
    const days = Math.floor((Date.now() - dueAt.getTime()) / (1000 * 60 * 60 * 24));
    if (sideChips) {
      const overdueTag = el("span", { class: "client-status-pill status-warn" }, [
        days <= 0 ? "today" : days === 1 ? "1 day overdue" : `${days} days overdue`,
      ]);
      // Replace the time chip if present.
      const timeChip = sideChips.querySelector(".today-row-time");
      if (timeChip) timeChip.replaceWith(overdueTag);
      else sideChips.appendChild(overdueTag);
    }
    list.appendChild(row);
  });
  root.appendChild(list);
  return root;
}

// ── Section: active workstreams ───────────────────────────────────────

function renderWorkstreams(state) {
  const root = el("section", { class: "today-section", "data-section": "workstreams" });
  root.appendChild(el("div", { class: "section-label" }, ["📌 ACTIVE WORKSTREAMS"]));
  const items = state.workstreams || [];
  if (items.length === 0) {
    root.appendChild(
      el("div", { class: "empty" }, [
        "No active workstreams. Scope something or take a recovery week.",
      ])
    );
    return root;
  }
  const list = el("div", { class: "today-list" });
  items.forEach((w) => list.appendChild(renderWorkstreamRow(w)));
  root.appendChild(list);
  return root;
}

function renderWorkstreamRow(w) {
  const row = el("button", {
    class: "today-row today-row-workstream",
    "data-workstream-code": w.code || "",
    type: "button",
  });
  row.addEventListener("click", () =>
    dispatch("today:workstream-click", { workstream: w })
  );

  const code = el("span", { class: "today-row-code" }, [w.code || ""]);
  const title = el("span", { class: "today-row-title" }, [w.title || ""]);
  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title-line" }, [code, title]),
    w.next_action
      ? el("div", { class: "today-row-meta" }, [`Next: ${w.next_action}`])
      : w.blocker
      ? el("div", { class: "today-row-meta" }, [`Blocked: ${w.blocker}`])
      : null,
  ]);

  const right = el("div", { class: "today-row-side" });
  if (w.phase) {
    right.appendChild(
      el("span", { class: `client-status-pill status-${w.phase}` }, [w.phase])
    );
  }
  if (w.status === "blocked") {
    right.appendChild(el("span", { class: "client-status-pill status-warn" }, ["blocked"]));
  }
  (w._client_codes || []).forEach((code) => {
    right.appendChild(el("span", { class: "client-status-pill status-active" }, [code]));
  });

  row.appendChild(left);
  row.appendChild(right);
  return row;
}

// ── Section: open decisions ───────────────────────────────────────────

function renderDecisions(state) {
  const items = state.decisions_open || [];
  if (items.length === 0) return null; // hide section
  const root = el("section", { class: "today-section", "data-section": "decisions" });
  root.appendChild(el("div", { class: "section-label" }, ["🧠 OPEN DECISIONS"]));
  const list = el("div", { class: "today-list" });
  items.forEach((d) => list.appendChild(renderDecisionRow(d)));
  root.appendChild(list);
  return root;
}

// ── Section: drift ────────────────────────────────────────────────────

const DRIFT_ICON = { high: "🔴", medium: "🟡", low: "🟢" };
const DRIFT_RANK = { high: 0, medium: 1, low: 2 };

function renderDrift(state) {
  const items = state.drift || [];
  if (items.length === 0) return null; // hide entire section

  const sorted = items.slice().sort((a, b) => {
    const ar = DRIFT_RANK[a.severity] ?? 9;
    const br = DRIFT_RANK[b.severity] ?? 9;
    return ar - br;
  });

  const root = el("section", { class: "today-section", "data-section": "drift" });
  root.appendChild(el("div", { class: "section-label section-label-warn" }, ["⚠ DRIFT"]));
  const list = el("div", { class: "today-list" });
  sorted.forEach((d) => {
    const icon = DRIFT_ICON[d.severity] || "";
    const sevLabel = (d.severity || "").charAt(0).toUpperCase() + (d.severity || "").slice(1);
    const row = el("button", {
      class: "today-row today-row-drift",
      type: "button",
      "data-severity": d.severity || "",
    });
    row.addEventListener("click", () => {
      // For now: console log. Future: deep link to fix.
      console.log("drift:", d);
    });
    const left = el("div", { class: "today-row-main" }, [
      el("div", { class: "today-row-title" }, [
        `${icon} [${sevLabel}] ${d.title || ""}`,
      ]),
      d.action ? el("div", { class: "today-row-meta" }, [`→ ${d.action}`]) : null,
    ]);
    row.appendChild(left);
    list.appendChild(row);
  });
  root.appendChild(list);
  return root;
}

// ── Section: live status ──────────────────────────────────────────────

function renderLiveStatus(state) {
  const ls = state.live_status || {};
  const root = el("section", { class: "today-section", "data-section": "live-status" });
  root.appendChild(el("div", { class: "section-label" }, ["📊 LIVE STATUS"]));

  const grid = el("div", { class: "live-status-grid" });

  // Studio version — clickable, opens release URL.
  const v = ls.studio?.version || "loading...";
  const studioLine = el("button", {
    class: "live-status-row clickable",
    type: "button",
    "data-action": "open-studio-release",
  }, [
    el("span", { class: "live-status-label" }, ["Studio"]),
    el("span", { class: "live-status-value" }, [`v${v}`]),
  ]);
  studioLine.addEventListener("click", () =>
    dispatch("today:open-url", {
      url: `https://github.com/incahoots-caitlin/studio/releases/tag/v${v}`,
    })
  );
  grid.appendChild(studioLine);

  // Context deploy — placeholder (see fetch.loadContextDeploy).
  grid.appendChild(
    el("div", { class: "live-status-row" }, [
      el("span", { class: "live-status-label" }, ["Context deploy"]),
      el("span", { class: "live-status-value live-status-muted" }, [
        ls.context?.error ? "(unavailable)" : "(check manually)",
      ]),
    ])
  );

  // GitHub Actions — colour-coded.
  const ga = ls.github_actions;
  let gaLabel = "(loading)";
  let gaClass = "live-status-muted";
  if (ga?.error) {
    gaLabel = "(unavailable)";
  } else if (ga) {
    if (ga.status === "in_progress" || ga.status === "queued") {
      gaLabel = "⚠ running";
      gaClass = "live-status-warn";
    } else if (ga.conclusion === "success") {
      gaLabel = "✅ green";
      gaClass = "live-status-ok";
    } else if (ga.conclusion === "failure" || ga.conclusion === "timed_out") {
      gaLabel = "🔴 red";
      gaClass = "live-status-bad";
    } else if (ga.conclusion === "cancelled") {
      gaLabel = "⚠ cancelled";
      gaClass = "live-status-warn";
    } else {
      gaLabel = ga.conclusion || ga.status || "unknown";
    }
  }
  const gaRow = el(
    "button",
    {
      class: "live-status-row clickable",
      type: "button",
      "data-action": "open-actions",
    },
    [
      el("span", { class: "live-status-label" }, ["GitHub Actions"]),
      el("span", { class: `live-status-value ${gaClass}` }, [gaLabel]),
    ]
  );
  gaRow.addEventListener("click", () => {
    const url =
      ga?.url ||
      "https://github.com/incahoots-caitlin/studio/actions?query=branch%3Amain";
    dispatch("today:open-url", { url });
  });
  grid.appendChild(gaRow);

  // contextfor.me uptime.
  const ctx = ls.contextfor_me;
  let cLabel = "(checking)";
  let cClass = "live-status-muted";
  if (ctx?.error) {
    cLabel = "(unavailable)";
  } else if (ctx) {
    cLabel = ctx.ok ? "✅ up" : "🔴 down";
    cClass = ctx.ok ? "live-status-ok" : "live-status-bad";
  }
  const cRow = el(
    "button",
    { class: "live-status-row clickable", type: "button" },
    [
      el("span", { class: "live-status-label" }, ["contextfor.me"]),
      el("span", { class: `live-status-value ${cClass}` }, [cLabel]),
    ]
  );
  cRow.addEventListener("click", () =>
    dispatch("today:open-url", { url: "https://contextfor.me" })
  );
  grid.appendChild(cRow);

  root.appendChild(grid);
  return root;
}

// ── Section: receipts pending tick ────────────────────────────────────

function renderReceiptsPending(state) {
  const items = state.receipts_pending || [];
  const root = el("section", { class: "today-section", "data-section": "receipts-pending" });
  root.appendChild(el("div", { class: "section-label" }, ["📃 RECENT RECEIPTS PENDING TICK"]));
  if (items.length === 0) {
    root.appendChild(el("div", { class: "empty" }, ["All receipts ticked through."]));
    return root;
  }
  const list = el("div", { class: "today-list" });
  items.forEach((r) => {
    const row = el("div", { class: "today-row" }, [
      el("div", { class: "today-row-main" }, [
        el("div", { class: "today-row-title" }, [r.title]),
        el("div", { class: "today-row-meta" }, [
          `${r.workflow ? r.workflow + " · " : ""}${fmtDate(r.date)}`,
        ]),
      ]),
      el("div", { class: "today-row-side" }, [
        ...(r._client_codes || []).map((code) =>
          el("span", { class: "client-status-pill status-active" }, [code])
        ),
        el("span", { class: "today-row-tick-count" }, [
          `${r.ticked} of ${r.total} ticked`,
        ]),
      ]),
    ]);
    list.appendChild(row);
  });
  root.appendChild(list);
  return root;
}

// ── Section: morning briefing ─────────────────────────────────────────

function renderMorningBriefing(state) {
  const b = state.morning_briefing;
  if (!b || !b.text) return null;
  const root = el("section", { class: "today-section", "data-section": "morning-briefing" });
  root.appendChild(el("div", { class: "section-label" }, ["🌅 MORNING BRIEFING"]));
  const generatedAt = b.generated_at
    ? new Date(b.generated_at * 1000).toLocaleString("en-AU", {
        weekday: "short",
        day: "numeric",
        month: "short",
        hour: "numeric",
        minute: "2-digit",
        hour12: true,
      })
    : "";
  if (generatedAt) {
    root.appendChild(el("div", { class: "today-section-meta" }, [generatedAt]));
  }
  const pre = el("pre", { class: "today-briefing-body" }, [b.text]);
  root.appendChild(pre);
  return root;
}

// ── Top-level draw ────────────────────────────────────────────────────

export function draw(state) {
  const container = document.getElementById("today-sections");
  if (!container) return;
  container.innerHTML = "";

  const sections = [
    renderDueToday(state),
    renderOverdue(state),
    renderDrift(state),
    renderWorkstreams(state),
    renderDecisions(state),
    renderLiveStatus(state),
    renderReceiptsPending(state),
    renderMorningBriefing(state),
  ];
  sections.forEach((s) => {
    if (s) container.appendChild(s);
  });
}

// Re-render a single live-status row (cheap, called every 60s by main.js).
export function drawLiveStatusOnly(state) {
  const container = document.getElementById("today-sections");
  if (!container) return;
  const existing = container.querySelector('[data-section="live-status"]');
  const fresh = renderLiveStatus(state);
  if (existing) existing.replaceWith(fresh);
  else container.appendChild(fresh);
}
