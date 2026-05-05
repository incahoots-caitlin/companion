// Studio drift checker — Block B6.
//
// Compares expected state against actual state across the codebase, the
// Dropbox docs and Airtable. Surfaces gaps on the Today dashboard so
// Caitlin sees them before they bite.
//
// Each `check_*` returns a `Vec<DriftItem>` (zero or more items). The
// public `run_all_checks` function fans them out and returns the
// concatenated list. Network-bound checks (Airtable) fail soft: if
// they return an error, the check returns an empty vec so the
// dashboard still renders.
//
// Cache lives in `AppState` (lib.rs). 1-hour TTL. Manual refresh from
// the dashboard's existing refresh icon flushes the cache by calling
// `check_drift` with `force: true`.
//
// File reads are best-effort: a missing path turns into a "(file
// missing)" drift item at low severity rather than crashing.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Serialize, Clone, Debug)]
pub struct DriftItem {
    pub title: String,
    pub severity: Severity,
    pub action: String,
    pub surface: String,
}

// ── Path helpers ─────────────────────────────────────────────────────

fn home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn studio_repo() -> Option<PathBuf> {
    home().map(|h| h.join("code/studio"))
}

fn context_repo() -> Option<PathBuf> {
    home().map(|h| h.join("code/context"))
}

fn dropbox_root() -> Option<PathBuf> {
    home().map(|h| h.join("Library/CloudStorage/Dropbox/IN CAHOOTS"))
}

// ── Check 1: version stamp drift ─────────────────────────────────────

fn read_version_from_json(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("version")?.as_str().map(|s| s.to_string())
}

fn read_version_from_cargo(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    // Look for `version = "x.y.z"` under [package]. First match wins —
    // dependency versions in Cargo.toml come after [package] but the
    // [package] block is at the top of the file in our setup.
    for line in raw.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("version") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                if let Some(start) = rest.find('"') {
                    let after = &rest[start + 1..];
                    if let Some(end) = after.find('"') {
                        return Some(after[..end].to_string());
                    }
                }
            }
        }
    }
    None
}

pub fn check_version_stamps() -> Vec<DriftItem> {
    let repo = match studio_repo() {
        Some(r) => r,
        None => return vec![],
    };
    let pkg_path = repo.join("package.json");
    let conf_path = repo.join("src-tauri/tauri.conf.json");
    let cargo_path = repo.join("src-tauri/Cargo.toml");

    let pkg = read_version_from_json(&pkg_path);
    let conf = read_version_from_json(&conf_path);
    let cargo = read_version_from_cargo(&cargo_path);

    let mut items = Vec::new();

    // If any file is unreadable, surface that as drift (high) and stop
    // comparing — comparison is meaningless with a missing file.
    let missing: Vec<&str> = [
        ("package.json", pkg.is_some()),
        ("tauri.conf.json", conf.is_some()),
        ("Cargo.toml", cargo.is_some()),
    ]
    .iter()
    .filter_map(|(name, ok)| if *ok { None } else { Some(*name) })
    .collect();

    if !missing.is_empty() {
        items.push(DriftItem {
            title: format!("Version file unreadable: {}", missing.join(", ")),
            severity: Severity::High,
            action: "Check the file exists and is valid".to_string(),
            surface: repo.display().to_string(),
        });
        return items;
    }

    let pkg = pkg.unwrap();
    let conf = conf.unwrap();
    let cargo = cargo.unwrap();

    if pkg != conf || pkg != cargo || conf != cargo {
        items.push(DriftItem {
            title: format!(
                "Version drift: package.json {}, tauri.conf.json {}, Cargo.toml {}",
                pkg, conf, cargo
            ),
            severity: Severity::High,
            action: "Bump all three to the same value".to_string(),
            surface: repo.display().to_string(),
        });
    }

    items
}

// ── Check 2: design system sync ──────────────────────────────────────

fn sha256_file(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
}

