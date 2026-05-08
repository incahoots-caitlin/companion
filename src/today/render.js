// Today dashboard — rendering.
//
// Pure-ish render function: read _state.today, mount markup into the
// section containers in index.html. No fetch in here. Click handlers
// dispatch CustomEvents on the section root so main.js can wire modals
// without us reaching into the global namespace.
//
// All emoji prefixes match Caitlin's scheduled-task digest style.

import { skillsForContext } from "../skills/registry.js";

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

// v0.42: empty-state hero with cowboys illustration + grass-stroke pattern
// behind it. Used when a Today section is genuinely empty (clear day,
// nothing overdue). Once per surface — never repeating, never on row cards.
function renderEmptyHero(headlineText, metaText) {
  const cowboys = el("img", {
    class: "illustration-cowboys illustration-cowboys-cloud",
    src: "illustrations/cowboys-cloud.png",
    alt: "",
    "aria-hidden": "true",
  });
  const children = [
    cowboys,
    el("div", { class: "empty-state-text" }, [headlineText]),
  ];
  if (metaText) {
    children.push(el("div", { class: "empty-state-text-meta" }, [metaText]));
  }
  return el("div", { class: "empty-state today-empty-hero illustration-grass-pattern" }, children);
}

// ── Section: due today ────────────────────────────────────────────────

function renderDueToday(state) {
  const root = el("section", { class: "today-section", "data-section": "due-today" });
  root.appendChild(el("div", { class: "section-label" }, ["⚠ DUE TODAY"]));

  const commitments = state.due_today?.commitments || [];
  const decisions = state.due_today?.decisions || [];

  if (commitments.length === 0 && decisions.length === 0) {
    root.appendChild(renderEmptyHero(
      "Nothing due today.",
      "Clear day. Maybe go for a walk."
    ));
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

// ── Section: email triage (v0.25) ─────────────────────────────────────
//
// "N unread, M flagged urgent." Hidden when Google isn't connected or
// the gmail scope wasn't granted. Click anywhere on the row opens the
// Gmail inbox in the browser.

function renderEmail(state) {
  const e = state.email || {};
  // Hide entirely when unread is null — that's the "not connected /
  // no gmail scope" state from loadEmail.
  if (e.unread == null && (!e.urgent || e.urgent.length === 0) && !e.error) {
    return null;
  }
  const root = el("section", { class: "today-section", "data-section": "email" });
  root.appendChild(el("div", { class: "section-label" }, ["📨 EMAIL TRIAGE"]));

  if (e.error && e.unread == null) {
    root.appendChild(
      el("div", { class: "empty" }, [
        "Gmail unavailable — try the refresh button.",
      ])
    );
    return root;
  }

  const unread = Number(e.unread || 0);
  const urgent = Array.isArray(e.urgent) ? e.urgent : [];

  const headline = `${unread} unread${unread === 1 ? "" : ""}, ${urgent.length} flagged urgent.`;
  const headlineRow = el("button", {
    class: "today-row clickable",
    type: "button",
  }, [
    el("div", { class: "today-row-main" }, [
      el("div", { class: "today-row-title" }, [headline]),
      el("div", { class: "today-row-meta" }, [
        "Click to open Gmail",
      ]),
    ]),
  ]);
  headlineRow.addEventListener("click", () =>
    dispatch("today:open-url", { url: "https://mail.google.com/mail/u/0/#inbox" })
  );

  const list = el("div", { class: "today-list" });
  list.appendChild(headlineRow);
  urgent.slice(0, 5).forEach((t) => list.appendChild(renderEmailRow(t)));
  root.appendChild(list);
  return root;
}

function renderEmailRow(t) {
  const row = el("button", {
    class: "today-row today-row-email",
    type: "button",
    "data-thread-id": t.id || "",
  });
  row.addEventListener("click", () =>
    dispatch("today:open-url", { url: t.web_link })
  );
  // Sender display: take just the name portion if the From header is
  // "Name <addr>", else show the bare address.
  const senderDisplay = (t.from || "").replace(/<.*?>/, "").trim() || t.from || "";
  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [t.subject || "(no subject)"]),
    el("div", { class: "today-row-meta" }, [
      senderDisplay,
      ...(t.snippet ? [` · ${t.snippet.slice(0, 100)}`] : []),
    ]),
  ]);
  const right = el("div", { class: "today-row-side" });
  if (t.starred) {
    right.appendChild(el("span", { class: "client-status-pill status-warn" }, ["★"]));
  }
  if (t.unread) {
    right.appendChild(el("span", { class: "client-status-pill status-active" }, ["unread"]));
  }
  row.appendChild(left);
  row.appendChild(right);
  return row;
}

