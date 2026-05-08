// Per-client view — sidebar client list.
//
// On app launch (and at most once every 60s), pull the active Clients list
// from Airtable and replace the Companion sidebar's client items. Falls back
// to whatever's already in the DOM if the bridge or Airtable isn't ready.
//
// The 60s cache is in-memory — switching views inside one session doesn't
// re-fetch, but a relaunch always pulls fresh.
//
// v0.28 Block E: each client item also lists its active projects as
// sub-items. The project list is fetched per-client lazily when the
// sidebar mounts, then cached alongside the records. Rendering as a
// flat list (client item, then sub-items underneath) keeps the existing
// click-through behaviour for the parent client item — clicking the
// client name still opens the per-client view.

const CACHE_TTL = 60 * 1000; // 60s

let _cache = {
  records: null,
  projects_by_code: null, // { CODE: ProjectSummary[] }
  subcontractors: null, // [{ code, name, role }]
  fetched_at: 0,
};

function bridge() {
  return window.__TAURI__?.core;
}

async function safeInvoke(cmd, args) {
  const t = bridge();
  if (!t) throw new Error("not in tauri");
  return t.invoke(cmd, args);
}

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

async function fetchClients() {
  const now = Date.now();
  if (_cache.records && now - _cache.fetched_at < CACHE_TTL) {
    return _cache.records;
  }
  // v0.41: get_airtable_status returns IntegrationStatus
  // ({ state, last_verified_at, error_reason, extra }). Treat
  // "verified" as the only signal to attempt the network pull. If
  // the integration is "failing" (PAT rotated, base ID typo, scope
  // missing) we deliberately don't try list_airtable_clients —
  // calling it would 401 and the sidebar would render the
  // misleading "no clients" empty state. Instead we return null so
  // the caller renders "Connect Airtable in Settings".
  let status = null;
  try {
    status = await safeInvoke("get_airtable_status");
  } catch {
    // not in tauri or bridge missing — caller falls back to static markup
  }
  if (!status || status.state !== "verified") return null;

  let raw;
  try {
    raw = await safeInvoke("list_airtable_clients");
  } catch (e) {
    console.warn("sidebar list_airtable_clients failed:", e);
    return null;
  }
  let records;
  try {
    records = JSON.parse(raw).records || [];
  } catch (e) {
    console.warn("sidebar parse failed:", e);
    return null;
  }
  _cache = { records, projects_by_code: {}, fetched_at: now };
  return records;
}

// Pull the active projects for a single client code. Errors return an
// empty list so the sidebar still renders cleanly. Cached on
// `_cache.projects_by_code` for the same TTL as the client list.
async function fetchProjectsForClient(code) {
  if (!code) return [];
  if (_cache.projects_by_code && _cache.projects_by_code[code]) {
    return _cache.projects_by_code[code];
  }
  let list = [];
  try {
    list = await safeInvoke("list_active_projects_for_client", {
      clientCode: code,
    });
  } catch (e) {
    console.warn(`sidebar projects fetch failed for ${code}:`, e);
    list = [];
  }
  if (!Array.isArray(list)) list = [];
  if (_cache.projects_by_code) {
    _cache.projects_by_code[code] = list;
  }
  return list;
}

// Pull active subcontractors. Returns [] on any error so the Team entry
// still renders the (empty) list. Cached on `_cache.subcontractors` for
// the same TTL as the client list.
async function fetchActiveSubcontractors() {
  if (_cache.subcontractors) return _cache.subcontractors;
  let list = [];
  try {
    list = await safeInvoke("list_active_subcontractors");
  } catch (e) {
    console.warn("sidebar list_active_subcontractors failed:", e);
    list = [];
  }
  if (!Array.isArray(list)) list = [];
  _cache.subcontractors = list;
  return list;
}

export function clearCache() {
  _cache = {
    records: null,
    projects_by_code: null,
    subcontractors: null,
    fetched_at: 0,
  };
}