pub fn check_design_system_sync() -> Vec<DriftItem> {
    let studio = match studio_repo() {
        Some(r) => r.join("src/styles/design-system"),
        None => return vec![],
    };
    let context = match context_repo() {
        Some(r) => r.join("app/src/styles/design-system"),
        None => return vec![],
    };

    if !studio.exists() {
        // Studio's own design-system folder missing means something is
        // very wrong — surface it.
        return vec![DriftItem {
            title: "Studio design-system folder missing".to_string(),
            severity: Severity::Medium,
            action: "Restore src/styles/design-system/".to_string(),
            surface: studio.display().to_string(),
        }];
    }

    if !context.exists() {
        // Context repo not checked out locally is a normal state on a
        // fresh machine — fail soft as the spec calls for.
        return vec![];
    }

    let dir = match fs::read_dir(&studio) {
        Ok(d) => d,
        Err(_) => return vec![],
    };

    let mut differing: Vec<String> = Vec::new();
    for entry in dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.ends_with(".css") {
            continue;
        }
        let context_file = context.join(&name);
        if !context_file.exists() {
            differing.push(format!("{} (only in Studio)", name));
            continue;
        }
        let a = sha256_file(&path);
        let b = sha256_file(&context_file);
        match (a, b) {
            (Some(x), Some(y)) if x != y => {
                differing.push(name);
            }
            _ => {}
        }
    }

    if differing.is_empty() {
        return vec![];
    }

    vec![DriftItem {
        title: format!(
            "Design system out of sync: {} differ{}",
            differing.join(", "),
            if differing.len() == 1 { "s" } else { "" }
        ),
        severity: Severity::Medium,
        action: "Run scripts/sync-design-from-context.sh".to_string(),
        surface: studio.display().to_string(),
    }]
}

// ── Check 3: spec.md last-reconciled date ────────────────────────────

pub fn check_spec_reconciliation() -> Vec<DriftItem> {
    let path = match dropbox_root() {
        Some(p) => p.join("STUDIO/spec.md"),
        None => return vec![],
    };
    let raw = match fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => {
            return vec![DriftItem {
                title: "spec.md missing".to_string(),
                severity: Severity::Low,
                action: "Restore Dropbox/IN CAHOOTS/STUDIO/spec.md".to_string(),
                surface: path.display().to_string(),
            }];
        }
    };

    // Find the "Last reconciled" line. Format example:
    //   **Last reconciled with code and architecture plan:** 4 May 2026.
    let line = raw
        .lines()
        .find(|l| l.to_lowercase().contains("last reconciled"));
    let line = match line {
        Some(l) => l,
        None => {
            return vec![DriftItem {
                title: "spec.md missing 'Last reconciled' line".to_string(),
                severity: Severity::Low,
                action: "Add a 'Last reconciled' stamp to spec.md".to_string(),
                surface: path.display().to_string(),
            }];
        }
    };

    let parsed = parse_reconciled_date(line);
    let reconciled = match parsed {
        Some(d) => d,
        None => {
            return vec![DriftItem {
                title: format!("spec.md reconciled date unparseable: {}", line.trim()),
                severity: Severity::Low,
                action: "Fix the date format in spec.md".to_string(),
                surface: path.display().to_string(),
            }];
        }
    };

    let today = chrono::Local::now().date_naive();
    let age_days = (today - reconciled).num_days();
    if age_days > 30 {
        vec![DriftItem {
            title: format!(
                "spec.md last reconciled {} ({} days ago)",
                reconciled.format("%-d %b %Y"),
                age_days
            ),
            severity: Severity::Low,
            action: "Update spec.md current-version stamp".to_string(),
            surface: path.display().to_string(),
        }]
    } else {
        vec![]
    }
}

fn parse_reconciled_date(line: &str) -> Option<chrono::NaiveDate> {
    // Try the "4 May 2026" form first (what spec.md uses), then fall
    // back to ISO YYYY-MM-DD.
    let after_colon = line.rfind(':').map(|i| &line[i + 1..]).unwrap_or(line);
    let cleaned = after_colon.trim().trim_end_matches('.').trim().trim_matches('*').trim();
    let formats = ["%-d %B %Y", "%d %B %Y", "%-d %b %Y", "%d %b %Y", "%Y-%m-%d"];
    for fmt in &formats {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(cleaned, fmt) {
            return Some(d);
        }
    }
    None
}

// ── Check 4: Lumin references in Studio source ───────────────────────

fn grep_recursive(root: &Path, needle_lower: &str) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            if let Ok(it) = fs::read_dir(&p) {
                for e in it.flatten() {
                    stack.push(e.path());
                }
            }
        } else if p.is_file() {
            // Skip binary-ish files. We only care about source.
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let ext_ok = name.ends_with(".rs")
                || name.ends_with(".js")
                || name.ends_with(".ts")
                || name.ends_with(".html")
                || name.ends_with(".css")
                || name.ends_with(".md")
                || name.ends_with(".toml")
                || name.ends_with(".json");
            if !ext_ok {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&p) {
                if content.to_lowercase().contains(needle_lower) {
                    hits.push(p);
                }
            }
        }
    }
    hits
}

