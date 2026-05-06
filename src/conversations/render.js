// Conversations chat surface — render (v0.27 Block D).
//
// Per the brief's open-question decision: layout (b) — full-pane replacing
// the per-client right pane when a workstream is selected. The chat takes
// over `#client-view`. A workstream rail on the left lists this client's
// active and blocked workstreams; the chat for the selected workstream
// fills the rest. A back button returns to the standard per-client view.
//
// Pure render: read state, mount markup, dispatch CustomEvents for clicks.
// main.js wires the event handlers.

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
  return d.toLocaleDateString("en-AU", { day: "numeric", month: "short", year: "numeric" });
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

// ── Workstream rail ───────────────────────────────────────────────────

function renderRail(workstreams, activeCode) {
  const rail = el("div", { class: "chat-rail" });
  rail.appendChild(
    el("div", { class: "chat-rail-header" }, [
      el("button", {
        class: "chat-back-btn",
        type: "button",
        title: "Back to client view",
        "aria-label": "Back to client view",
      }, ["← Back"]),
      el("div", { class: "chat-rail-title" }, ["Workstreams"]),
    ])
  );

  if (!workstreams || workstreams.length === 0) {
    rail.appendChild(
      el("div", { class: "chat-rail-empty" }, ["No active workstreams for this client."])
    );
    return rail;
  }

  const list = el("div", { class: "chat-rail-list" });
  workstreams.forEach((w) => {
    const isActive = (w.code || "") === activeCode;
    const item = el("button", {
      class: "chat-rail-item" + (isActive ? " is-active" : ""),
      type: "button",
      "data-workstream-code": w.code || "",
    }, [
      el("div", { class: "chat-rail-item-code" }, [w.code || ""]),
      el("div", { class: "chat-rail-item-title" }, [w.title || "(untitled)"]),
      w.next_action
        ? el("div", { class: "chat-rail-item-meta" }, [`Next: ${w.next_action}`])
        : w.blocker
        ? el("div", { class: "chat-rail-item-meta" }, [`Blocked: ${w.blocker}`])
        : null,
    ]);
    item.addEventListener("click", () =>
      dispatch("conversation:rail-click", { workstream: w })
    );
    list.appendChild(item);
  });
  rail.appendChild(list);

  // Wire back button
  rail.querySelector(".chat-back-btn").addEventListener("click", () =>
    dispatch("conversation:back-click", {})
  );

  return rail;
}

// ── Chat pane ─────────────────────────────────────────────────────────

function renderHeader(state, workstream) {
  const header = el("div", { class: "chat-header" });
  const titleRow = el("div", { class: "chat-header-row" }, [
    el("div", { class: "chat-header-title" }, [
      state.workstream_title || state.workstream_code || "Conversation",
    ]),
  ]);
  if (workstream && state.status !== "archived") {
    const infoBtn = el("button", {
      class: "chat-header-action",
      type: "button",
      title: "Workstream details",
    }, ["Details"]);
    infoBtn.addEventListener("click", () =>
      dispatch("conversation:info-click", { workstream })
    );
    titleRow.appendChild(infoBtn);
  }
  header.appendChild(titleRow);

  const meta = el("div", { class: "chat-header-meta" });
  if (state.workstream_code) {
    meta.appendChild(el("span", { class: "chat-header-code" }, [state.workstream_code]));
  }
  if (state.status === "archived") {
    meta.appendChild(el("span", { class: "chat-header-archived" }, [
      `Archived${state.last_message_at ? ` ${fmtDate(state.last_message_at)}` : ""}`,
    ]));
  } else if (state.last_message_at) {
    meta.appendChild(el("span", {}, [`Last message ${fmtDate(state.last_message_at)} ${fmtTime(state.last_message_at)}`]));
  } else if (state.status === "new") {
    meta.appendChild(el("span", {}, ["New conversation"]));
  }
  header.appendChild(meta);
  return header;
}

