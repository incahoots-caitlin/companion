// Studio CFO — render (v0.37 Block F).
//
// Renders the financial intelligence dashboard. Pure DOM build, reads
// from state. Click handlers dispatch CustomEvents; main.js wires the
// month-shift handler to re-fetch and re-render.

const MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v == null) continue;
    if (k === "class") node.className = v;
    else if (k === "style") node.setAttribute("style", v);
    else if (k in node) node[k] = v;
    else node.setAttribute(k, v);
  }
  for (const c of children) {
    if (c == null || c === false) continue;
    if (typeof c === "string") node.appendChild(document.createTextNode(c));
    else node.appendChild(c);
  }
  return node;
}

function dispatch(name, detail) {
  document.dispatchEvent(new CustomEvent(name, { detail }));
}

function fmtMoney(n) {
  if (n == null || Number.isNaN(n)) return "$0";
  const rounded = Math.round(n);
  return "$" + rounded.toLocaleString("en-AU");
}

function fmtHours(n) {
  if (n == null || Number.isNaN(n)) return "0h";
  // Drop trailing .0 for clean display.
  const r = Math.round(n * 10) / 10;
  return Number.isInteger(r) ? `${r}h` : `${r.toFixed(1)}h`;
}

function fmtPct(n) {
  if (n == null || Number.isNaN(n)) return "—";
  return `${Math.round(n * 100)}%`;
}

function burnClass(pct) {
  if (pct == null) return "";
  if (pct >= 1) return "cfo-burn-bad";
  if (pct >= 0.8) return "cfo-burn-warn";
  return "cfo-burn-ok";
}

// ── Sections ──────────────────────────────────────────────────────────

function renderHeader(state) {
  const header = el("header", { class: "main-header" });
  header.appendChild(
    el("div", { class: "main-title" }, ["Studio CFO"])
  );

  const monthRow = el("div", { class: "header-actions cfo-month-picker" });

  const prevBtn = el(
    "button",
    {
      class: "button-icon",
      type: "button",
      title: "Previous month",
      "aria-label": "Previous month",
    },
    ["◀"]
  );
  prevBtn.addEventListener("click", () =>
    dispatch("cfo:shift-month", { delta: -1 })
  );

  const label = el("div", { class: "cfo-month-label" }, [
    `${MONTHS[state.month - 1]} ${state.year}`,
  ]);

  const nextBtn = el(
    "button",
    {
      class: "button-icon",
      type: "button",
      title: "Next month",
      "aria-label": "Next month",
    },
    ["▶"]
  );
  nextBtn.addEventListener("click", () =>
    dispatch("cfo:shift-month", { delta: 1 })
  );

  monthRow.appendChild(prevBtn);
  monthRow.appendChild(label);
  monthRow.appendChild(nextBtn);
  header.appendChild(monthRow);

  return header;
}

function renderStudioTotals(state) {
  const t = state.totals;
  const section = el("section", { class: "today-section" });
  section.appendChild(
    el("div", { class: "section-label" }, ["📊 STUDIO TOTALS"])
  );

  if (state.loading && !t) {
    section.appendChild(el("div", { class: "empty" }, ["Loading totals..."]));
    return section;
  }
  if (!t) {
    section.appendChild(
      el("div", { class: "empty" }, ["No data this month."])
    );
    return section;
  }

  const grid = el("div", { class: "live-status-grid" });

  const rows = [
    ["Hours", `${fmtHours(t.hours_total)} (${fmtHours(t.hours_billable)} billable, ${fmtHours(t.hours_internal)} internal)`],
    ["Revenue", fmtMoney(t.revenue)],
    ["Subcontractor cost", `${fmtMoney(t.subcontractor_cost)} (Rose, ${fmtHours(t.hours_rose)} × $66)`],
    ["Caitlin hours", `${fmtHours(t.hours_caitlin)} × $110 = ${fmtMoney(t.hours_caitlin * 110)}`],
    ["Margin", `${fmtMoney(t.margin)} (${fmtMoney(t.avg_margin_per_hour)}/h average)`],
  ];
  for (const [label, value] of rows) {
    grid.appendChild(
      el("div", { class: "live-status-row" }, [
        el("span", { class: "live-status-label" }, [label]),
        el("span", { class: "live-status-value" }, [value]),
      ])
    );
  }
  section.appendChild(grid);
  return section;
}