pub fn check_lumin_references() -> Vec<DriftItem> {
    let repo = match studio_repo() {
        Some(r) => r,
        None => return vec![],
    };
    let mut hits = Vec::new();
    for sub in ["src", "src-tauri/src"] {
        let p = repo.join(sub);
        if p.exists() {
            hits.extend(grep_recursive(&p, "lumin"));
        }
    }
    if hits.is_empty() {
        return vec![];
    }
    let names: Vec<String> = hits
        .iter()
        .map(|p| {
            p.strip_prefix(&repo)
                .unwrap_or(p)
                .display()
                .to_string()
        })
        .collect();
    let preview = if names.len() <= 3 {
        names.join(", ")
    } else {
        format!("{} (+{} more)", names[..3].join(", "), names.len() - 3)
    };
    vec![DriftItem {
        title: format!("Lumin references in Studio source: {}", preview),
        severity: Severity::Medium,
        action: "Replace Lumin with Dropbox Sign".to_string(),
        surface: repo.display().to_string(),
    }]
}

// ── Check 5: Tally references in forms folder ────────────────────────

pub fn check_tally_references() -> Vec<DriftItem> {
    let folder = match dropbox_root() {
        Some(p) => p.join("STUDIO/forms"),
        None => return vec![],
    };
    if !folder.exists() {
        return vec![];
    }
    let hits = grep_recursive(&folder, "tally");
    if hits.is_empty() {
        return vec![];
    }
    let names: Vec<String> = hits
        .iter()
        .filter_map(|p| {
            p.strip_prefix(&folder)
                .ok()
                .map(|s| s.display().to_string())
        })
        .collect();
    let preview = if names.len() <= 3 {
        names.join(", ")
    } else {
        format!("{} (+{} more)", names[..3].join(", "), names.len() - 3)
    };
    vec![DriftItem {
        title: format!("Tally references in forms folder: {}", preview),
        severity: Severity::Low,
        action: "Move forms to Airtable Forms per schema.md".to_string(),
        surface: folder.display().to_string(),
    }]
}

// ── Check 7: team-workflow.md retired alongside Operating Model ──────

pub fn check_team_workflow_retired() -> Vec<DriftItem> {
    let root = match dropbox_root() {
        Some(p) => p,
        None => return vec![],
    };
    let old = root.join("team-workflow.md");
    let new = root.join("TEAM HUB/Onboarding/01 — Operating Model.md");
    if old.exists() && new.exists() {
        return vec![DriftItem {
            title: "team-workflow.md still exists alongside Operating Model".to_string(),
            severity: Severity::Low,
            action: "Archive Dropbox/IN CAHOOTS/team-workflow.md".to_string(),
            surface: old.display().to_string(),
        }];
    }
    vec![]
}

// ── Check 10: Studio _archive folder cleanup ─────────────────────────

pub fn check_archive_cleanup() -> Vec<DriftItem> {
    let archive = match studio_repo() {
        Some(r) => r.join("_archive"),
        None => return vec![],
    };
    if !archive.exists() {
        return vec![];
    }
    let mut count = 0usize;
    let mut stack: Vec<PathBuf> = vec![archive.clone()];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            if let Ok(it) = fs::read_dir(&p) {
                for e in it.flatten() {
                    stack.push(e.path());
                }
            }
        } else if p.is_file() {
            count += 1;
        }
    }
    if count == 0 {
        return vec![];
    }
    vec![DriftItem {
        title: format!(
            "_archive/ has {} file{} pending cleanup",
            count,
            if count == 1 { "" } else { "s" }
        ),
        severity: Severity::Low,
        action: "Move _archive contents out of the repo".to_string(),
        surface: archive.display().to_string(),
    }]
}

// ── Airtable-backed checks ───────────────────────────────────────────
//
// These call existing helpers in lib.rs via the closure `airtable_get`
// passed in by the Tauri command. Keeping the network plumbing in
// lib.rs means we don't duplicate Keychain/HTTP setup here.