// ── Section: Slack activity (v0.26) ───────────────────────────────────
//
// Hidden when Slack OAuth isn't connected. Otherwise: top channels by
// unread count. Click opens the Slack desktop app at that channel.
// Empty list (connected but nothing unread) collapses cleanly to a
// single "all caught up" line.

function renderSlack(state) {
  const s = state.slack || {};
  if (!s.status?.connected) return null; // hide entirely

  const root = el("section", { class: "today-section", "data-section": "slack" });
  root.appendChild(el("div", { class: "section-label" }, ["💬 SLACK ACTIVITY"]));

  const unreads = Array.isArray(s.unreads) ? s.unreads : [];
  if (s.error && unreads.length === 0) {
    root.appendChild(
      el("div", { class: "empty" }, [
        "Slack unavailable — try the refresh button.",
      ])
    );
    return root;
  }

  if (unreads.length === 0) {
    root.appendChild(el("div", { class: "empty" }, ["All channels caught up."]));
    return root;
  }

  const total = unreads.reduce((acc, u) => acc + (Number(u.unread_count) || 0), 0);
  root.appendChild(
    el("div", { class: "today-section-meta" }, [
      `${total} unread across ${unreads.length} channel${unreads.length === 1 ? "" : "s"}`,
    ])
  );

  const list = el("div", { class: "today-list" });
  unreads.forEach((u) => list.appendChild(renderSlackChannelRow(u)));
  root.appendChild(list);
  return root;
}

