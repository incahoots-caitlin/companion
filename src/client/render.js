// Per-client view — rendering.
//
// Pure-ish render: read _state.client, mount markup into #client-view.
// No fetch in here. Click handlers dispatch CustomEvents on document so
// main.js can wire modals without us reaching into the global namespace.

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
  const d = new Date(s.length === 10 ? `${s}T00:00:00` : s);
  if (Number.isNaN(d.getTime())) return s;
  return d.toLocaleDateString("en-AU", { weekday: "short", day: "numeric", month: "short" });
}

function fmtDateTime(s) {
  if (!s) return "";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
  const day = d.toLocaleDateString("en-AU", { weekday: "short", day: "numeric", month: "short" });
  const time = d
    .toLocaleTimeString("en-AU", { hour: "numeric", minute: "2-digit", hour12: true })
    .replace(/\s/g, "")
    .toLowerCase();
  return `${day}, ${time}`;
}

// ── Header ────────────────────────────────────────────────────────────

function renderHeader(state) {
  const h = state.header || {};
  const root = el("div", { class: "client-header" });
  root.appendChild(el("div", { class: "client-code" }, [state.code || ""]));

  const titleRow = el("div", { class: "client-title-row" }, [
    el("div", { class: "client-name" }, [h.name || state.code || ""]),
    el("button", {
      class: "button-icon",
      id: "client-refresh-btn",
      title: "Refresh client view",
      "aria-label": "Refresh",
    }, [el("span", { "aria-hidden": "true" }, ["↻"])]),
  ]);
  root.appendChild(titleRow);

  const meta = el("div", { class: "client-meta" });
  if (h.status) {
    meta.appendChild(
      el("span", { class: `client-status-pill status-${h.status}` }, [h.status])
    );
  }
  if (h.primary_contact_name || h.primary_contact_email) {
    const parts = [];
    if (h.primary_contact_name) parts.push(h.primary_contact_name);
    if (h.primary_contact_email) {
      meta.appendChild(
        el("a", { href: `mailto:${h.primary_contact_email}` }, [
          parts.join(" · ") || h.primary_contact_email,
        ])
      );
      if (h.primary_contact_email && parts.length) {
        meta.appendChild(el("span", { class: "client-meta-sep" }, ["·"]));
      }
    } else {
      meta.appendChild(el("span", {}, [parts.join(" · ")]));
    }
  }
  if (h.last_touch) {
    meta.appendChild(el("span", {}, [`Last touch: ${fmtDate(h.last_touch)}`]));
  }
  if (h.dropbox_folder) {
    meta.appendChild(
      el("a", {
        href: h.dropbox_folder,
        target: "_blank",
        rel: "noopener",
      }, ["Dropbox"])
    );
  }
  if (h.abn) {
    meta.appendChild(el("span", {}, [`ABN ${h.abn}`]));
  }
  root.appendChild(meta);
  return root;
}

// ── Forms toolbar (v0.30) ─────────────────────────────────────────────
//
// Two send-form buttons surfaced at the top of every per-client view:
// "Send Discovery Pre-Brief" (always available — assumes the user picks
// a project at click time) and "Send Post-Campaign Feedback" (only
// surfaced when the client has at least one project at status = wrap
// or done). Both dispatch CustomEvents the main.js listener handles by
// composing a mailto: with the prefilled form URL.

function projectsAtStatus(state, statuses) {
  const items = state.projects || [];
  return items.filter((p) =>
    statuses.includes(String(p.status || "").toLowerCase())
  );
}

