// Companion - Cross-conversation search (v0.36 Block F).
//
// Mounts the global header search bar's behaviour:
//   - Debounced (300ms) as-you-type query against `search_companion`.
//   - Cmd-K (or Ctrl-K) focuses the input.
//   - Escape clears + blurs.
//   - Arrow keys + Enter navigate results; click jumps to the source.
//
// Jump-to dispatch is delegated through the existing CustomEvent buses
// so we don't duplicate modal openers. Each result carries a `jump_to`
// envelope from the Rust side; we map kind → existing handler.

const DEBOUNCE_MS = 300;
const RESULT_LIMIT = 30;

const SOURCE_LABELS = {
  conversation: { label: "Conversations", icon: "💬" },
  receipt: { label: "Receipts", icon: "📃" },
  project_note: { label: "Project notes", icon: "📝" },
  decision: { label: "Decisions", icon: "🎯" },
  commitment: { label: "Commitments", icon: "✓" },
};

// Display order for groups in the dropdown.
const GROUP_ORDER = [
  "conversation",
  "receipt",
  "project_note",
  "decision",
  "commitment",
];

export function mount({ invoke, isTauri, onJump, showToast }) {
  const root = document.getElementById("global-search");
  const input = document.getElementById("global-search-input");
  const dropdown = document.getElementById("global-search-dropdown");
  if (!root || !input || !dropdown) {
    console.warn("[search] missing global search DOM nodes");
    return;
  }

  let debounceTimer = null;
  let inFlight = null; // generation counter so stale results don't overwrite
  let generation = 0;
  let currentResults = [];
  let selectedIdx = -1;

  function setDropdown(html, visible = true) {
    if (typeof html === "string") {
      dropdown.innerHTML = html;
    } else {
      dropdown.innerHTML = "";
      dropdown.appendChild(html);
    }
    if (visible) {
      dropdown.removeAttribute("hidden");
      root.classList.add("is-active");
    } else {
      dropdown.setAttribute("hidden", "");
      root.classList.remove("is-active");
    }
  }

  function clearDropdown() {
    setDropdown("", false);
    currentResults = [];
    selectedIdx = -1;
  }

  function showState(text) {
    const div = document.createElement("div");
    div.className = "global-search-state";
    div.textContent = text;
    setDropdown(div, true);
  }

  function highlight(text, query) {
    if (!text) return "";
    if (!query) return escapeHtml(text);
    const q = query.trim();
    if (!q) return escapeHtml(text);
    const lower = text.toLowerCase();
    const qLower = q.toLowerCase();
    const idx = lower.indexOf(qLower);
    if (idx < 0) return escapeHtml(text);
    return (
      escapeHtml(text.slice(0, idx)) +
      "<mark>" +
      escapeHtml(text.slice(idx, idx + q.length)) +
      "</mark>" +
      highlight(text.slice(idx + q.length), q)
    );
  }

  function escapeHtml(s) {
    return String(s)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  function fmtTimestamp(ts) {
    if (!ts) return "";
    try {
      const d = new Date(ts);
      if (isNaN(d.getTime())) return ts;
      const now = new Date();
      const diffMs = now.getTime() - d.getTime();
      const diffDays = Math.floor(diffMs / 86_400_000);
      if (diffDays === 0) return "today";
      if (diffDays === 1) return "yesterday";
      if (diffDays < 7) return `${diffDays}d ago`;
      if (diffDays < 30) return `${Math.floor(diffDays / 7)}w ago`;
      return d.toISOString().slice(0, 10);
    } catch {
      return ts;
    }
  }

  function renderResults(results, query) {
    currentResults = results;
    selectedIdx = -1;

    if (!results.length) {
      showState("No matches.");
      return;
    }

    const grouped = {};
    for (const r of results) {
      if (!grouped[r.source]) grouped[r.source] = [];
      grouped[r.source].push(r);
    }

    const wrap = document.createElement("div");
    let resultIdx = 0;
    for (const key of GROUP_ORDER) {
      const items = grouped[key];
      if (!items || !items.length) continue;
      const meta = SOURCE_LABELS[key] || { label: key, icon: "•" };

      const group = document.createElement("div");
      group.className = "global-search-group";

      const label = document.createElement("div");
      label.className = "global-search-group-label";
      label.innerHTML = `<span class="global-search-group-icon" aria-hidden="true">${escapeHtml(
        meta.icon
      )}</span><span>${escapeHtml(meta.label)} · ${items.length}</span>`;
      group.appendChild(label);

      for (const item of items) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "global-search-result";
        btn.dataset.idx = String(resultIdx);
        btn.innerHTML = `
          <div class="global-search-result-title">${escapeHtml(
            item.title || ""
          )}</div>
          <div class="global-search-result-snippet">${highlight(
            item.snippet || "",
            query
          )}</div>
          <div class="global-search-result-meta">${escapeHtml(
            fmtTimestamp(item.timestamp)
          )}</div>
        `;
        btn.addEventListener("click", () => {
          jumpTo(item);
        });
        btn.addEventListener("mouseenter", () => {
          setSelected(parseInt(btn.dataset.idx, 10));
        });
        group.appendChild(btn);
        resultIdx += 1;
      }
      wrap.appendChild(group);
    }
    setDropdown(wrap, true);
  }

  function setSelected(idx) {
    selectedIdx = idx;
    dropdown.querySelectorAll(".global-search-result").forEach((el) => {
      const i = parseInt(el.dataset.idx, 10);
      if (i === idx) {
        el.classList.add("is-selected");
        // Scroll into view if needed.
        const r = el.getBoundingClientRect();
        const dr = dropdown.getBoundingClientRect();
        if (r.top < dr.top || r.bottom > dr.bottom) {
          el.scrollIntoView({ block: "nearest" });
        }
      } else {
        el.classList.remove("is-selected");
      }
    });
  }

  function jumpTo(result) {
    if (!result || !result.jump_to) return;
    clearDropdown();
    input.blur();
    if (typeof onJump === "function") {
      onJump(result);
    }
  }

  async function runQuery(query) {
    const myGen = ++generation;
    if (!query || query.length < 2) {
      clearDropdown();
      return;
    }

    if (!isTauri) {
      showState("Search is only available in the Companion app.");
      return;
    }

    showState("Searching...");
    try {
      const results = await invoke("search_companion", {
        query,
        limit: RESULT_LIMIT,
      });
      // Drop stale results if a newer query has started.
      if (myGen !== generation) return;
      renderResults(Array.isArray(results) ? results : [], query);
    } catch (err) {
      if (myGen !== generation) return;
      showState(`Search failed: ${err}`);
      console.warn("[search] failed:", err);
    }
  }

  function onInput() {
    const q = input.value.trim();
    if (debounceTimer) clearTimeout(debounceTimer);
    if (!q) {
      generation += 1; // invalidate any in-flight
      clearDropdown();
      return;
    }
    debounceTimer = setTimeout(() => runQuery(q), DEBOUNCE_MS);
  }

  function onKeyDown(e) {
    if (e.key === "Escape") {
      input.value = "";
      clearDropdown();
      input.blur();
      return;
    }
    if (e.key === "ArrowDown") {
      if (currentResults.length === 0) return;
      e.preventDefault();
      setSelected(Math.min(selectedIdx + 1, currentResults.length - 1));
      return;
    }
    if (e.key === "ArrowUp") {
      if (currentResults.length === 0) return;
      e.preventDefault();
      setSelected(Math.max(selectedIdx - 1, 0));
      return;
    }
    if (e.key === "Enter") {
      if (selectedIdx >= 0 && selectedIdx < currentResults.length) {
        e.preventDefault();
        jumpTo(currentResults[selectedIdx]);
      }
    }
  }

  function onFocus() {
    if (currentResults.length > 0 && input.value.trim()) {
      // Re-show last results when refocusing without retyping.
      dropdown.removeAttribute("hidden");
      root.classList.add("is-active");
    }
  }

  function onDocumentClick(e) {
    if (!root.contains(e.target)) {
      // Click outside — collapse but keep results in memory.
      dropdown.setAttribute("hidden", "");
      root.classList.remove("is-active");
    }
  }

  function onGlobalKey(e) {
    // Cmd-K (mac) or Ctrl-K. Focus search.
    const isCmdK =
      (e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K");
    if (isCmdK) {
      e.preventDefault();
      input.focus();
      input.select();
    }
  }

  input.addEventListener("input", onInput);
  input.addEventListener("keydown", onKeyDown);
  input.addEventListener("focus", onFocus);
  document.addEventListener("click", onDocumentClick);
  document.addEventListener("keydown", onGlobalKey);
}
