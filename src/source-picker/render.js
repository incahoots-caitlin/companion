// Source-picker — render (v0.32 Block F).
//
// Builds the dropdown + secondary input + "Pull" button. Mounts inside
// any modal that wants the source-picker pattern. Returns a small handle
// the modal uses to read the chosen source / fetched context blob:
//
//   const picker = mountSourcePicker({ container, state, available });
//   ...
//   const blob = await picker.ensureContextBlob();
//   if (blob === null) return; // user dismissed an error toast
//
// Sources that aren't connected are hidden silently per the brief.

import {
  SOURCE_TYPES,
  SOURCE_LABELS,
  setSource,
  setSourceRef,
} from "./state.js";
import { fetchContext } from "./fetch.js";

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === "class") node.className = v;
    else if (k === "style") node.setAttribute("style", v);
    else if (k in node) node[k] = v;
    else node.setAttribute(k, v);
  }
  for (const c of children) {
    if (c == null) continue;
    if (typeof c === "string") node.appendChild(document.createTextNode(c));
    else node.appendChild(c);
  }
  return node;
}

const SOURCE_PLACEHOLDERS = {
  granola: "Optional. Substring of meeting title to filter (e.g. 'discovery'). Blank = most recent.",
  gmail: "Gmail thread ID or a search expression (e.g. from:client@example.com newer_than:7d).",
  slack: "Slack thread URL, or leave blank to read the last 7 days of #client-{slug}.",
  calendar: "Calendar event ID, or YYYY-MM-DD to grab today's first event for that date.",
  form: "Form key. Defaults to form_lead_intake. Use form_discovery_pre_brief for discovery.",
  manual: "Paste the source text. Anything goes — call notes, brief, screenshot OCR, raw email.",
};

const SOURCE_HELP = {
  granola: "Pulls Granola meeting transcripts via the existing v0.22 connector.",
  gmail: "Pulls a Gmail thread body via the v0.25 Gmail integration.",
  slack: "Pulls a Slack thread or recent #client- channel activity (v0.26).",
  calendar: "Pulls a Google Calendar event description and attendees (v0.24).",
  form: "Reads the latest Lead Intake or Discovery Pre-Brief submission for this client from Airtable.",
  manual: "Plain paste. Use this for anything that doesn't fit the other sources.",
};

export function mountSourcePicker({ container, state, available, label = "Brief source" }) {
  const wrap = el("div", { class: "source-picker", style: "margin-top: 16px;" });

  const heading = el("div", { class: "settings-label" }, [label]);
  wrap.appendChild(heading);

  // Filter source types to only those available right now.
  const enabledTypes = SOURCE_TYPES.filter((t) => available?.[t] !== false);

  // Two-column row: source select + secondary ref input.
  const row = el("div", {
    style: "display: grid; grid-template-columns: 200px 1fr; gap: 10px; margin-top: 6px;",
  });

  const select = el("select", { class: "settings-input" });
  enabledTypes.forEach((t) => {
    const opt = document.createElement("option");
    opt.value = t;
    opt.textContent = SOURCE_LABELS[t];
    select.appendChild(opt);
  });
  if (!enabledTypes.includes(state.sourceType)) {
    state.sourceType = enabledTypes[0] || "manual";
  }
  select.value = state.sourceType;

  const refInput = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: SOURCE_PLACEHOLDERS[state.sourceType] || "",
    value: state.sourceRef || "",
  });

  row.appendChild(select);
  row.appendChild(refInput);
  wrap.appendChild(row);

  // Manual paste textarea — only rendered when source = manual.
  const manualPad = el("div", { style: "margin-top: 10px;" });
  const manualArea = el("textarea", {
    class: "modal-textarea",
    placeholder: "Paste the source text here. Anything goes — call notes, brief, raw email body, etc.",
    rows: 5,
  });
  manualPad.appendChild(manualArea);

  // Help line + status row.
  const help = el("div", { class: "settings-meta", style: "margin-top: 6px;" }, [
    SOURCE_HELP[state.sourceType] || "",
  ]);
  wrap.appendChild(help);

  const status = el("div", { class: "settings-meta", style: "margin-top: 6px;" }, [""]);
  wrap.appendChild(status);

  // Pull button — non-manual sources need a fetch step before Run. The
  // workflow's Run handler can also call ensureContextBlob() on submit
  // if the user forgot to click Pull.
  const pullBtn = el("button", {
    type: "button",
    class: "button button-secondary",
    style: "margin-top: 8px; padding: 4px 12px; font-size: 12px;",
  }, ["Pull source"]);

  const updateLayout = () => {
    const t = state.sourceType;
    refInput.placeholder = SOURCE_PLACEHOLDERS[t] || "";
    help.textContent = SOURCE_HELP[t] || "";
    if (t === "manual") {
      refInput.style.display = "none";
      pullBtn.style.display = "none";
      if (!manualPad.parentNode) wrap.insertBefore(manualPad, help);
    } else {
      refInput.style.display = "";
      pullBtn.style.display = "";
      if (manualPad.parentNode) manualPad.remove();
    }
    status.textContent = state.contextBlob
      ? "Source ready."
      : state.error
      ? `Error: ${state.error}`
      : "";
  };

  wrap.appendChild(pullBtn);

  select.addEventListener("change", () => {
    setSource(state, select.value);
    refInput.value = "";
    manualArea.value = "";
    updateLayout();
  });
  refInput.addEventListener("input", () => {
    setSourceRef(state, refInput.value.trim());
    state.contextBlob = null;
    status.textContent = "";
  });
  manualArea.addEventListener("input", () => {
    setSourceRef(state, manualArea.value);
    state.contextBlob = null;
  });

  pullBtn.addEventListener("click", async () => {
    pullBtn.disabled = true;
    pullBtn.textContent = "Pulling...";
    status.textContent = "";
    await fetchContext(state);
    pullBtn.disabled = false;
    pullBtn.textContent = "Pull source";
    updateLayout();
  });

  container.appendChild(wrap);
  updateLayout();

  // Modal Run handler calls this before invoking the workflow. Returns
  // the blob (string) or null on error / empty manual paste.
  async function ensureContextBlob() {
    // Manual: synthesise the blob locally so the Rust router still sees
    // a normalised envelope.
    if (state.sourceType === "manual") {
      const body = manualArea.value.trim();
      if (!body) return ""; // empty manual paste returns "" — workflow decides if it's required
      return await fetchContext({ ...state, sourceRef: body }) ?? wrapManualLocally(body);
    }
    if (state.contextBlob) return state.contextBlob;
    const blob = await fetchContext(state);
    updateLayout();
    return blob;
  }

  return {
    state,
    ensureContextBlob,
    getSourceType: () => state.sourceType,
    getSourceRef: () =>
      state.sourceType === "manual" ? manualArea.value : state.sourceRef,
  };
}

// Local fallback envelope — if the backend manual path errors for any
// reason we still wrap the paste in the same shape Rust would emit, so
// the workflow's user message construction stays uniform.
function wrapManualLocally(body) {
  const ts = new Date().toISOString();
  return `# Source: Manual paste\n\nRetrieved: ${ts}\n\n---\n\n${body}`;
}