function renderClientFormsToolbar(state) {
  // Discovery Pre-Brief: client has any non-archived project.
  const anyProject = (state.projects || []).length > 0;
  // Post-campaign feedback: only when a project is wrapped/done.
  const wrapped = projectsAtStatus(state, ["wrap", "done", "wrapped", "complete", "completed"]);

  if (!anyProject && wrapped.length === 0) return null;

  const root = el("section", {
    class: "client-section client-forms-toolbar",
    "data-section": "forms",
  });
  root.appendChild(el("div", { class: "section-label" }, ["✉️ SEND A FORM"]));

  const row = el("div", { class: "client-forms-buttons" });

  if (anyProject) {
    const btn = el("button", {
      class: "button button-secondary",
      type: "button",
      "data-form-action": "send-discovery-pre-brief",
    }, ["Send Discovery Pre-Brief"]);
    btn.addEventListener("click", () =>
      dispatch("client:form-send", {
        form_key: "form_discovery_pre_brief",
        client_code: state.code,
      })
    );
    row.appendChild(btn);
  }

  if (wrapped.length > 0) {
    const btn = el("button", {
      class: "button button-secondary",
      type: "button",
      "data-form-action": "send-post-campaign-feedback",
    }, ["Send Post-Campaign Feedback"]);
    btn.addEventListener("click", () =>
      dispatch("client:form-send", {
        form_key: "form_post_campaign_feedback",
        client_code: state.code,
      })
    );
    row.appendChild(btn);
  }

  root.appendChild(row);
  return root;
}

// ── Workstreams ───────────────────────────────────────────────────────

function renderWorkstreams(state) {
  const items = state.workstreams || [];
  if (items.length === 0) return null; // hide section
  const root = el("section", { class: "client-section", "data-section": "workstreams" });
  root.appendChild(el("div", { class: "section-label" }, ["📌 ACTIVE WORKSTREAMS"]));
  const list = el("div", { class: "today-list" });
  items.forEach((w) => list.appendChild(renderWorkstreamRow(w)));
  root.appendChild(list);
  return root;
}

function renderWorkstreamRow(w) {
  const row = el("button", {
    class: "today-row today-row-workstream",
    type: "button",
    "data-workstream-code": w.code || "",
  });
  row.addEventListener("click", () => dispatch("client:workstream-click", { workstream: w }));

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

  row.appendChild(left);
  row.appendChild(right);
  return row;
}

// ── Decisions ─────────────────────────────────────────────────────────

function renderDecisions(state) {
  const items = state.decisions || [];
  if (items.length === 0) return null; // hide section
  const root = el("section", { class: "client-section", "data-section": "decisions" });
  root.appendChild(el("div", { class: "section-label" }, ["🎯 OPEN DECISIONS"]));
  const list = el("div", { class: "today-list" });
  items.forEach((d) => list.appendChild(renderDecisionRow(d)));
  root.appendChild(list);
  return root;
}

function renderDecisionRow(d) {
  const row = el("button", {
    class: "today-row today-row-decision",
    type: "button",
    "data-decision-id": d.id || "",
  });
  row.addEventListener("click", () => dispatch("client:decision-click", { decision: d }));

  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [d.title || "(untitled)"]),
    d.due_date ? el("div", { class: "today-row-meta" }, [`Due ${fmtDate(d.due_date)}`]) : null,
  ]);
  const right = el("div", { class: "today-row-side" });
  if (d.decision_type) right.appendChild(el("span", { class: "pill" }, [d.decision_type]));

  row.appendChild(left);
  row.appendChild(right);
  return row;
}

// ── Commitments ───────────────────────────────────────────────────────

function renderCommitments(state) {
  const items = state.commitments || [];
  if (items.length === 0) return null; // hide section
  const root = el("section", { class: "client-section", "data-section": "commitments" });
  root.appendChild(el("div", { class: "section-label" }, ["✅ OPEN COMMITMENTS"]));
  const list = el("div", { class: "today-list" });
  items.forEach((c) => list.appendChild(renderCommitmentRow(c)));
  root.appendChild(list);
  return root;
}

function renderCommitmentRow(c) {
  const row = el("button", {
    class: "today-row today-row-commitment",
    type: "button",
    "data-commitment-id": c.id || "",
  });
  row.addEventListener("click", () => dispatch("client:commitment-click", { commitment: c }));

  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [c.title || "(untitled)"]),
    c.notes ? el("div", { class: "today-row-meta" }, [c.notes]) : null,
  ]);
  const right = el("div", { class: "today-row-side" });
  if (c.due_at) right.appendChild(el("span", { class: "today-row-time" }, [fmtDateTime(c.due_at)]));

  row.appendChild(left);
  row.appendChild(right);
  return row;
}

// ── Projects ──────────────────────────────────────────────────────────