function renderSlackChannelRow(u) {
  const ch = u.channel || {};
  const row = el("button", {
    class: "today-row",
    type: "button",
    "data-channel-id": ch.id || "",
  });
  row.addEventListener("click", () => {
    // Prefer the deeplink — if Slack desktop is running it'll handle it;
    // when not, the URL just falls through to the system handler. Web
    // link is the explicit fallback.
    const url = ch.deeplink || ch.web_link;
    if (url) dispatch("today:open-url", { url });
  });

  const last = u.last_message;
  const senderName = last?.user_name || (last?.user ? `<@${last.user}>` : "");
  const snippet = last?.text ? last.text.slice(0, 100) : "";
  const metaBits = [];
  if (senderName) metaBits.push(senderName);
  if (snippet) metaBits.push(snippet);

  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [`#${ch.name || "channel"}`]),
    metaBits.length
      ? el("div", { class: "today-row-meta" }, [metaBits.join(" · ")])
      : null,
  ]);
  const right = el("div", { class: "today-row-side" }, [
    ch.is_private
      ? el("span", { class: "client-status-pill" }, ["private"])
      : null,
    el("span", { class: "client-status-pill status-active" }, [
      `${u.unread_count} unread`,
    ]),
  ]);

  row.appendChild(left);
  row.appendChild(right);
  return row;
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
    el("span", { class: "live-status-label" }, ["Companion"]),
    el("span", { class: "live-status-value" }, [`v${v}`]),
  ]);
  studioLine.addEventListener("click", () =>
    dispatch("today:open-url", {
      url: `https://github.com/incahoots-caitlin/companion/releases/tag/v${v}`,
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

  // v0.37 Block F — Studio CFO margin this month.
  const margin = ls.margin;
  let mLabel = "(loading)";
  let mClass = "live-status-muted";
  if (margin?.error) {
    mLabel = "(unavailable)";
  } else if (margin && typeof margin.margin === "number") {
    const m = Math.round(margin.margin);
    mLabel = "$" + m.toLocaleString("en-AU");
    mClass = m >= 0 ? "live-status-ok" : "live-status-bad";
  }
  const mRow = el(
    "button",
    { class: "live-status-row clickable", type: "button" },
    [
      el("span", { class: "live-status-label" }, ["Margin this month"]),
      el("span", { class: `live-status-value ${mClass}` }, [mLabel]),
    ]
  );
  mRow.addEventListener("click", () =>
    dispatch("today:open-cfo", {})
  );
  grid.appendChild(mRow);

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

// ── Section: calendar (v0.24) ─────────────────────────────────────────
//
// Today's events on top, week-ahead summary collapsible underneath.
// Events render with start time, summary, location/attendees one-liner,
// and a Meet link when present.

function fmtEventTime(ev) {
  if (ev.all_day) return "All day";
  return fmtTime(ev.start);
}

function fmtEventDayHeader(s) {
  // s is RFC3339 or YYYY-MM-DD. Normalise to a display string like
  // "Tue 6 May".
  if (!s) return "";
  const d = new Date(s.length === 10 ? `${s}T00:00:00` : s);
  if (Number.isNaN(d.getTime())) return s;
  return d.toLocaleDateString("en-AU", { weekday: "short", day: "numeric", month: "short" });
}

function eventDayKey(s) {
  // Group week events by their start date in local time. RFC3339 with
  // a timezone is already date-bearing; for all-day YYYY-MM-DD strings
  // we keep them as-is.
  if (!s) return "";
  if (s.length === 10) return s;
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return "";
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function renderEventRow(ev) {
  const row = el("button", {
    class: "today-row today-row-event",
    type: "button",
    "data-event-id": ev.id || "",
  });
  if (ev.html_link) {
    row.addEventListener("click", () =>
      dispatch("today:open-url", { url: ev.html_link })
    );
  }

  const titleParts = [];
  if (ev.summary) titleParts.push(ev.summary);
  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [ev.summary || "(no title)"]),
  ]);
  const metaBits = [];
  if (ev.location && !/^https?:\/\//i.test(ev.location)) metaBits.push(ev.location);
  if (ev.attendees && ev.attendees.length) {
    const n = ev.attendees.length;
    metaBits.push(`${n} attendee${n === 1 ? "" : "s"}`);
  }
  if (ev.calendar_name && ev.calendar_name !== "primary") {
    metaBits.push(ev.calendar_name);
  }
  if (metaBits.length) {
    left.appendChild(el("div", { class: "today-row-meta" }, [metaBits.join(" · ")]));
  }

  const right = el("div", { class: "today-row-side" });
  if (ev.hangout_link) {
    const meetBtn = el("a", {
      class: "client-status-pill status-active",
      href: ev.hangout_link,
      target: "_blank",
      rel: "noopener",
    }, ["Meet"]);
    meetBtn.addEventListener("click", (e) => {
      // Stop the row's open-html-link bubble.
      e.stopPropagation();
    });
    right.appendChild(meetBtn);
  }
  right.appendChild(el("span", { class: "today-row-time" }, [fmtEventTime(ev)]));

  row.appendChild(left);
  row.appendChild(right);
  return row;
}

function renderCalendar(state) {
  const cal = state.calendar || {};
  const root = el("section", { class: "today-section", "data-section": "calendar" });
  root.appendChild(el("div", { class: "section-label" }, ["📅 CALENDAR"]));

  if (!cal.status?.connected) {
    root.appendChild(
      el("div", { class: "empty" }, ["Connect Google in Settings to see calendar."])
    );
    return root;
  }

  if (cal.error && (!cal.today || cal.today.length === 0)) {
    root.appendChild(
      el("div", { class: "empty" }, [
        "Calendar unavailable — try the refresh button. (",
        String(cal.error).slice(0, 200),
        ")",
      ])
    );
    return root;
  }

  const today = cal.today || [];
  const list = el("div", { class: "today-list" });
  if (today.length === 0) {
    list.appendChild(el("div", { class: "empty" }, ["Nothing on today."]));
  } else {
    today.forEach((ev) => list.appendChild(renderEventRow(ev)));
  }
  root.appendChild(list);

  // Week-ahead summary. We exclude today's events (already shown above)
  // and group the remaining events by day so it stays scannable.
  const todayKey = eventDayKey(new Date().toISOString());
  const weekFuture = (cal.week || []).filter((ev) => eventDayKey(ev.start) !== todayKey);

  const details = el("details", { class: "today-week-ahead" });
  const summary = el("summary", { class: "today-week-ahead-summary" }, [
    weekFuture.length === 0
      ? "Clear week ahead."
      : `Week ahead — ${weekFuture.length} event${weekFuture.length === 1 ? "" : "s"}`,
  ]);
  details.appendChild(summary);

  if (weekFuture.length > 0) {
    const grouped = new Map();
    weekFuture.forEach((ev) => {
      const k = eventDayKey(ev.start);
      if (!grouped.has(k)) grouped.set(k, []);
      grouped.get(k).push(ev);
    });
    const sortedKeys = Array.from(grouped.keys()).sort();
    const weekList = el("div", { class: "today-list" });
    sortedKeys.forEach((k) => {
      weekList.appendChild(
        el("div", { class: "today-week-day-label" }, [fmtEventDayHeader(k)])
      );
      grouped.get(k).forEach((ev) => weekList.appendChild(renderEventRow(ev)));
    });
    details.appendChild(weekList);
  }
  root.appendChild(details);

  return root;
}

// ── Section: pipeline (v0.33) ─────────────────────────────────────────
//
// Active leads with a "Promote Lead" action button per row. Hidden
// entirely when the leads slice is null (still loading) or empty.

function renderPipeline(state) {
  const pipeline = state.pipeline || {};
  const leads = pipeline.leads;
  // Hide while loading. Empty list collapses to a single "all caught
  // up" line. Errors surface inline so the dashboard still loads.
  if (leads == null) return null;
  const root = el("section", { class: "today-section", "data-section": "pipeline" });
  root.appendChild(el("div", { class: "section-label" }, ["🌱 PIPELINE"]));

  if (pipeline.error && leads.length === 0) {
    root.appendChild(
      el("div", { class: "empty" }, ["Leads unavailable — try refresh."])
    );
    return root;
  }
  if (leads.length === 0) {
    root.appendChild(el("div", { class: "empty" }, ["No active leads."]));
    return root;
  }

  const list = el("div", { class: "today-list" });
  leads.forEach((lead) => list.appendChild(renderPipelineRow(lead)));
  root.appendChild(list);
  return root;
}

function renderPipelineRow(lead) {
  const row = el("div", { class: "today-row today-row-lead" });
  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [
      `${lead.code || "(no code)"} — ${lead.name || "(untitled)"}`,
    ]),
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
  const btn = el("button", { class: "button button-secondary", type: "button" }, [
    "Promote Lead",
  ]);
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    dispatch("today:promote-lead", { lead });
  });
  right.appendChild(btn);
  row.appendChild(left);
  row.appendChild(right);
  return row;
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