function renderPerClient(state) {
  const list = state.per_client || [];
  const section = el("section", { class: "today-section" });
  section.appendChild(
    el("div", { class: "section-label" }, ["💼 PER CLIENT"])
  );

  if (state.loading && list.length === 0) {
    section.appendChild(
      el("div", { class: "empty" }, ["Loading client breakdown..."])
    );
    return section;
  }
  if (list.length === 0) {
    section.appendChild(
      el("div", { class: "empty" }, ["No client time logged this month."])
    );
    return section;
  }

  const table = el("div", { class: "cfo-client-table" });
  for (const c of list) {
    const codeCell = el("span", { class: "cfo-client-code" }, [
      c.client_code +
        (c.internal && c.client_name ? ` (${c.client_name})` : ""),
    ]);
    const hoursCell = el("span", { class: "cfo-client-hours" }, [
      fmtHours(c.hours),
    ]);
    const revenueCell = el("span", { class: "cfo-client-revenue" }, [
      c.internal ? `${fmtMoney(0)} (unbilled)` : fmtMoney(c.revenue),
    ]);
    let badge;
    if (c.internal) {
      badge = el("span", { class: "cfo-client-budget muted" }, ["—"]);
    } else if (c.no_budget || c.budget_total == null) {
      badge = el("span", { class: "cfo-client-budget cfo-burn-warn" }, [
        "⚠ No budget set",
      ]);
    } else {
      badge = el(
        "span",
        { class: `cfo-client-budget ${burnClass(c.budget_burn_pct)}` },
        [
          `◐ Budget: ${fmtMoney(c.budget_total)} (${fmtPct(
            c.budget_burn_pct
          )})`,
        ]
      );
    }
    const row = el("div", { class: "cfo-client-row" }, [
      codeCell,
      hoursCell,
      revenueCell,
      badge,
    ]);
    table.appendChild(row);
  }
  section.appendChild(table);
  return section;
}

function renderHourCreep(state) {
  const list = state.alerts || [];
  const section = el("section", { class: "today-section" });
  section.appendChild(
    el("div", { class: "section-label" }, ["⚠ HOUR CREEP"])
  );

  if (state.loading && list.length === 0) {
    section.appendChild(
      el("div", { class: "empty" }, ["Loading hour-creep alerts..."])
    );
    return section;
  }
  if (list.length === 0) {
    section.appendChild(
      el("div", { class: "empty" }, [
        "All active projects under 80% of budgeted hours.",
      ])
    );
    return section;
  }

  const table = el("div", { class: "cfo-creep-table" });
  for (const a of list) {
    const left = el("span", { class: "cfo-creep-project" }, [
      a.project_code +
        (a.project_name ? ` — ${a.project_name}` : ""),
    ]);
    const right = el(
      "span",
      { class: `cfo-creep-pct ${burnClass(a.burn_pct)}` },
      [
        `${fmtPct(a.burn_pct)} used (${fmtHours(a.hours_logged)} of ${fmtHours(
          a.hours_budgeted
        )})${a.end_date ? ` · ends ${a.end_date}` : ""}`,
      ]
    );
    table.appendChild(el("div", { class: "cfo-creep-row" }, [left, right]));
  }
  section.appendChild(table);
  return section;
}

function renderOutlook(state) {
  const o = state.outlook;
  const section = el("section", { class: "today-section" });
  section.appendChild(
    el("div", { class: "section-label" }, ["📅 NEXT MONTH OUTLOOK"])
  );

  if (state.loading && !o) {
    section.appendChild(
      el("div", { class: "empty" }, ["Loading next-month outlook..."])
    );
    return section;
  }
  if (!o) {
    section.appendChild(
      el("div", { class: "empty" }, ["No outlook data."])
    );
    return section;
  }

  const grid = el("div", { class: "live-status-grid" });
  grid.appendChild(
    el("div", { class: "live-status-row" }, [
      el("span", { class: "live-status-label" }, [
        `${MONTHS[o.month - 1]} ${o.year}`,
      ]),
      el("span", { class: "live-status-value" }, [
        `${o.active_projects} active project${o.active_projects === 1 ? "" : "s"}`,
      ]),
    ])
  );
  grid.appendChild(
    el("div", { class: "live-status-row" }, [
      el("span", { class: "live-status-label" }, ["Budgeted"]),
      el("span", { class: "live-status-value" }, [fmtMoney(o.budgeted_total)]),
    ])
  );
  grid.appendChild(
    el("div", { class: "live-status-row" }, [
      el("span", { class: "live-status-label" }, ["Capacity at 75%"]),
      el("span", { class: "live-status-value" }, [
        `~${fmtHours(o.capacity_caitlin_hours)} Caitlin, ~${fmtHours(
          o.capacity_rose_hours
        )} Rose`,
      ]),
    ])
  );
  section.appendChild(grid);
  return section;
}

// ── Public API ────────────────────────────────────────────────────────

export function draw(container, state) {
  if (!container) return;
  container.innerHTML = "";
  container.appendChild(renderHeader(state));

  if (state.error) {
    const banner = el("section", { class: "today-section" }, [
      el("div", { class: "today-section-meta" }, [state.error]),
    ]);
    container.appendChild(banner);
  }

  container.appendChild(renderStudioTotals(state));
  container.appendChild(renderPerClient(state));
  container.appendChild(renderHourCreep(state));
  container.appendChild(renderOutlook(state));
}
