// Companion v0.40 — central poll manager.
//
// Sections register a fetch function and a base cadence. The manager:
//
//   1. Runs each section on its own timer while the window is focused.
//   2. Pauses every section when the window blurs (Tauri or web focus).
//   3. Resumes on focus, force-refetching any section last updated
//      more than 60s ago.
//   4. Doubles the cadence after 3 consecutive failures (per section)
//      until a poll succeeds, then drops back to the configured cadence.
//   5. Surfaces freshness markers on each section header via the
//      paintFreshness helper — "Updated Xs ago" / stale / connection
//      issue + retry.
//
// One global instance, exported as `poller`, mounted in main.js. Sections
// register themselves on mount and unregister on view switch.
//
// Cadence presets live in CADENCE_PRESETS. Persisted in localStorage
// under "companion.refresh_cadence" — backend-free per the v0.40 brief
// (CompanionSettings is form-key locked at the moment, migration is a
// future-version concern).

const STORAGE_KEY = "companion.refresh_cadence";
const FOCUS_REFRESH_MS = 60_000;
const FAIL_THRESHOLD = 3;
const STALE_BADGE_REFRESH_MS = 5_000;

export const CADENCE_PRESETS = {
  live: { label: "Live", data_ms: 30_000, status_ms: 10_000 },
  battery_saver: { label: "Battery saver", data_ms: 120_000, status_ms: 30_000 },
  off: { label: "Off", data_ms: null, status_ms: null }, // focus-only
};

export function readCadencePreference() {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v && CADENCE_PRESETS[v]) return v;
  } catch {}
  return "live";
}

export function writeCadencePreference(preset) {
  if (!CADENCE_PRESETS[preset]) return;
  try {
    localStorage.setItem(STORAGE_KEY, preset);
  } catch {}
}

class Poller {
  constructor() {
    this._sections = new Map(); // id -> SectionEntry
    this._badgeTimer = null;
    this._isFocused = !document.hidden;
    this._wireFocusEvents();
  }

  _wireFocusEvents() {
    // Browser-level visibility change works in dev preview (web).
    document.addEventListener("visibilitychange", () => {
      if (document.hidden) this._onBlur();
      else this._onFocus();
    });
    // Window focus/blur fires in Tauri's webview as well; covers the
    // case where the OS hides the window without firing visibilitychange.
    window.addEventListener("blur", () => this._onBlur());
    window.addEventListener("focus", () => this._onFocus());
    // Tauri-specific events. Wire defensively — the listener API is
    // namespaced under window.__TAURI__ on Tauri 2.x and isn't present
    // in plain browser previews.
    const ev = window.__TAURI__?.event;
    if (ev?.listen) {
      ev.listen("tauri://blur", () => this._onBlur()).catch(() => {});
      ev.listen("tauri://focus", () => this._onFocus()).catch(() => {});
    }
  }

  _onBlur() {
    if (!this._isFocused) return;
    this._isFocused = false;
    for (const entry of this._sections.values()) {
      this._stopTimer(entry);
    }
  }

  async _onFocus() {
    if (this._isFocused) return;
    this._isFocused = true;
    const now = Date.now();
    const tasks = [];
    for (const entry of this._sections.values()) {
      // Restart the timer so the next tick is at the proper interval
      // from now rather than catching up on missed ticks at once.
      this._startTimer(entry);
      const lastOk = entry.last_success_at || 0;
      if (now - lastOk > FOCUS_REFRESH_MS) {
        tasks.push(this._tick(entry, { manual: true }));
      } else {
        // Repaint badge so the "Updated Xs ago" reflects real time.
        paintFreshness(entry);
      }
    }
    await Promise.allSettled(tasks);
  }

  // Register a section. Returns an unregister fn.
  //
  //   id          — stable identifier; re-registering replaces.
  //   kind        — "data" or "status"; controls cadence preset bucket.
  //   fetch()     — async fn that performs the work. Throws on failure.
  //   onAfter()   — sync fn called after each successful tick (e.g. to
  //                 redraw). Receives { changed: bool, entry }.
  //   getHeader() — returns the header DOM element for paint-freshness,
  //                 or null if the section isn't currently mounted (we
  //                 still poll, render layer can re-establish later).
  //   getSignature() — returns a string fingerprint of the section's
  //                 data, used to detect "did data change?". Optional.
  register({ id, kind = "data", fetch, onAfter, getHeader, getSignature }) {
    if (!id || typeof fetch !== "function") return () => {};
    this.unregister(id);
    const entry = {
      id,
      kind,
      fetch,
      onAfter: onAfter || (() => {}),
      getHeader: getHeader || (() => null),
      getSignature: getSignature || (() => ""),
      timer: null,
      last_attempt_at: 0,
      last_success_at: 0,
      last_signature: "",
      consecutive_failures: 0,
      backoff_factor: 1, // doubled on failure streak
      in_flight: false,
    };
    this._sections.set(id, entry);
    if (this._isFocused) this._startTimer(entry);
    this._ensureBadgeTimer();
    return () => this.unregister(id);
  }

  unregister(id) {
    const entry = this._sections.get(id);
    if (!entry) return;
    this._stopTimer(entry);
    this._sections.delete(id);
    if (this._sections.size === 0) this._stopBadgeTimer();
  }

  unregisterPrefix(prefix) {
    const ids = Array.from(this._sections.keys()).filter((k) =>
      k.startsWith(prefix)
    );
    ids.forEach((k) => this.unregister(k));
  }

  unregisterAll() {
    Array.from(this._sections.keys()).forEach((k) => this.unregister(k));
  }