pub fn check_workstreams_stale(records: &serde_json::Value) -> Vec<DriftItem> {
    let mut stale = Vec::new();
    let now = chrono::Local::now();
    let arr = match records["records"].as_array() {
        Some(a) => a,
        None => return vec![],
    };
    for r in arr {
        let f = &r["fields"];
        let status = f["status"].as_str().unwrap_or("");
        if status != "active" {
            continue;
        }
        let last = f["last_touch_at"].as_str().unwrap_or("");
        if last.is_empty() {
            continue;
        }
        let parsed = chrono::DateTime::parse_from_rfc3339(last)
            .ok()
            .map(|d| d.with_timezone(&chrono::Local));
        let parsed = match parsed {
            Some(d) => d,
            None => continue,
        };
        let days = (now - parsed).num_days();
        if days >= 7 {
            let code = f["code"].as_str().unwrap_or("?");
            let title = f["title"].as_str().unwrap_or("(untitled)");
            stale.push((days, code.to_string(), title.to_string()));
        }
    }
    if stale.is_empty() {
        return vec![];
    }
    stale.sort_by(|a, b| b.0.cmp(&a.0));
    let top = stale
        .iter()
        .take(3)
        .map(|(d, c, t)| format!("{} {} ({}d)", c, t, d))
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if stale.len() > 3 {
        format!(" (+{} more)", stale.len() - 3)
    } else {
        String::new()
    };
    vec![DriftItem {
        title: format!("Stale active workstreams: {}{}", top, suffix),
        severity: Severity::Medium,
        action: "Touch each workstream or move it to recovery / blocked".to_string(),
        surface: "Airtable Workstreams".to_string(),
    }]
}

pub fn check_overdue_commitments(records: &serde_json::Value) -> Vec<DriftItem> {
    let mut overdue = Vec::new();
    let now = chrono::Local::now();
    let arr = match records["records"].as_array() {
        Some(a) => a,
        None => return vec![],
    };
    for r in arr {
        let f = &r["fields"];
        let status = f["status"].as_str().unwrap_or("");
        if status != "open" {
            continue;
        }
        let due = f["due_at"].as_str().unwrap_or("");
        if due.is_empty() {
            continue;
        }
        let parsed = chrono::DateTime::parse_from_rfc3339(due)
            .ok()
            .map(|d| d.with_timezone(&chrono::Local));
        let parsed = match parsed {
            Some(d) => d,
            None => continue,
        };
        if parsed >= now {
            continue;
        }
        let title = f["title"].as_str().unwrap_or("(untitled)");
        let days = (now - parsed).num_days().max(0);
        overdue.push((days, title.to_string()));
    }
    if overdue.is_empty() {
        return vec![];
    }
    overdue.sort_by(|a, b| b.0.cmp(&a.0));
    let preview = overdue
        .iter()
        .take(3)
        .map(|(d, t)| format!("{} ({}d)", t, d))
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if overdue.len() > 3 {
        format!(" (+{} more)", overdue.len() - 3)
    } else {
        String::new()
    };
    vec![DriftItem {
        title: format!(
            "{} overdue commitment{}: {}{}",
            overdue.len(),
            if overdue.len() == 1 { "" } else { "s" },
            preview,
            suffix
        ),
        severity: Severity::High,
        action: "Close, reschedule, or cancel each overdue commitment".to_string(),
        surface: "Airtable Commitments".to_string(),
    }]
}

pub fn check_overdue_decisions(records: &serde_json::Value) -> Vec<DriftItem> {
    let mut overdue = Vec::new();
    let today = chrono::Local::now().date_naive();
    let arr = match records["records"].as_array() {
        Some(a) => a,
        None => return vec![],
    };
    for r in arr {
        let f = &r["fields"];
        let status = f["status"].as_str().unwrap_or("");
        if status != "open" {
            continue;
        }
        let due = f["due_date"].as_str().unwrap_or("");
        if due.is_empty() {
            continue;
        }
        let parsed = chrono::NaiveDate::parse_from_str(due, "%Y-%m-%d").ok();
        let parsed = match parsed {
            Some(d) => d,
            None => continue,
        };
        if parsed >= today {
            continue;
        }
        let title = f["title"].as_str().unwrap_or("(untitled)");
        let days = (today - parsed).num_days();
        overdue.push((days, title.to_string()));
    }
    if overdue.is_empty() {
        return vec![];
    }
    overdue.sort_by(|a, b| b.0.cmp(&a.0));
    let preview = overdue
        .iter()
        .take(3)
        .map(|(d, t)| format!("{} ({}d)", t, d))
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if overdue.len() > 3 {
        format!(" (+{} more)", overdue.len() - 3)
    } else {
        String::new()
    };
    vec![DriftItem {
        title: format!(
            "{} overdue decision{}: {}{}",
            overdue.len(),
            if overdue.len() == 1 { "" } else { "s" },
            preview,
            suffix
        ),
        severity: Severity::High,
        action: "Make each call or push the due date".to_string(),
        surface: "Airtable Decisions".to_string(),
    }]
}
