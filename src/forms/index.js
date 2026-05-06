// Forms layer (v0.30 Block F).
//
// Six Airtable Forms Caitlin builds once in Airtable's web UI (the API
// doesn't expose form creation). Companion stores their URLs in the
// CompanionSettings table on Airtable and surfaces them as "Send Form"
// or "Share Form" buttons across Today, per-client, per-project and
// Settings.
//
// This module owns:
// - The canonical metadata for each form (title, description, base
//   table, prefill behaviour, surface)
// - A 60s in-memory cache of `list_form_urls` so each surface that
//   needs a URL doesn't round-trip Airtable on every render
// - Tiny helpers for prefill URLs, clipboard copy, mailto draft, and
//   "open in browser" via the Tauri opener.
//
// The cache is invalidated whenever the Settings UI saves a URL, so
// freshly-pasted URLs surface immediately on other pages.

const CACHE_TTL_MS = 60_000;

// Canonical order used in Settings UI and the picker fallback.
export const FORM_KEYS = [
  "form_lead_intake",
  "form_discovery_pre_brief",
  "form_content_approval",
  "form_post_campaign_feedback",
  "form_subcontractor_intake",
  "form_weekly_status",
];

export const FORM_META = {
  form_lead_intake: {
    label: "Lead Intake",
    table: "Leads",
    blurb:
      "Public form. Share with prospects. New row lands in Leads at status=cold.",
    prefill: null,
    surfaces: ["today-pipeline", "settings"],
  },
  form_discovery_pre_brief: {
    label: "Discovery Pre-Brief",
    table: "Projects",
    blurb:
      "Sent to a client before the discovery call. Prefilled with the Project record id.",
    prefill: "Project",
    surfaces: ["client", "project", "settings"],
  },
  form_content_approval: {
    label: "Content Approval",
    table: "SocialPosts",
    blurb:
      "One link per draft post. Prefilled with the SocialPost record id so the response binds to the post.",
    prefill: "SocialPost",
    surfaces: ["project", "settings"],
  },
  form_post_campaign_feedback: {
    label: "Post-Campaign Feedback",
    table: "Projects",
    blurb:
      "Sent after a project hits status = wrap or done. Prefilled with the Project record id.",
    prefill: "Project",
    surfaces: ["client", "project", "settings"],
  },
  form_subcontractor_intake: {
    label: "Subcontractor Intake",
    table: "Subcontractors",
    blurb:
      "Sent to a new contractor on day one. Their answers seed the Subcontractor row.",
    prefill: null,
    surfaces: ["team", "settings"],
  },
  form_weekly_status: {
    label: "Weekly Status",
    table: "Receipts",
    blurb:
      "Rose fills weekly. Tracks active workstreams, blockers, hours.",
    prefill: null,
    surfaces: ["personal", "settings"],
  },
};

let _cache = null; // { urls: Map<key, {value, updated_at}>, ts }

function tauri() {
  return window.__TAURI__?.core;
}

async function safeInvoke(cmd, args) {
  const t = tauri();
  if (!t) throw new Error("not in tauri");
  return t.invoke(cmd, args);
}

// Force the next loadForms() to round-trip Airtable. Called from the
// Settings UI after a save.
export function invalidateCache() {
  _cache = null;
}

// Returns Map<key, {value, updated_at}>. Cached for 60s. Empty map on
// failure so callers can keep rendering.
export async function loadForms() {
  const now = Date.now();
  if (_cache && now - _cache.ts < CACHE_TTL_MS) {
    return _cache.urls;
  }
  const urls = new Map();
  try {
    const raw = await safeInvoke("list_form_urls");
    const arr = JSON.parse(raw);
    if (Array.isArray(arr)) {
      arr.forEach((row) => {
        if (row?.key) {
          urls.set(row.key, {
            value: row.value || "",
            updated_at: row.updated_at || null,
          });
        }
      });
    }
  } catch (e) {
    console.warn("list_form_urls failed:", e);
  }
  _cache = { urls, ts: now };
  return urls;
}

// Quick read of a single URL (with cache). Returns "" if not set.
export async function getFormUrl(key) {
  const urls = await loadForms();
  return urls.get(key)?.value || "";
}

// Save a URL. Round-trips to Airtable and invalidates cache.
export async function setFormUrl(key, value) {
  await safeInvoke("set_form_url", { key, value });
  invalidateCache();
}

// Returns base URL with `?prefill_<Field>=<recordId>` appended (or
// merged if there's already a query string). Empty string in, empty
// string out.
export function buildPrefillUrl(baseUrl, prefillField, recordId) {
  const url = (baseUrl || "").trim();
  if (!url) return "";
  if (!prefillField || !recordId) return url;
  const sep = url.includes("?") ? "&" : "?";
  return `${url}${sep}prefill_${encodeURIComponent(prefillField)}=${encodeURIComponent(recordId)}`;
}

// mailto: with subject + body. body is plain text, line breaks become
// %0A. macOS Mail and the user's default mail client both handle it.
export function buildMailto({ to, subject, body }) {
  const params = [];
  if (subject) params.push(`subject=${encodeURIComponent(subject)}`);
  if (body) params.push(`body=${encodeURIComponent(body)}`);
  const qs = params.length ? `?${params.join("&")}` : "";
  return `mailto:${encodeURIComponent(to || "").replace(/%40/g, "@")}${qs}`;
}

export async function copyToClipboard(text) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  // Fallback for non-secure contexts. Should never trigger inside the
  // Tauri app but keeps the preview build working.
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.position = "fixed";
  ta.style.opacity = "0";
  document.body.appendChild(ta);
  ta.select();
  try {
    document.execCommand("copy");
  } finally {
    ta.remove();
  }
}

// Open URL via Tauri opener if available, otherwise window.open.
export function openUrl(url) {
  if (!url) return;
  if (window.__TAURI__?.opener?.openUrl) {
    window.__TAURI__.opener.openUrl(url).catch(() => window.open(url, "_blank"));
  } else {
    window.open(url, "_blank");
  }
}