  // Manual refresh — called from Cmd-R / the icon.
  async refreshAll() {
    const tasks = [];
    for (const entry of this._sections.values()) {
      tasks.push(this._tick(entry, { manual: true }));
    }
    await Promise.allSettled(tasks);
  }

  // Apply a new cadence preset and restart timers.
  applyPreset(preset) {
    writeCadencePreference(preset);
    if (!this._isFocused) return;
    for (const entry of this._sections.values()) {
      this._startTimer(entry);
    }
  }

  _cadenceMs(entry) {
    const preset = CADENCE_PRESETS[readCadencePreference()] || CADENCE_PRESETS.live;
    const base = entry.kind === "status" ? preset.status_ms : preset.data_ms;
    if (!base) return null; // off — focus-only
    return base * (entry.backoff_factor || 1);
  }

  _startTimer(entry) {
    this._stopTimer(entry);
    const ms = this._cadenceMs(entry);
    if (!ms) return; // off
    entry.timer = setInterval(() => this._tick(entry), ms);
  }

  _stopTimer(entry) {
    if (entry.timer) {
      clearInterval(entry.timer);
      entry.timer = null;
    }
  }

  async _tick(entry, { manual = false } = {}) {
    if (!manual && !this._isFocused) return;
    if (entry.in_flight) return;
    entry.in_flight = true;
    entry.last_attempt_at = Date.now();
    let changed = false;
    let failed = false;
    try {
      await entry.fetch();
      const sig = safeSignature(entry.getSignature);
      if (sig !== entry.last_signature) {
        changed = true;
        entry.last_signature = sig;
      }
      entry.last_success_at = Date.now();
      // Recover from any failure streak.
      if (entry.consecutive_failures > 0 || entry.backoff_factor !== 1) {
        entry.consecutive_failures = 0;
        entry.backoff_factor = 1;
        // Re-establish timer at the base cadence.
        this._startTimer(entry);
      }
    } catch (e) {
      failed = true;
      entry.consecutive_failures += 1;
      console.warn(`[poller] ${entry.id} failed:`, e);
      if (entry.consecutive_failures >= FAIL_THRESHOLD) {
        // Double the cadence (cap at 8x to avoid silent disconnect).
        entry.backoff_factor = Math.min((entry.backoff_factor || 1) * 2, 8);
        this._startTimer(entry);
      }
    } finally {
      entry.in_flight = false;
    }
    paintFreshness(entry);
    try {
      entry.onAfter({ changed, failed, entry });
    } catch (e) {
      console.warn(`[poller] ${entry.id} onAfter threw:`, e);
    }
    if (changed && !failed) pulseHeader(entry);
  }

  _ensureBadgeTimer() {
    if (this._badgeTimer) return;
    this._badgeTimer = setInterval(() => {
      for (const entry of this._sections.values()) {
        paintFreshness(entry);
      }
    }, STALE_BADGE_REFRESH_MS);
  }

  _stopBadgeTimer() {
    if (this._badgeTimer) {
      clearInterval(this._badgeTimer);
      this._badgeTimer = null;
    }
  }
}

function safeSignature(fn) {
  try {
    const v = fn();
    if (v == null) return "";
    return String(v);
  } catch {
    return "";
  }
}

// Wrap a section-label element with the freshness badge. Idempotent.
// Returns the badge element so callers can update it directly if they
// want, but the standard path is paintFreshness(entry).
export function ensureFreshnessBadge(headerEl) {
  if (!headerEl) return null;
  let badge = headerEl.querySelector(".section-updated-badge");
  if (!badge) {
    badge = document.createElement("span");
    badge.className = "section-updated-badge";
    headerEl.appendChild(badge);
    headerEl.classList.add("section-label-row");
  }
  return badge;
}

export function paintFreshness(entry) {
  const header = safeCall(entry.getHeader);
  if (!header) return;
  const badge = ensureFreshnessBadge(header);
  if (!badge) return;

  if (entry.consecutive_failures >= FAIL_THRESHOLD) {
    badge.textContent = "(connection issue)";
    badge.classList.remove("section-stale");
    badge.classList.add("section-error");
    let retry = header.querySelector(".section-retry-btn");
    if (!retry) {
      retry = document.createElement("button");
      retry.type = "button";
      retry.className = "section-retry-btn";
      retry.textContent = "Retry";
      retry.addEventListener("click", (e) => {
        e.stopPropagation();
        poller._tick(entry, { manual: true });
      });
      header.appendChild(retry);
    }
    return;
  }

  // Drop any retry button that lingered after recovery.
  const retry = header.querySelector(".section-retry-btn");
  if (retry) retry.remove();
  badge.classList.remove("section-error");

  if (!entry.last_success_at) {
    badge.textContent = "Loading...";
    badge.classList.remove("section-stale");
    return;
  }

  const lastFailed = entry.consecutive_failures > 0;
  const ageS = Math.max(0, Math.round((Date.now() - entry.last_success_at) / 1000));
  badge.textContent = lastFailed
    ? `Last successful update ${formatAge(ageS)}`
    : `Updated ${formatAge(ageS)}`;
  badge.classList.toggle("section-stale", lastFailed);
}

function pulseHeader(entry) {
  const header = safeCall(entry.getHeader);
  if (!header) return;
  // Restart the animation by toggling the class off and on next frame.
  header.classList.remove("section-pulse");
  // Force reflow so re-adding the class restarts the animation.
  // eslint-disable-next-line no-unused-expressions
  header.offsetWidth;
  header.classList.add("section-pulse");
  setTimeout(() => header.classList.remove("section-pulse"), 1600);
}

function safeCall(fn) {
  try { return fn(); } catch { return null; }
}

function formatAge(s) {
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.floor(m / 60);
  return `${h}h ago`;
}

// Single global instance.
export const poller = new Poller();