function renderProjects(state) {
  const items = state.projects || [];
  const root = el("section", { class: "client-section", "data-section": "projects" });
  root.appendChild(el("div", { class: "section-label" }, ["📁 PROJECTS"]));
  if (items.length === 0) {
    root.appendChild(
      el("div", { class: "client-empty" }, [
        "No projects yet. Run New Campaign Scope to add one.",
      ])
    );
    return root;
  }
  const wrapper = el("div", {
    class: items.length > 5 ? "client-projects client-projects-scroll" : "client-projects",
  });
  items.forEach((p) => wrapper.appendChild(renderProjectRow(p)));
  root.appendChild(wrapper);
  return root;
}

function renderProjectRow(p) {
  const row = el("button", {
    class: "client-project client-project-clickable",
    type: "button",
    "data-project-code": p.code || "",
  });
  row.addEventListener("click", () => dispatch("client:project-click", { project: p }));

  const main = el("div", { class: "client-project-main" }, [
    el("div", { class: "client-project-code" }, [p.code || ""]),
    el("div", { class: "client-project-name" }, [p.name || ""]),
  ]);

  const metaParts = [];
  if (p.campaign_type) metaParts.push(p.campaign_type);
  if (p.budget_total) metaParts.push(`Budget ${p.budget_total}`);
  if (p.start_date || p.end_date) {
    metaParts.push(`${fmtDate(p.start_date)} → ${fmtDate(p.end_date) || "—"}`);
  }
  if (metaParts.length) {
    main.appendChild(el("div", { class: "client-project-meta" }, [metaParts.join(" · ")]));
  }

  const right = el("div", { class: "client-project-side" }, [
    p.status ? el("div", { class: "client-project-status" }, [p.status]) : null,
  ]);

  row.appendChild(main);
  row.appendChild(right);
  return row;
}

// ── Meetings (v0.24) ──────────────────────────────────────────────────
//
// Hidden if no events match — Google's empty result is the same as "not
// connected" from the user's POV here. The brief specifies hide-on-empty
// for the per-client section.

function fmtMeetingTime(ev) {
  if (ev.all_day) return "All day";
  const d = new Date(ev.start);
  if (Number.isNaN(d.getTime())) return ev.start || "";
  return d
    .toLocaleTimeString("en-AU", { hour: "numeric", minute: "2-digit", hour12: true })
    .replace(/\s/g, "")
    .toLowerCase();
}

function renderMeetings(state) {
  const items = state.meetings || [];
  if (items.length === 0) return null; // hide section
  const root = el("section", { class: "client-section", "data-section": "meetings" });
  root.appendChild(el("div", { class: "section-label" }, ["📅 UPCOMING MEETINGS"]));
  const list = el("div", { class: "today-list" });
  items.forEach((ev) => list.appendChild(renderMeetingRow(ev)));
  root.appendChild(list);
  return root;
}

function renderMeetingRow(ev) {
  const row = el("button", {
    class: "today-row today-row-event",
    type: "button",
    "data-event-id": ev.id || "",
  });
  if (ev.html_link) {
    row.addEventListener("click", () => {
      if (window.__TAURI__?.opener?.openUrl) {
        window.__TAURI__.opener.openUrl(ev.html_link).catch(() => window.open(ev.html_link, "_blank"));
      } else {
        window.open(ev.html_link, "_blank");
      }
    });
  }

  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [ev.summary || "(no title)"]),
  ]);
  const metaBits = [];
  metaBits.push(fmtDate(ev.start));
  if (ev.location && !/^https?:\/\//i.test(ev.location)) metaBits.push(ev.location);
  if (ev.attendees && ev.attendees.length) {
    metaBits.push(`${ev.attendees.length} attendee${ev.attendees.length === 1 ? "" : "s"}`);
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
    meetBtn.addEventListener("click", (e) => e.stopPropagation());
    right.appendChild(meetBtn);
  }
  right.appendChild(el("span", { class: "today-row-time" }, [fmtMeetingTime(ev)]));

  row.appendChild(left);
  row.appendChild(right);
  return row;
}

// ── Recent emails (v0.25) ─────────────────────────────────────────────
//
// Hidden when the email list is empty — Google not connected, no Gmail
// scope, or no threads matched the client filter all collapse to the
// same empty state. Caitlin sees a row only when there's something
// useful to surface.