// ── Quick skills strip (v0.34) ────────────────────────────────────────
//
// Three quick-skill cards directly under the Today sections: Strategic
// Thinking, Log time, Schedule social post. Pulled from the central
// skills registry by the today_quick context. Click dispatches
// "skill:dispatch" — main.js handles the rest.

function renderQuickSkills() {
  const container = document.getElementById("today-quick-skills");
  if (!container) return;
  container.innerHTML = "";
  const skills = skillsForContext("today_quick");
  skills.forEach((s) => {
    const card = el("button", {
      class: "workflow-card",
      type: "button",
      "data-skill-id": s.id,
    }, [
      el("div", { class: "workflow-card-title" }, [s.label]),
      el("div", { class: "workflow-card-meta" }, [s.description || ""]),
    ]);
    card.addEventListener("click", () =>
      dispatch("skill:dispatch", { skill_id: s.id })
    );
    container.appendChild(card);
  });
}

// ── Top-level draw ────────────────────────────────────────────────────

// v0.40 — per-section render registry. Each section's render fn is
// referenced by name so the live poller can swap a single section in
// place after a tick instead of redrawing the entire pane.
const SECTION_RENDERERS = [
  { name: "due-today", render: renderDueToday },
  { name: "overdue", render: renderOverdue },
  { name: "drift", render: renderDrift },
  { name: "calendar", render: renderCalendar },
  { name: "workstreams", render: renderWorkstreams },
  { name: "decisions", render: renderDecisions },
  { name: "pipeline", render: renderPipeline },
  { name: "email", render: renderEmail },
  { name: "slack", render: renderSlack },
  { name: "live-status", render: renderLiveStatus },
  { name: "receipts-pending", render: renderReceiptsPending },
  { name: "morning-briefing", render: renderMorningBriefing },
];

export function draw(state) {
  const container = document.getElementById("today-sections");
  if (!container) return;
  container.innerHTML = "";

  SECTION_RENDERERS.forEach(({ render }) => {
    const node = render(state);
    if (node) container.appendChild(node);
  });

  // Quick skills strip lives in its own static container (#today-quick-
  // skills) below the section grid. Render it once here so it stays in
  // sync with the registry.
  renderQuickSkills();
}

// v0.40 — single-section redraw. Returns the fresh DOM node (or null
// if the section is hidden in the current state). Used by the live
// poller after each tick.
export function drawSection(state, name) {
  const container = document.getElementById("today-sections");
  if (!container) return null;
  const renderer = SECTION_RENDERERS.find((r) => r.name === name);
  if (!renderer) return null;
  const fresh = renderer.render(state);
  const existing = container.querySelector(`[data-section="${name}"]`);
  if (fresh && existing) {
    existing.replaceWith(fresh);
    return fresh;
  }
  if (fresh && !existing) {
    container.appendChild(fresh);
    return fresh;
  }
  if (!fresh && existing) {
    existing.remove();
    return null;
  }
  return null;
}

// Re-render a single live-status row (cheap, called every 60s by main.js).
export function drawLiveStatusOnly(state) {
  return drawSection(state, "live-status");
}

// Look up a section's header element. Used by the poller to paint
// freshness badges and the pulse animation.
export function sectionHeader(name) {
  const container = document.getElementById("today-sections");
  if (!container) return null;
  const sectionEl = container.querySelector(`[data-section="${name}"]`);
  return sectionEl?.querySelector(".section-label") || null;
}
