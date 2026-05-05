// Per-client view — sidebar client list.
//
// On app launch (and at most once every 60s), pull the active Clients list
// from Airtable and replace the Studio sidebar's client items. Falls back
// to whatever's already in the DOM if the bridge or Airtable isn't ready.
//
// The 60s cache is in-memory — switching views inside one session doesn't
// re-fetch, but a relaunch always pulls fresh.

const CACHE_TTL = 60 * 1000; // 60s

let _cache = {
  records: null,
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
  let configured = false;
  try {
    configured = await safeInvoke("get_airtable_status");
  } catch {
    // not in tauri or bridge missing — caller falls back to static markup
  }
  if (!configured) return null;

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
  _cache = { records, fetched_at: now };
  return records;
}

export function clearCache() {
  _cache = { records: null, fetched_at: 0 };
}

export async function loadClients() {
  const studioSection = document.querySelector(".sidebar-section[data-section='studio']");
  if (!studioSection) return;

  const records = await fetchClients();
  if (!records) {
    // Airtable not configured yet — replace the static "Loading..."
    // with a real placeholder so it doesn't read as a stuck spinner.
    studioSection.innerHTML = "";
    studioSection.appendChild(el("div", { class: "sidebar-label" }, ["Studio"]));
    studioSection.appendChild(
      el("div", { class: "sidebar-empty" }, [
        "Connect Airtable in Settings to see clients",
      ])
    );
    studioSection.appendChild(
      el("a", { class: "sidebar-item", "data-view": "pipeline", href: "#" }, ["Pipeline"])
    );
    return;
  }

  // Sort by code so the order is stable across launches.
  records.sort((a, b) => {
    const ac = (a.fields?.code || "").toUpperCase();
    const bc = (b.fields?.code || "").toUpperCase();
    return ac.localeCompare(bc);
  });

  studioSection.innerHTML = "";
  studioSection.appendChild(el("div", { class: "sidebar-label" }, ["Studio"]));

  if (records.length === 0) {
    studioSection.appendChild(
      el("div", { class: "sidebar-empty" }, [
        "No active clients in Airtable",
      ])
    );
  } else {
    records.forEach((r) => {
      const f = r.fields || {};
      const code = (f.code || "").toUpperCase();
      if (!code) return;
      const name = f.name || code;
      studioSection.appendChild(
        el("a", {
          class: "sidebar-item",
          "data-view": `client-${code}`,
          "data-client-code": code,
          href: "#",
        }, [`${code} — ${name}`])
      );
    });
  }

  studioSection.appendChild(
    el("a", {
      class: "sidebar-item",
      "data-view": "pipeline",
      href: "#",
    }, ["Pipeline"])
  );
}