function fmtEmailDate(ms) {
  if (!ms) return "";
  const d = new Date(Number(ms));
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString("en-AU", { weekday: "short", day: "numeric", month: "short" });
}

function renderEmails(state) {
  const items = state.emails || [];
  if (items.length === 0) return null; // hide section
  const root = el("section", { class: "client-section", "data-section": "emails" });
  root.appendChild(el("div", { class: "section-label" }, ["📨 RECENT EMAILS"]));
  const list = el("div", { class: "today-list" });
  items.forEach((t) => list.appendChild(renderEmailRow(t)));
  root.appendChild(list);
  return root;
}

function renderEmailRow(t) {
  const row = el("button", {
    class: "today-row today-row-email",
    type: "button",
    "data-thread-id": t.id || "",
  });
  if (t.web_link) {
    row.addEventListener("click", () => {
      if (window.__TAURI__?.opener?.openUrl) {
        window.__TAURI__.opener.openUrl(t.web_link).catch(() => window.open(t.web_link, "_blank"));
      } else {
        window.open(t.web_link, "_blank");
      }
    });
  }
  const senderDisplay = (t.from || "").replace(/<.*?>/, "").trim() || t.from || "";
  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [t.subject || "(no subject)"]),
    el("div", { class: "today-row-meta" }, [
      [senderDisplay, fmtEmailDate(t.date_ms)].filter(Boolean).join(" · "),
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

// ── Recent Slack activity (v0.26) ─────────────────────────────────────
//
// Hidden when slack_activity is null — Slack OAuth not connected or no
// matching #client-{slug} channel. Shows the last 5 messages in that
// channel; click opens the message in Slack.

function fmtSlackTime(ts) {
  if (!ts) return "";
  // Slack ts looks like "1714898123.001234" — seconds since epoch.
  const secs = Number(String(ts).split(".")[0]);
  if (!Number.isFinite(secs) || secs <= 0) return "";
  const d = new Date(secs * 1000);
  if (Number.isNaN(d.getTime())) return "";
  return d
    .toLocaleTimeString("en-AU", { hour: "numeric", minute: "2-digit", hour12: true })
    .replace(/\s/g, "")
    .toLowerCase();
}

function renderSlackActivity(state) {
  const activity = state.slack_activity;
  if (!activity || !activity.channel) return null;
  const messages = Array.isArray(activity.messages) ? activity.messages : [];

  const root = el("section", { class: "client-section", "data-section": "slack" });
  const ch = activity.channel;
  const headerLabel = `💬 RECENT SLACK ACTIVITY · #${ch.name || "channel"}`;
  const headerRow = el("button", {
    class: "client-section-header-link",
    type: "button",
  }, [el("div", { class: "section-label" }, [headerLabel])]);
  headerRow.addEventListener("click", () => {
    const url = ch.deeplink || ch.web_link;
    if (!url) return;
    if (window.__TAURI__?.opener?.openUrl) {
      window.__TAURI__.opener.openUrl(url).catch(() => window.open(url, "_blank"));
    } else {
      window.open(url, "_blank");
    }
  });
  root.appendChild(headerRow);

  if (messages.length === 0) {
    root.appendChild(
      el("div", { class: "client-empty" }, ["No messages in the last 24 hours."])
    );
    return root;
  }

  const list = el("div", { class: "today-list" });
  messages.forEach((m) => list.appendChild(renderSlackMessageRow(m)));
  root.appendChild(list);
  return root;
}

function renderSlackMessageRow(m) {
  const row = el("button", {
    class: "today-row",
    type: "button",
    "data-message-ts": m.ts || "",
  });
  if (m.permalink) {
    row.addEventListener("click", () => {
      if (window.__TAURI__?.opener?.openUrl) {
        window.__TAURI__.opener.openUrl(m.permalink).catch(() => window.open(m.permalink, "_blank"));
      } else {
        window.open(m.permalink, "_blank");
      }
    });
  }
  const senderName = m.user_name || (m.user ? `<@${m.user}>` : "(system)");
  const text = (m.text || "").slice(0, 200);
  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [senderName]),
    el("div", { class: "today-row-meta" }, [text || "(no text)"]),
  ]);
  const right = el("div", { class: "today-row-side" }, [
    el("span", { class: "today-row-time" }, [fmtSlackTime(m.ts)]),
  ]);
  row.appendChild(left);
  row.appendChild(right);
  return row;
}

