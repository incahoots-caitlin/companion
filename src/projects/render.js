// Per-project view — render (v0.28 Block E).
//
// Replaces #client-view with a project header, a new-note composer, and
// a chronological feed of updates pulled from every connected source.
// Pure render: read state, mount markup, dispatch CustomEvents for
// clicks. main.js wires the handlers.

import { NOTE_TAGS } from "./state.js";

const SOURCE_ICON = {
  note: "🗒",
  receipt: "🧾",
  conversation: "💬",
  calendar: "📅",
  slack: "📨",
  gmail: "📧",
  drive: "📁",
};

const SOURCE_LABEL = {
  note: "Note",
  receipt: "Receipt",
  conversation: "Chat",
  calendar: "Calendar",
  slack: "Slack",
  gmail: "Email",
  drive: "Drive",
};

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

function dispatch(name, detail) {
  document.dispatchEvent(new CustomEvent(name, { detail }));
}

function fmtDate(s) {
  if (!s) return "";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
  return d.toLocaleDateString("en-AU", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

function fmtDateTime(s) {
  if (!s) return "";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
  const date = d.toLocaleDateString("en-AU", {
    day: "numeric",
    month: "short",
  });
  const time = d
    .toLocaleTimeString("en-AU", {
      hour: "numeric",
      minute: "2-digit",
      hour12: true,
    })
    .replace(/\s/g, "")
    .toLowerCase();
  return `${date} ${time}`;
}

function fmtBudget(v) {
  if (v == null) return "";
  const n = Number(v);
  if (Number.isNaN(n)) return "";
  return n.toLocaleString("en-AU", {
    style: "currency",
    currency: "AUD",
    maximumFractionDigits: 0,
  });
}

// ── Header ────────────────────────────────────────────────────────────

function renderHeader(state) {
  const h = state.header || { code: state.project_code };
  const wrap = el("div", { class: "project-header" });

  const titleRow = el("div", { class: "project-header-row" }, [
    el("button", {
      class: "project-back-btn",
      type: "button",
      title: "Back to client view",
      "aria-label": "Back",
    }, ["← Back"]),
    el("div", { class: "project-header-title" }, [h.name || h.code || ""]),
  ]);
  titleRow
    .querySelector(".project-back-btn")
    .addEventListener("click", () => dispatch("project:back-click", {}));
  wrap.appendChild(titleRow);

  const codeRow = el("div", { class: "project-header-meta" });
  if (h.code) {
    codeRow.appendChild(el("span", { class: "project-header-code" }, [h.code]));
  }
  if (h.status) {
    codeRow.appendChild(el("span", { class: "project-header-pill" }, [h.status]));
  }
  if (h.campaign_type) {
    codeRow.appendChild(el("span", {}, [h.campaign_type]));
  }
  const budget = fmtBudget(h.budget_total);
  if (budget) {
    codeRow.appendChild(el("span", {}, [`Budget ${budget}`]));
  }
  if (h.start_date || h.end_date) {
    codeRow.appendChild(
      el("span", {}, [`${fmtDate(h.start_date)} → ${fmtDate(h.end_date) || "—"}`])
    );
  }
  if (h.client_code || h.client_name) {
    const label = h.client_name
      ? `Client: ${h.client_name}${h.client_code ? ` (${h.client_code})` : ""}`
      : `Client: ${h.client_code}`;
    const link = el("a", {
      class: "project-header-client",
      href: "#",
      "data-client-code": h.client_code || "",
    }, [label]);
    link.addEventListener("click", (e) => {
      e.preventDefault();
      if (h.client_code) {
        dispatch("project:client-link-click", { client_code: h.client_code });
      }
    });
    codeRow.appendChild(link);
  }
  wrap.appendChild(codeRow);

  return wrap;
}

// ── Composer ──────────────────────────────────────────────────────────

function renderComposer(state) {
  const composer = el("section", {
    class: "project-section project-note-composer",
    "data-section": "composer",
  });
  composer.appendChild(el("div", { class: "section-label" }, ["✏️ NEW NOTE"]));

  const ta = el("textarea", {
    class: "project-note-input",
    placeholder: "Drop a quick note for this project…",
    rows: 3,
  });
  ta.value = state.composer.body || "";
  ta.disabled = !!state.composer.saving;
  ta.addEventListener("input", () => {
    state.composer.body = ta.value;
  });
  composer.appendChild(ta);

  const tagRow = el("div", { class: "project-note-tags" });
  NOTE_TAGS.forEach((tag) => {
    const isOn = state.composer.tags.includes(tag);
    const btn = el(
      "button",
      {
        class: "project-tag-btn" + (isOn ? " is-active" : ""),
        type: "button",
        "data-tag": tag,
      },
      [tag]
    );
    btn.disabled = !!state.composer.saving;
    btn.addEventListener("click", () => {
      if (isOn) {
        state.composer.tags = state.composer.tags.filter((t) => t !== tag);
      } else {
        state.composer.tags = [...state.composer.tags, tag];
      }
      // Toggle CSS in place — cheaper than a full rerender for a hot click.
      btn.classList.toggle("is-active");
    });
    tagRow.appendChild(btn);
  });
  composer.appendChild(tagRow);

  const actionRow = el("div", { class: "project-note-actions" });
  const saveBtn = el("button", {
    class: "button",
    type: "button",
  }, [state.composer.saving ? "Saving…" : "Save note"]);
  saveBtn.disabled = !!state.composer.saving;
  saveBtn.addEventListener("click", () => {
    const body = (state.composer.body || "").trim();
    if (!body) return;
    dispatch("project:note-save", {
      body,
      tags: [...state.composer.tags],
    });
  });
  actionRow.appendChild(saveBtn);
  if (state.composer.error) {
    actionRow.appendChild(
      el("span", { class: "project-note-error" }, [state.composer.error])
    );
  }
  composer.appendChild(actionRow);

  return composer;
}

// ── Forms (v0.30) ─────────────────────────────────────────────────────
//
// Three send-form actions per project: Discovery Pre-Brief and Post-
// Campaign Feedback (both prefilled with the Project record id), and
// Content Approval (prefilled with a SocialPost record id the user
// pastes in). v0.30 ships the buttons; v0.32 polishes the email
// drafts and the SocialPost picker.

function renderProjectForms(state) {
  const root = el("section", {
    class: "project-section project-forms",
    "data-section": "forms",
  });
  root.appendChild(el("div", { class: "section-label" }, ["✉️ SEND A FORM"]));

  const status = String(state.header?.status || "").toLowerCase();
  const wrapped = ["wrap", "done", "wrapped", "complete", "completed"].includes(status);

  const buttons = el("div", { class: "project-forms-buttons" });

  const preBriefBtn = el("button", {
    class: "button button-secondary",
    type: "button",
  }, ["Send Discovery Pre-Brief"]);
  preBriefBtn.addEventListener("click", () =>
    dispatch("project:form-send", { form_key: "form_discovery_pre_brief" })
  );
  buttons.appendChild(preBriefBtn);

  const approvalBtn = el("button", {
    class: "button button-secondary",
    type: "button",
  }, ["Send Content Approval"]);
  approvalBtn.addEventListener("click", () =>
    dispatch("project:form-send", { form_key: "form_content_approval" })
  );
  buttons.appendChild(approvalBtn);

  if (wrapped) {
    const wrapBtn = el("button", {
      class: "button button-secondary",
      type: "button",
    }, ["Send Post-Campaign Feedback"]);
    wrapBtn.addEventListener("click", () =>
      dispatch("project:form-send", { form_key: "form_post_campaign_feedback" })
    );
    buttons.appendChild(wrapBtn);

    // v0.31: when a project is at wrap or done, offer the wrap report
    // skill. Routed through main.js via project:wrap-report-click so the
    // modal stays in the central catalogue.
    const wrapReportBtn = el("button", {
      class: "button",
      type: "button",
    }, ["Draft Wrap Report"]);
    wrapReportBtn.addEventListener("click", () =>
      dispatch("project:wrap-report-click", {
        project_code: state.header?.code || "",
      })
    );
    buttons.appendChild(wrapReportBtn);
  }

  root.appendChild(buttons);
  root.appendChild(
    el("div", { class: "project-forms-meta" }, [
      "Discovery and Post-Campaign drafts attach this project's record id. Content Approval asks for the SocialPost record id when you click.",
    ])
  );
  return root;
}

// ── Updates feed ──────────────────────────────────────────────────────

function renderTagPills(tags) {
  if (!tags || tags.length === 0) return null;
  const wrap = el("div", { class: "project-update-tags" });
  tags.forEach((t) =>
    wrap.appendChild(el("span", { class: "project-tag-pill" }, [t]))
  );
  return wrap;
}

function renderNoteRow(u) {
  const row = el("div", {
    class: "project-update project-update-note",
    "data-update-id": u.id || "",
    "data-record-id": u.record_id || "",
  });
  row.appendChild(
    el("div", { class: "project-update-header" }, [
      el("span", { class: "project-update-icon" }, [SOURCE_ICON.note]),
      el("span", { class: "project-update-source" }, [SOURCE_LABEL.note]),
      u.created_by
        ? el("span", { class: "project-update-author" }, [`from ${u.created_by}`])
        : null,
      el("span", { class: "project-update-ts" }, [fmtDateTime(u.ts)]),
    ])
  );
  row.appendChild(el("div", { class: "project-update-body" }, [u.body || ""]));
  const tagPills = renderTagPills(u.tags);
  if (tagPills) row.appendChild(tagPills);
  return row;
}

function renderReceiptRow(u) {
  const row = el("div", { class: "project-update project-update-receipt" });
  row.appendChild(
    el("div", { class: "project-update-header" }, [
      el("span", { class: "project-update-icon" }, [SOURCE_ICON.receipt]),
      el("span", { class: "project-update-source" }, [SOURCE_LABEL.receipt]),
      u.workflow
        ? el("span", { class: "project-update-author" }, [u.workflow])
        : null,
      el("span", { class: "project-update-ts" }, [fmtDate(u.ts)]),
    ])
  );
  row.appendChild(el("div", { class: "project-update-body" }, [u.title || ""]));
  if (u.ticked_count != null) {
    row.appendChild(
      el("div", { class: "project-update-meta" }, [
        `${u.ticked_count} item${u.ticked_count === 1 ? "" : "s"} ticked`,
      ])
    );
  }
  return row;
}

function renderConversationRow(u) {
  const row = el("div", { class: "project-update project-update-conversation" });
  row.appendChild(
    el("div", { class: "project-update-header" }, [
      el("span", { class: "project-update-icon" }, [SOURCE_ICON.conversation]),
      el("span", { class: "project-update-source" }, [SOURCE_LABEL.conversation]),
      u.workstream_code
        ? el("span", { class: "project-update-author" }, [u.workstream_code])
        : null,
      el("span", { class: "project-update-ts" }, [fmtDateTime(u.ts)]),
    ])
  );
  if (u.summary) {
    row.appendChild(el("div", { class: "project-update-body" }, [u.summary]));
  }
  if (u.message_count != null) {
    row.appendChild(
      el("div", { class: "project-update-meta" }, [
        `${u.message_count} message${u.message_count === 1 ? "" : "s"}`,
      ])
    );
  }
  return row;
}

function renderCalendarRow(u) {
  const row = el("div", { class: "project-update project-update-calendar" });
  const headerChildren = [
    el("span", { class: "project-update-icon" }, [SOURCE_ICON.calendar]),
    el("span", { class: "project-update-source" }, [SOURCE_LABEL.calendar]),
    el("span", { class: "project-update-ts" }, [
      u.all_day ? fmtDate(u.ts) + " (all day)" : fmtDateTime(u.ts),
    ]),
  ];
  row.appendChild(el("div", { class: "project-update-header" }, headerChildren));
  if (u.html_link) {
    const link = el("a", {
      class: "project-update-body project-update-link",
      href: u.html_link,
    }, [u.summary || ""]);
    link.addEventListener("click", (e) => {
      e.preventDefault();
      dispatch("project:open-url", { url: u.html_link });
    });
    row.appendChild(link);
  } else {
    row.appendChild(el("div", { class: "project-update-body" }, [u.summary || ""]));
  }
  return row;
}

function renderSlackRow(u) {
  const row = el("div", { class: "project-update project-update-slack" });
  row.appendChild(
    el("div", { class: "project-update-header" }, [
      el("span", { class: "project-update-icon" }, [SOURCE_ICON.slack]),
      el("span", { class: "project-update-source" }, [
        `#${u.channel_name || "slack"}`,
      ]),
      u.user_name
        ? el("span", { class: "project-update-author" }, [u.user_name])
        : null,
      el("span", { class: "project-update-ts" }, [fmtDateTime(u.ts)]),
    ])
  );
  if (u.permalink) {
    const link = el("a", {
      class: "project-update-body project-update-link",
      href: u.permalink,
    }, [u.text || ""]);
    link.addEventListener("click", (e) => {
      e.preventDefault();
      dispatch("project:open-url", { url: u.permalink });
    });
    row.appendChild(link);
  } else {
    row.appendChild(el("div", { class: "project-update-body" }, [u.text || ""]));
  }
  return row;
}

function renderGmailRow(u) {
  const row = el("div", { class: "project-update project-update-gmail" });
  row.appendChild(
    el("div", { class: "project-update-header" }, [
      el("span", { class: "project-update-icon" }, [SOURCE_ICON.gmail]),
      el("span", { class: "project-update-source" }, [SOURCE_LABEL.gmail]),
      u.from
        ? el("span", { class: "project-update-author" }, [u.from])
        : null,
      el("span", { class: "project-update-ts" }, [fmtDateTime(u.ts)]),
    ])
  );
  const link = el("a", {
    class: "project-update-body project-update-link",
    href: u.web_link || "#",
  }, [u.subject || "(no subject)"]);
  if (u.web_link) {
    link.addEventListener("click", (e) => {
      e.preventDefault();
      dispatch("project:open-url", { url: u.web_link });
    });
  }
  row.appendChild(link);
  if (u.snippet) {
    row.appendChild(
      el("div", { class: "project-update-meta" }, [u.snippet])
    );
  }
  return row;
}

function renderDriveRow(u) {
  const row = el("div", { class: "project-update project-update-drive" });
  row.appendChild(
    el("div", { class: "project-update-header" }, [
      el("span", { class: "project-update-icon" }, [SOURCE_ICON.drive]),
      el("span", { class: "project-update-source" }, [SOURCE_LABEL.drive]),
      u.modified_by
        ? el("span", { class: "project-update-author" }, [u.modified_by])
        : null,
      el("span", { class: "project-update-ts" }, [fmtDateTime(u.ts)]),
    ])
  );
  if (u.web_view_link) {
    const link = el("a", {
      class: "project-update-body project-update-link",
      href: u.web_view_link,
    }, [u.name || "(untitled)"]);
    link.addEventListener("click", (e) => {
      e.preventDefault();
      dispatch("project:open-url", { url: u.web_view_link });
    });
    row.appendChild(link);
  } else {
    row.appendChild(el("div", { class: "project-update-body" }, [u.name || ""]));
  }
  return row;
}

function renderUpdateRow(u) {
  switch (u.kind) {
    case "note":
      return renderNoteRow(u);
    case "receipt":
      return renderReceiptRow(u);
    case "conversation":
      return renderConversationRow(u);
    case "calendar":
      return renderCalendarRow(u);
    case "slack":
      return renderSlackRow(u);
    case "gmail":
      return renderGmailRow(u);
    case "drive":
      return renderDriveRow(u);
    default:
      return el("div", { class: "project-update" }, [
        JSON.stringify(u, null, 2),
      ]);
  }
}

function renderUpdates(state) {
  const wrap = el("section", {
    class: "project-section project-updates",
    "data-section": "updates",
  });
  wrap.appendChild(el("div", { class: "section-label" }, ["📜 UPDATES"]));

  if (state.updates == null) {
    wrap.appendChild(
      el("div", { class: "project-empty" }, ["Loading updates…"])
    );
    return wrap;
  }
  if (state.updates.length === 0) {
    wrap.appendChild(
      el("div", { class: "project-empty" }, [
        "No updates yet for this project. Drop a note above to start the trail.",
      ])
    );
    return wrap;
  }

  state.updates.forEach((u) => wrap.appendChild(renderUpdateRow(u)));
  return wrap;
}

// ── Top-level draw ────────────────────────────────────────────────────

export function draw(state) {
  const container = document.getElementById("client-view");
  if (!container) return;
  container.innerHTML = "";
  container.classList.add("client-view-project");

  const layout = el("div", { class: "project-layout" });
  layout.appendChild(renderHeader(state));
  layout.appendChild(renderProjectForms(state));
  layout.appendChild(renderComposer(state));
  layout.appendChild(renderUpdates(state));
  if (state.error) {
    layout.appendChild(
      el("div", { class: "project-error" }, [
        `Some updates failed to load: ${state.error}`,
      ])
    );
  }
  container.appendChild(layout);
}

export function drawLoading(projectCode) {
  const container = document.getElementById("client-view");
  if (!container) return;
  container.innerHTML = "";
  container.classList.add("client-view-project");
  container.appendChild(
    el("div", { class: "project-loading" }, [
      `Loading ${projectCode || "project"}…`,
    ])
  );
}

// Called when leaving the per-project view so the standard per-client
// view renders cleanly without leftover project classes.
export function exitProject() {
  const container = document.getElementById("client-view");
  if (container) container.classList.remove("client-view-project");
}