export async function loadClients() {
  const studioSection = document.querySelector(".sidebar-section[data-section='studio']");
  if (!studioSection) return;

  const records = await fetchClients();
  if (!records) {
    // Airtable not configured yet — replace the static "Loading..."
    // with a real placeholder so it doesn't read as a stuck spinner.
    studioSection.innerHTML = "";
    studioSection.appendChild(el("div", { class: "sidebar-label" }, ["Companion"]));
    studioSection.appendChild(
      el("div", { class: "sidebar-empty" }, [
        "Connect Airtable in Settings to see clients",
      ])
    );
    studioSection.appendChild(
      el("a", { class: "sidebar-item", "data-view": "pipeline", href: "#" }, ["Pipeline"])
    );
    const teamItem = el("a", {
      class: "sidebar-item",
      "data-view": "team",
      href: "#",
    }, ["Team"]);
    studioSection.appendChild(teamItem);
    const subsWrapDisc = el("div", {
      class: "sidebar-subitems",
      "data-subcontractors-wrap": "1",
    });
    studioSection.appendChild(subsWrapDisc);
    return;
  }

  // Sort by code so the order is stable across launches.
  records.sort((a, b) => {
    const ac = (a.fields?.code || "").toUpperCase();
    const bc = (b.fields?.code || "").toUpperCase();
    return ac.localeCompare(bc);
  });

  studioSection.innerHTML = "";
  studioSection.appendChild(el("div", { class: "sidebar-label" }, ["Companion"]));

  if (records.length === 0) {
    studioSection.appendChild(
      el("div", { class: "sidebar-empty" }, [
        "No active clients in Airtable",
      ])
    );
  } else {
    // Render each client item, then asynchronously expand it with
    // active projects. The parent click handler still routes to the
    // per-client view; project sub-items route to the per-project view.
    records.forEach((r) => {
      const f = r.fields || {};
      const code = (f.code || "").toUpperCase();
      if (!code) return;
      const name = f.name || code;
      const clientItem = el("a", {
        class: "sidebar-item",
        "data-view": `client-${code}`,
        "data-client-code": code,
        href: "#",
      }, [`${code} — ${name}`]);
      studioSection.appendChild(clientItem);

      // Empty container for project sub-items so we can fill it in
      // place once Airtable returns. Using a marker ensures repeat
      // mounts don't double-up.
      const projectsWrap = el("div", {
        class: "sidebar-subitems",
        "data-projects-for": code,
      });
      studioSection.appendChild(projectsWrap);

      // Fire and forget — the sidebar reflects projects when they
      // arrive without blocking the rest of the boot.
      fetchProjectsForClient(code)
        .then((projects) => {
          renderProjectSubitems(projectsWrap, code, projects);
        })
        .catch(() => {
          // already logged in fetcher — leave the wrap empty.
        });
    });
  }

  studioSection.appendChild(
    el("a", {
      class: "sidebar-item",
      "data-view": "pipeline",
      href: "#",
    }, ["Pipeline"])
  );
  const teamItem = el("a", {
    class: "sidebar-item",
    "data-view": "team",
    href: "#",
  }, ["Team"]);
  studioSection.appendChild(teamItem);

  // v0.38: Team entry expands to active subcontractors. Today this is
  // just Rose; future hires appear automatically. Fire-and-forget so the
  // sidebar paints even when Airtable is slow.
  const subsWrap = el("div", {
    class: "sidebar-subitems",
    "data-subcontractors-wrap": "1",
  });
  studioSection.appendChild(subsWrap);
  fetchActiveSubcontractors()
    .then((subs) => renderSubcontractorSubitems(subsWrap, subs))
    .catch(() => {
      // already logged in fetcher — leave the wrap empty.
    });
}

function renderSubcontractorSubitems(wrap, subs) {
  if (!wrap) return;
  wrap.innerHTML = "";
  if (!subs || subs.length === 0) return;
  subs.forEach((s) => {
    const code = (s.code || "").toUpperCase();
    if (!code) return;
    const label = s.name ? `${s.name}` : code;
    const sub = el("a", {
      class: "sidebar-subitem",
      "data-subcontractor-code": code,
      href: "#",
      title: code,
    }, [label]);
    wrap.appendChild(sub);
  });
}

function renderProjectSubitems(wrap, clientCode, projects) {
  if (!wrap) return;
  wrap.innerHTML = "";
  if (!projects || projects.length === 0) return;
  projects.forEach((p) => {
    const code = p.code || "";
    if (!code) return;
    const label = p.name ? p.name : code;
    const sub = el("a", {
      class: "sidebar-subitem",
      "data-project-code": code,
      "data-client-code": clientCode,
      href: "#",
      title: code,
    }, [label]);
    wrap.appendChild(sub);
  });
}