// ── Recent Drive activity (v0.25) ─────────────────────────────────────
//
// Hidden when no files came back. Empty list means: Google not
// connected, drive scope missing, no drive_folder_id on the client, or
// no files modified in the last 14 days. Same hide-on-empty pattern.

function fmtDriveTime(s) {
  if (!s) return "";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
  // "Tue 5 May" — short, scannable. Time of day is rarely meaningful
  // for "this file moved", so we drop it.
  return d.toLocaleDateString("en-AU", { weekday: "short", day: "numeric", month: "short" });
}

// Friendly label for the most common Workspace mime types. Anything
// else falls back to a generic "File".
function driveTypeLabel(mime) {
  if (!mime) return "File";
  if (mime === "application/vnd.google-apps.document") return "Doc";
  if (mime === "application/vnd.google-apps.spreadsheet") return "Sheet";
  if (mime === "application/vnd.google-apps.presentation") return "Slides";
  if (mime === "application/vnd.google-apps.folder") return "Folder";
  if (mime === "application/pdf") return "PDF";
  if (mime.startsWith("image/")) return "Image";
  if (mime.startsWith("video/")) return "Video";
  return "File";
}

function renderDriveFiles(state) {
  const items = state.drive_files || [];
  if (items.length === 0) return null;
  const root = el("section", { class: "client-section", "data-section": "drive" });
  root.appendChild(el("div", { class: "section-label" }, ["📁 RECENT DRIVE ACTIVITY"]));
  const list = el("div", { class: "today-list" });
  items.forEach((f) => list.appendChild(renderDriveRow(f)));
  root.appendChild(list);
  return root;
}

function renderDriveRow(f) {
  const row = el("button", {
    class: "today-row",
    type: "button",
    "data-file-id": f.id || "",
  });
  if (f.web_view_link) {
    row.addEventListener("click", () => {
      if (window.__TAURI__?.opener?.openUrl) {
        window.__TAURI__.opener.openUrl(f.web_view_link).catch(() => window.open(f.web_view_link, "_blank"));
      } else {
        window.open(f.web_view_link, "_blank");
      }
    });
  }
  const metaBits = [driveTypeLabel(f.mime_type)];
  if (f.modified_by) metaBits.push(f.modified_by);
  metaBits.push(fmtDriveTime(f.modified_time));
  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [f.name || "(untitled)"]),
    el("div", { class: "today-row-meta" }, [metaBits.filter(Boolean).join(" · ")]),
  ]);
  row.appendChild(left);
  return row;
}

// ── Receipts ──────────────────────────────────────────────────────────

function renderReceipts(state) {
  const items = state.receipts || [];
  const root = el("section", { class: "client-section", "data-section": "receipts" });
  root.appendChild(el("div", { class: "section-label" }, ["📃 RECENT RECEIPTS"]));
  if (items.length === 0) {
    root.appendChild(
      el("div", { class: "client-empty" }, ["No receipts for this client yet."])
    );
    return root;
  }
  const list = el("div", { class: "today-list" });
  items.forEach((r) => list.appendChild(renderReceiptRow(r)));
  root.appendChild(list);
  return root;
}

function renderReceiptRow(r) {
  const row = el("button", {
    class: "today-row",
    type: "button",
    "data-receipt-id": r.id || "",
    "data-airtable-id": r.airtable_id || "",
  });
  row.addEventListener("click", () => dispatch("client:receipt-click", { receipt: r }));

  const left = el("div", { class: "today-row-main" }, [
    el("div", { class: "today-row-title" }, [r.title || "Receipt"]),
    el("div", { class: "today-row-meta" }, [
      `${r.workflow ? r.workflow + " · " : ""}${fmtDate(r.date)}`,
    ]),
  ]);
  const right = el("div", { class: "today-row-side" }, [
    el("span", { class: "today-row-tick-count" }, [
      `${r.ticked} of ${r.total} ticked`,
    ]),
  ]);

  row.appendChild(left);
  row.appendChild(right);
  return row;
}