function renderMessage(msg) {
  const cls = msg.role === "user" ? "chat-msg chat-msg-user" : "chat-msg chat-msg-assistant";
  const node = el("div", { class: cls });
  const body = el("div", { class: "chat-msg-body" });
  // Plain-text rendering with newline preservation. No markdown for v0.27 —
  // adds parsing surface and we'd rather ship a working chat than a fancy one.
  const lines = String(msg.content || "").split("\n");
  lines.forEach((line, i) => {
    if (i > 0) body.appendChild(el("br"));
    body.appendChild(document.createTextNode(line));
  });
  node.appendChild(body);
  if (msg.ts) {
    node.appendChild(el("div", { class: "chat-msg-ts" }, [fmtTime(msg.ts)]));
  }
  return node;
}

function renderTranscript(state) {
  const wrap = el("div", { class: "chat-transcript" });
  if (state.messages.length === 0 && state.status === "new") {
    wrap.appendChild(
      el("div", { class: "chat-empty" }, ["New conversation. Type to start."])
    );
  } else if (state.messages.length === 0) {
    wrap.appendChild(el("div", { class: "chat-empty" }, ["No messages yet."]));
  } else {
    state.messages.forEach((m) => wrap.appendChild(renderMessage(m)));
  }
  if (state.sending) {
    wrap.appendChild(
      el("div", { class: "chat-msg chat-msg-assistant chat-msg-pending" }, [
        el("div", { class: "chat-msg-body" }, ["Thinking…"]),
      ])
    );
  }
  return wrap;
}

function renderComposer(state) {
  if (state.status === "archived") {
    return el("div", { class: "chat-archived-banner" }, [
      "This workstream is done. The conversation is archived (read-only).",
    ]);
  }
  const composer = el("form", { class: "chat-composer" });
  const ta = el("textarea", {
    class: "chat-input",
    placeholder: "Message…",
    rows: 2,
  });
  ta.disabled = !!state.sending;
  const send = el("button", {
    class: "button chat-send",
    type: "submit",
  }, [state.sending ? "Sending…" : "Send"]);
  send.disabled = !!state.sending;
  composer.appendChild(ta);
  composer.appendChild(send);

  composer.addEventListener("submit", (e) => {
    e.preventDefault();
    const text = ta.value.trim();
    if (!text) return;
    if (state.sending) return;
    ta.value = "";
    dispatch("conversation:send", { text });
  });
  // Cmd/Ctrl+Enter to send.
  ta.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      composer.requestSubmit();
    }
  });
  // Auto-focus on mount.
  setTimeout(() => ta.focus(), 0);
  return composer;
}

// ── Top-level draw ────────────────────────────────────────────────────

export function draw(state, workstreams) {
  const container = document.getElementById("client-view");
  if (!container) return;
  container.innerHTML = "";
  container.classList.add("client-view-chat");

  const layout = el("div", { class: "chat-layout" });
  layout.appendChild(renderRail(workstreams, state.workstream_code));

  // Resolve the active workstream from the rail list so the Details
  // button in the header can hand it to the legacy detail modal.
  const activeWorkstream =
    (workstreams || []).find((w) => (w.code || "") === state.workstream_code) || null;

  const pane = el("div", { class: "chat-pane" });
  pane.appendChild(renderHeader(state, activeWorkstream));
  pane.appendChild(renderTranscript(state));
  pane.appendChild(renderComposer(state));
  layout.appendChild(pane);

  container.appendChild(layout);

  // Scroll transcript to the bottom so the latest message is visible.
  requestAnimationFrame(() => {
    const tr = container.querySelector(".chat-transcript");
    if (tr) tr.scrollTop = tr.scrollHeight;
  });
}

export function drawLoading() {
  const container = document.getElementById("client-view");
  if (!container) return;
  container.innerHTML = "";
  container.classList.add("client-view-chat");
  container.appendChild(
    el("div", { class: "chat-loading" }, ["Loading conversation…"])
  );
}

// Called when leaving the chat view so the standard per-client view
// renders cleanly without leftover chat classes.
export function exitChat() {
  const container = document.getElementById("client-view");
  if (container) container.classList.remove("client-view-chat");
}
