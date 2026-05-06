// Per-Subcontractor view render (v0.38).
//
// Rose's home in Companion. Single section that paints into the
// supplied container element. Reads from state.js and the Skills
// registry — no Tauri calls here.

import { skillsForContext } from "../skills/registry.js";

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v == null) continue;
    if (k === "class") node.className = v;
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

function fmtDate(s) {
  if (!s) return "";
  // YYYY-MM-DD as-is if already in that shape; otherwise leave raw.
  return /^\d{4}-\d{2}-\d{2}/.test(s) ? s.slice(0, 10) : s;
}

function formatRate(r) {
  if (r == null) return "";
  return `$${Number(r).toFixed(0)}/h`;
}

// Sum hours from timelogs (already filtered to this person + month).
// Also bucket by client_code so we can show a per-client breakdown.
function summariseHours(timelogs) {
  let total = 0;
  const by_client = new Map();
  for (const r of timelogs) {
    const f = r.fields || {};
    const h = Number(f.hours || 0);
    if (Number.isFinite(h)) total += h;
    // client_code is a lookup field — Airtable returns lookups as arrays.
    let code = f.client_code;
    if (Array.isArray(code)) code = code[0] || "";
    code = String(code || "").toUpperCase().trim();
    if (!code) code = "—";
    by_client.set(code, (by_client.get(code) || 0) + (Number.isFinite(h) ? h : 0));
  }
  const rows = Array.from(by_client.entries())
    .map(([code, hours]) => ({ code, hours }))
    .sort((a, b) => b.hours - a.hours);
  return { total, rows };
}

// Filter the recent receipts down to ones written by this Subcontractor.
// We match on the receipt's JSON payload: any of `created_by`,
// `subcontractor` or `subcontractor_code` set to the supplied code (case-
// insensitive). Receipts that carry no signal at all fall through to the
// "uncategorised" bucket so legacy entries don't disappear.
function partitionReceiptsByCreator(receipts, code) {
  const upper = String(code || "").toUpperCase();
  const mine = [];
  const uncategorised = [];
  for (const r of receipts) {
    const f = r.fields || {};
    let parsed = null;
    try {
      parsed = f.json ? JSON.parse(f.json) : null;
    } catch {
      parsed = null;
    }
    const candidates = [
      parsed?.created_by,
      parsed?.subcontractor,
      parsed?.subcontractor_code,
    ]
      .filter(Boolean)
      .map((v) => String(v).toUpperCase());
    if (candidates.length === 0) {
      uncategorised.push(r);
    } else if (candidates.includes(upper)) {
      mine.push(r);
    }
    // Receipts tagged for someone else are dropped silently.
  }
  return { mine: mine.slice(0, 10), uncategorised: uncategorised.slice(0, 5) };
}

function renderHeader(view, state) {
  const h = state.header || {};
  const header = el("header", { class: "main-header" });
  const title = el("div", { class: "main-title" }, [
    h.name || state.code || "Subcontractor",
  ]);
  header.appendChild(title);
  view.appendChild(header);

  const meta = el("section", { class: "today-section" });
  const codeLine = [h.code || state.code, h.role].filter(Boolean).join(" · ");
  const startLine = h.start_date
    ? `Started ${fmtDate(h.start_date)}`
    : "";
  const rateLine = formatRate(h.hourly_rate);
  const summaryLine = [codeLine, startLine, rateLine]
    .filter(Boolean)
    .join(" · ");
  if (summaryLine) {
    meta.appendChild(
      el("div", { class: "today-section-meta" }, [summaryLine])
    );
  }
  if (h.email) {
    meta.appendChild(
      el("div", { class: "today-section-meta" }, [`Email: ${h.email}`])
    );
  }
  if (h.notes) {
    meta.appendChild(
      el("div", { class: "today-section-meta" }, [h.notes])
    );
  }
  view.appendChild(meta);
}

function renderWorkstreams(view, state) {
  const section = el("section", { class: "today-section" });
  section.appendChild(
    el("div", { class: "section-label" }, ["📌 ASSIGNED WORKSTREAMS"])
  );
  if (!state.workstreams.length) {
    section.appendChild(
      el("div", { class: "empty" }, [
        "Nothing assigned yet. Run Subcontractor Onboarding to scope week 1.",
      ])
    );
  } else {
    const list = el("div", { class: "today-list" });
    state.workstreams.forEach((r) => {
      const f = r.fields || {};
      const code = f.code || "";
      const title = f.title || code || "Workstream";
      const phase = f.phase || "";
      const next = f.next_action || "";
      const parts = [code && `[${code}]`, title, phase && `· ${phase}`]
        .filter(Boolean)
        .join(" ");
      const row = el("div", { class: "today-row" }, [
        el("div", {}, [parts]),
        next
          ? el("div", { class: "today-section-meta" }, [`next: ${next}`])
          : null,
      ]);
      list.appendChild(row);
    });
    section.appendChild(list);
  }
  view.appendChild(section);
}