// ── Workflows grid (pre-scoped) ───────────────────────────────────────

const WORKFLOW_CARDS = [
  {
    key: "monthly-checkin",
    title: "Monthly Check-in",
    meta: "Last 30 days · pre-filled to this client",
  },
  {
    key: "new-campaign-scope",
    title: "New Campaign Scope",
    meta: "Festival, venue, touring, capability",
  },
  {
    key: "build-scope",
    title: "Build Scope",
    meta: "SOW in Caitlin's voice (v0.31)",
  },
  {
    key: "quarterly-review",
    title: "Quarterly Review",
    meta: "Last 90 days · QBR receipt",
  },
  {
    key: "strategic-thinking",
    title: "Strategic Thinking",
    meta: "Open thinking session",
  },
  // Pure-Airtable workflows (v0.21) — no Anthropic call.
  {
    key: "schedule-social-post",
    title: "Schedule social post",
    meta: "Drafts to SocialPosts",
  },
  {
    key: "log-time",
    title: "Log time",
    meta: "Hours → TimeLogs",
  },
  {
    key: "edit-project",
    title: "Edit project",
    meta: "Update fields, file diff",
  },
  // v0.31 Block F — Skills batch 1.
  // NCT caption only surfaces on the NCT client view; gated by client_code.
  {
    key: "nct-caption",
    title: "Draft NCT social caption",
    meta: "Venue voice · 3 variants",
    only_for_client: "NCT",
  },
];

function renderWorkflows(state) {
  const root = el("section", { class: "client-section", "data-section": "workflows" });
  root.appendChild(el("div", { class: "section-label" }, ["Workflows for this client"]));
  const grid = el("div", { class: "client-shortcut-grid" });
  // v0.31: cards may declare `only_for_client` to gate per-client. When
  // set, the card only renders for the matching client code (e.g. the
  // NCT caption writer is NCT-only).
  const code = (state.code || "").toUpperCase();
  WORKFLOW_CARDS.filter((w) => !w.only_for_client || w.only_for_client === code).forEach((w) => {
    const card = el("button", {
      class: "client-shortcut" + (w.placeholder ? " client-shortcut-placeholder" : ""),
      type: "button",
      "data-workflow-key": w.key,
    }, [
      el("div", { class: "client-shortcut-title" }, [w.title]),
      el("div", { class: "client-shortcut-meta" }, [w.meta]),
    ]);
    card.addEventListener("click", () =>
      dispatch("client:workflow-click", {
        key: w.key,
        placeholder: !!w.placeholder,
        client_code: state.code,
      })
    );
    grid.appendChild(card);
  });
  root.appendChild(grid);
  return root;
}

// ── Top-level draw ────────────────────────────────────────────────────

export function draw(state) {
  const container = document.getElementById("client-view");
  if (!container) return;
  container.innerHTML = "";

  // Header is always shown.
  container.appendChild(renderHeader(state));

  // Sections in spec order. Empty workstreams/decisions/commitments
  // collapse cleanly so a brand-new client view stays tidy. Forms
  // toolbar (v0.30) sits at the top so the "send the form" actions
  // are reachable without scrolling.
  const sections = [
    renderClientFormsToolbar(state),
    renderWorkstreams(state),
    renderDecisions(state),
    renderCommitments(state),
    renderMeetings(state),
    renderEmails(state),
    renderSlackActivity(state),
    renderDriveFiles(state),
    renderProjects(state),
    renderReceipts(state),
    renderWorkflows(state),
  ];
  sections.forEach((s) => {
    if (s) container.appendChild(s);
  });

  // Wire the refresh icon.
  const refreshBtn = container.querySelector("#client-refresh-btn");
  if (refreshBtn) {
    refreshBtn.addEventListener("click", () =>
      dispatch("client:refresh-click", { code: state.code })
    );
  }
}

export function drawLoading(code) {
  const container = document.getElementById("client-view");
  if (!container) return;
  container.innerHTML = "";
  container.appendChild(
    el("div", { class: "client-empty" }, [`Loading ${code}...`])
  );
}