function renderCommitments(view, state) {
  const section = el("section", { class: "today-section" });
  section.appendChild(
    el("div", { class: "section-label" }, ["✅ OPEN COMMITMENTS"])
  );
  if (!state.commitments.length) {
    section.appendChild(
      el("div", { class: "empty" }, ["No open commitments."])
    );
  } else {
    const list = el("div", { class: "today-list" });
    state.commitments.forEach((r) => {
      const f = r.fields || {};
      const title = f.title || "Commitment";
      const due = f.due_at ? `due ${fmtDate(f.due_at)}` : "";
      const priority = f.priority ? `· ${f.priority}` : "";
      const line = [title, due && `(${due})`, priority]
        .filter(Boolean)
        .join(" ");
      list.appendChild(el("div", { class: "today-row" }, [line]));
    });
    section.appendChild(list);
  }
  view.appendChild(section);
}

function renderHours(view, state) {
  const section = el("section", { class: "today-section" });
  section.appendChild(
    el("div", { class: "section-label" }, ["⏱ HOURS THIS MONTH"])
  );
  const { total, rows } = summariseHours(state.timelogs);
  const rate = Number(state.header?.hourly_rate || 0);
  const takeHome = Number.isFinite(total * rate) ? total * rate : 0;

  const totalLine = `Total: ${total.toFixed(1)}h`;
  section.appendChild(
    el("div", { class: "today-section-meta" }, [totalLine])
  );
  if (rows.length) {
    const breakdown = rows
      .map((r) => `${r.code} ${r.hours.toFixed(1)}h`)
      .join(", ");
    section.appendChild(
      el("div", { class: "today-section-meta" }, [`By client: ${breakdown}`])
    );
  }
  if (rate > 0) {
    section.appendChild(
      el("div", { class: "today-section-meta" }, [
        `Take-home pay (at ${formatRate(rate)}): $${takeHome.toFixed(0)}`,
      ])
    );
  }
  view.appendChild(section);
}

function renderReceipts(view, state) {
  const section = el("section", { class: "today-section" });
  section.appendChild(
    el("div", { class: "section-label" }, [
      "📃 RECENT RECEIPTS BY THIS PERSON",
    ])
  );
  const { mine, uncategorised } = partitionReceiptsByCreator(
    state.receipts,
    state.code
  );
  if (mine.length === 0 && uncategorised.length === 0) {
    section.appendChild(
      el("div", { class: "empty" }, ["No receipts yet."])
    );
  } else {
    if (mine.length > 0) {
      const list = el("div", { class: "today-list" });
      mine.forEach((r) => {
        const f = r.fields || {};
        const title = f.title || "Receipt";
        const date = fmtDate(f.date || "");
        const wf = f.workflow || "";
        const line = [date && `${date}`, title, wf && `· ${wf}`]
          .filter(Boolean)
          .join(" ");
        list.appendChild(el("div", { class: "today-row" }, [line]));
      });
      section.appendChild(list);
    }
    if (uncategorised.length > 0) {
      section.appendChild(
        el("div", { class: "today-section-meta" }, [
          `Plus ${uncategorised.length} uncategorised (legacy receipts with no created_by).`,
        ])
      );
    }
  }
  view.appendChild(section);
}

function renderSkills(view, state) {
  const section = el("section", { class: "today-section" });
  section.appendChild(el("div", { class: "section-label" }, ["🛠 SKILLS"]));
  const skills = skillsForContext("subcontractor");
  if (skills.length === 0) {
    section.appendChild(
      el("div", { class: "empty" }, ["No skills available here yet."])
    );
  } else {
    const grid = el("div", { class: "client-shortcut-grid" });
    skills.forEach((s) => {
      const card = el(
        "button",
        {
          type: "button",
          class:
            "client-shortcut" +
            (s.placeholder ? " client-shortcut-placeholder" : ""),
          "data-skill-id": s.id,
        },
        [
          el("div", { class: "client-shortcut-title" }, [s.label]),
          el("div", { class: "client-shortcut-meta" }, [s.description || ""]),
        ]
      );
      card.addEventListener("click", () => {
        document.dispatchEvent(
          new CustomEvent("skill:dispatch", {
            detail: {
              skill_id: s.id,
              subcontractor_code: state.code,
            },
          })
        );
      });
      grid.appendChild(card);
    });
    section.appendChild(grid);
  }
  view.appendChild(section);
}

export function drawLoading(view, code) {
  if (!view) return;
  view.innerHTML = "";
  const header = el("header", { class: "main-header" });
  header.appendChild(
    el("div", { class: "main-title" }, [code || "Subcontractor"])
  );
  view.appendChild(header);
  const section = el("section", { class: "today-section" });
  section.appendChild(
    el("div", { class: "today-section-meta" }, ["Loading..."])
  );
  view.appendChild(section);
}

export function draw(view, state) {
  if (!view) return;
  view.innerHTML = "";
  if (state.error) {
    const header = el("header", { class: "main-header" });
    header.appendChild(
      el("div", { class: "main-title" }, [state.code || "Subcontractor"])
    );
    view.appendChild(header);
    const section = el("section", { class: "today-section" });
    section.appendChild(
      el("div", { class: "empty" }, [
        `Couldn't load this view (${state.error}).`,
      ])
    );
    view.appendChild(section);
    return;
  }
  renderHeader(view, state);
  renderWorkstreams(view, state);
  renderCommitments(view, state);
  renderHours(view, state);
  renderReceipts(view, state);
  renderSkills(view, state);
}
