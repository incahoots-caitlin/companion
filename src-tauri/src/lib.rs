// Companion - Rust backend
//
// Tauri commands the frontend invokes:
//   get_api_key_status()          -> bool
//   save_api_key(key)             -> Result<(), String>
//   get_slack_status()            -> bool
//   save_slack_webhook(url)       -> Result<(), String>
//   run_strategic_thinking(input) -> Result<String, String>
//   list_receipts(limit)          -> Result<Vec<String>, String>
//   delete_receipt(id)            -> Result<(), String>
//   tick_item(receipt_id, idx)    -> Result<String, String>  (returns updated JSON)

mod calendar;
mod cfo;
mod conversations;
mod drift;
mod drive;
mod gmail;
mod google;
mod granola;
mod oauth;
mod project_feed;
mod search;
mod slack;
mod source_picker;

use keyring::Entry;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, State,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const KEYRING_SERVICE: &str = "marketing.incahoots.studio";
const KEYRING_USER: &str = "anthropic-api-key";
const KEYRING_SLACK: &str = "slack-webhook-url";
const KEYRING_AIRTABLE_KEY: &str = "airtable-api-key";
const KEYRING_AIRTABLE_BASE: &str = "airtable-base-id";
pub(crate) const ANTHROPIC_API: &str = "https://api.anthropic.com/v1/messages";
const AIRTABLE_API: &str = "https://api.airtable.com/v0";
pub(crate) const MODEL_ID: &str = "claude-opus-4-7";

// Cheap model used for background enrichment (auto-tag + auto-summary on
// receipt write, drift filtering, etc). Calls are short and we don't need
// Opus for them.
const HAIKU_MODEL_ID: &str = "claude-haiku-4-5-20251001";

// The 12 canonical Receipts.tags choices. Companion's auto-tagger picks
// 1-3 from this list per receipt. Airtable's create_record uses
// typecast: true so any new tag will be added to the field, but we
// constrain the prompt to this list to keep tag noise down.
const RECEIPT_TAG_CHOICES: &[&str] = &[
    "strategy",
    "scope",
    "creative",
    "reporting",
    "onboarding",
    "review",
    "decision",
    "follow-up",
    "win",
    "blocker",
    "internal",
    "client-facing",
];

// Beta header for the Anthropic Messages API native MCP connector.
// When this is sent, the request body may include `mcp_servers` and
// Claude will invoke the listed MCP tools server-side.
// Source: https://platform.claude.com/docs/en/agents-and-tools/mcp-connector
const ANTHROPIC_MCP_BETA: &str = "mcp-client-2025-11-20";

// Tracks the unix-timestamp of the last successful Granola pull. Used
// for the Settings "Last pull" indicator. Survives restarts via
// Keychain.
const KEYRING_GRANOLA_LAST_PULL: &str = "granola-last-pull-at";

// Same idea, for the Google Calendar pull (v0.24). Stamped on every
// successful list_calendar_today / list_calendar_week so the Settings
// "Last sync" line knows when calendar data was last freshened. Gmail
// and Drive (v0.25) share the same stamp — Settings shows the latest
// successful Google call regardless of surface.
const KEYRING_GOOGLE_LAST_SYNC: &str = "google-last-sync-at";

// Records which scopes the user actually granted on their last
// successful re-authorise. Stamped to "calendar,gmail,drive" today;
// future versions may grant fewer when the user opts out of some
// surfaces. Settings reads this to render the connected-scope summary.
const KEYRING_GOOGLE_GRANTED_SCOPES: &str = "google-granted-scopes";

// v0.26: timestamp of the last successful Slack read API call. Same
// pattern as KEYRING_GOOGLE_LAST_SYNC — Settings UI reads this to render
// the "Last sync" line under the Slack row.
const KEYRING_SLACK_LAST_SYNC: &str = "slack-oauth-last-sync-at";

// v0.41: per-integration verify-on-save metadata. Each integration gets
// two Keychain entries — RFC3339 timestamp of the last successful
// verification, and the human-readable failure reason from the last
// attempted verification. Settings reads these to render the three-state
// IntegrationStatus (not configured / verified / failing) without
// hitting the integration's network on every render. See verify_*
// helpers below for what counts as "verified".
const KEYRING_AIRTABLE_LAST_VERIFIED_AT: &str = "airtable-last-verified-at";
const KEYRING_AIRTABLE_LAST_ERROR: &str = "airtable-last-error";
const KEYRING_ANTHROPIC_LAST_VERIFIED_AT: &str = "anthropic-last-verified-at";
const KEYRING_ANTHROPIC_LAST_ERROR: &str = "anthropic-last-error";
const KEYRING_SLACK_WEBHOOK_LAST_VERIFIED_AT: &str = "slack-webhook-last-verified-at";
const KEYRING_SLACK_WEBHOOK_LAST_ERROR: &str = "slack-webhook-last-error";
const KEYRING_SLACK_OAUTH_LAST_VERIFIED_AT: &str = "slack-oauth-last-verified-at";
const KEYRING_SLACK_OAUTH_LAST_ERROR: &str = "slack-oauth-last-error";
const KEYRING_GRANOLA_LAST_VERIFIED_AT: &str = "granola-last-verified-at";
const KEYRING_GRANOLA_LAST_ERROR: &str = "granola-last-error";
const KEYRING_GOOGLE_LAST_VERIFIED_AT: &str = "google-last-verified-at";
const KEYRING_GOOGLE_LAST_ERROR: &str = "google-last-error";

// Munbyn reprint paths. Both live in the team-shared Dropbox so Rose
// also has them, but only Caitlin's Mac has the printer queue named
// "Munbyn", so reprint will fail soft on Rose's machine.
//
// Resolved at runtime via $HOME so the published binary doesn't bake in
// any single user's path. Each user's Dropbox lives at the same relative
// location, just under their own home dir.
const TYPST_REL: &str =
    "Library/CloudStorage/Dropbox/IN CAHOOTS/TEMPLATES/RECEIPT-DOCS/typst";
const PRINT_SCRIPT_REL: &str =
    "Library/CloudStorage/Dropbox/IN CAHOOTS/TEMPLATES/RECEIPT-DOCS/print-thermal.sh";

fn home_dir() -> Result<String, String> {
    std::env::var("HOME").map_err(|_| "HOME not set".to_string())
}

fn typst_dir() -> Result<String, String> {
    Ok(format!("{}/{}", home_dir()?, TYPST_REL))
}

fn print_script() -> Result<String, String> {
    Ok(format!("{}/{}", home_dir()?, PRINT_SCRIPT_REL))
}

const STRATEGIC_THINKING_PROMPT: &str =
    include_str!("../prompts/strategic-thinking-system.md");
const NEW_CLIENT_ONBOARDING_PROMPT: &str =
    include_str!("../prompts/new-client-onboarding-system.md");
const NEW_CAMPAIGN_SCOPE_PROMPT: &str =
    include_str!("../prompts/new-campaign-scope-system.md");
const QUARTERLY_REVIEW_PROMPT: &str =
    include_str!("../prompts/quarterly-review-system.md");
const SUBCONTRACTOR_ONBOARDING_PROMPT: &str =
    include_str!("../prompts/subcontractor-onboarding-system.md");

// v0.31 Block F — Skills batch 1.
//
// Each skill's SKILL.md is bundled in at compile time alongside a shared
// receipt envelope that explains how Companion expects the model to wrap
// its output. The full system prompt is `SKILL_BODY + RECEIPT_ENVELOPE`,
// joined at runtime by `skill_system_prompt`. Workflows wired to skills
// in this release: northcote-theatre-caption-writer, in-cahoots-social-
// manager, monthly-checkin (upgraded), campaign-wrap-report,
// scope-of-work-builder.
const SKILL_RECEIPT_ENVELOPE: &str =
    include_str!("../prompts/skills/_receipt-envelope.md");
const SKILL_NCT_CAPTION: &str =
    include_str!("../prompts/skills/northcote-theatre-caption-writer.md");
const SKILL_IN_CAHOOTS_SOCIAL: &str =
    include_str!("../prompts/skills/in-cahoots-social-manager.md");
const SKILL_MONTHLY_CHECKIN: &str =
    include_str!("../prompts/skills/monthly-checkin.md");
const SKILL_CAMPAIGN_WRAP_REPORT: &str =
    include_str!("../prompts/skills/campaign-wrap-report.md");
const SKILL_SCOPE_OF_WORK_BUILDER: &str =
    include_str!("../prompts/skills/scope-of-work-builder.md");

// v0.32 Block F — Skills batch 2.
const SKILL_PRESS_RELEASE_WRITER: &str =
    include_str!("../prompts/skills/press-release-writer.md");
const SKILL_EDM_WRITER: &str = include_str!("../prompts/skills/edm-writer.md");
const SKILL_REELS_SCRIPTING: &str =
    include_str!("../prompts/skills/reels-scripting.md");
const SKILL_HOOK_GENERATOR: &str =
    include_str!("../prompts/skills/hook-generator.md");
const SKILL_CLIENT_EMAIL: &str = include_str!("../prompts/skills/client-email.md");
const SKILL_HUMANIZER: &str = include_str!("../prompts/skills/humanizer.md");
const SKILL_IN_CAHOOTS_COPY_EDITOR: &str =
    include_str!("../prompts/skills/in-cahoots-copy-editor.md");

// v0.39 Block F — Multi-source dossier draft.
const SKILL_CLIENT_ONBOARDING: &str =
    include_str!("../prompts/skills/client-onboarding.md");

fn skill_system_prompt(skill_body: &str) -> String {
    format!("{}\n\n{}", skill_body, SKILL_RECEIPT_ENVELOPE)
}

// ── DB state ───────────────────────────────────────────────────────────

pub struct DbState(pub Mutex<Connection>);

// ── Drift cache ────────────────────────────────────────────────────────
//
// Last result of `check_drift` plus the timestamp. 1-hour TTL. The
// dashboard's manual refresh button passes `force: true` to bypass it.

const DRIFT_TTL: Duration = Duration::from_secs(60 * 60);

pub struct DriftCache(pub Mutex<Option<(Instant, Vec<drift::DriftItem>)>>);

// ── Rate limit state ────────────────────────────────────────────────────
// Defends against a buggy frontend looping a Claude API call and burning
// credits. Single-user desktop app, so a global "last call time" is plenty.

const RATE_LIMIT_INTERVAL: Duration = Duration::from_secs(5);

pub struct RateLimit(pub Mutex<Option<Instant>>);

fn check_rate_limit(state: &RateLimit) -> Result<(), String> {
    let mut guard = state.0.lock().map_err(|e| format!("Rate-limit lock: {}", e))?;
    let now = Instant::now();
    if let Some(last) = *guard {
        let elapsed = now.duration_since(last);
        if elapsed < RATE_LIMIT_INTERVAL {
            let wait = RATE_LIMIT_INTERVAL - elapsed;
            return Err(format!(
                "Slow down — wait {}s before running another workflow",
                wait.as_secs() + 1
            ));
        }
    }
    *guard = Some(now);
    Ok(())
}

fn init_db(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS receipts (
            id TEXT PRIMARY KEY,
            project TEXT NOT NULL,
            workflow TEXT NOT NULL,
            title TEXT NOT NULL,
            date TEXT NOT NULL,
            json TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );
        CREATE INDEX IF NOT EXISTS idx_receipts_created ON receipts(created_at DESC);",
    )
}

fn persist_receipt(conn: &Connection, json: &str) -> Result<(), String> {
    let parsed: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Invalid receipt JSON: {}", e))?;
    let id = parsed["id"].as_str().ok_or("Receipt missing id")?;
    let project = parsed["project"].as_str().unwrap_or("in-cahoots-studio");
    let workflow = parsed["workflow"].as_str().unwrap_or("unknown");
    let title = parsed["title"].as_str().unwrap_or("Receipt");
    let date = parsed["date"].as_str().unwrap_or("");
    conn.execute(
        "INSERT OR REPLACE INTO receipts (id, project, workflow, title, date, json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, project, workflow, title, date, json],
    )
    .map_err(|e| format!("DB write: {}", e))?;
    Ok(())
}

// ── Secrets cache ──────────────────────────────────────────────────────
//
// Process-global cache of Keychain values keyed by account name. We hit
// macOS Keychain once at app launch (warmed in the Tauri setup hook), then
// serve every API call from memory. Without this, each Anthropic, Slack
// or Airtable call re-prompts the user with "Studio wants to use your
// confidential information stored in 'X'" until they tick "Always Allow"
// for every binary rebuild.
//
// Save handlers refresh the cache so a rotated key takes effect
// immediately without an app restart.

fn secrets_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_secret(account: &str) -> Option<String> {
    {
        let guard = secrets_cache().lock().ok()?;
        if let Some(v) = guard.get(account) {
            return Some(v.clone());
        }
    }
    let v = Entry::new(KEYRING_SERVICE, account)
        .ok()?
        .get_password()
        .ok()?;
    if let Ok(mut guard) = secrets_cache().lock() {
        guard.insert(account.to_string(), v.clone());
    }
    Some(v)
}

fn cache_secret(account: &str, value: &str) {
    if let Ok(mut guard) = secrets_cache().lock() {
        guard.insert(account.to_string(), value.to_string());
    }
}

// ── Integration status (v0.41) ─────────────────────────────────────────
//
// Three-state status replaces the binary `bool` returns the v0.40
// status commands used. Settings can now tell the truth: whether a
// credential is missing, present-and-verified, or present-but-failing.
//
// Each integration's save command (or OAuth callback) runs a cheap
// verification call against the provider. Success writes a fresh
// `last_verified_at` and clears any cached error. Failure clears the
// timestamp and writes the error message. Settings reads from the
// cached timestamps and never hits the network on open — which keeps
// the panel snappy and avoids a thundering herd of API calls every
// time Caitlin glances at it.
//
// The `extra` field carries integration-specific metadata that doesn't
// fit the shared shape: Google's granted scope list, Granola's
// last_pull_at, Slack OAuth's last_sync_at, OAuth-flow `has_client_id`
// flags. Frontend reads from here when rendering per-integration
// extras below the shared status block.

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum IntegrationState {
    NotConfigured,
    Verified,
    Failing,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct IntegrationStatus {
    state: IntegrationState,
    // RFC3339 — only set when state == Verified.
    last_verified_at: Option<String>,
    // Human-readable; only set when state == Failing.
    error_reason: Option<String>,
    // Per-integration extras (Google scopes, Granola last_pull_at, etc).
    extra: serde_json::Value,
}

impl IntegrationStatus {
    fn not_configured_with_extra(extra: serde_json::Value) -> Self {
        IntegrationStatus {
            state: IntegrationState::NotConfigured,
            last_verified_at: None,
            error_reason: None,
            extra,
        }
    }

    fn verified(last_verified_at: String, extra: serde_json::Value) -> Self {
        IntegrationStatus {
            state: IntegrationState::Verified,
            last_verified_at: Some(last_verified_at),
            error_reason: None,
            extra,
        }
    }

    fn failing(error_reason: String, extra: serde_json::Value) -> Self {
        IntegrationStatus {
            state: IntegrationState::Failing,
            last_verified_at: None,
            error_reason: Some(error_reason),
            extra,
        }
    }
}

// Stamps "we just verified this" by writing now() to the verified-at
// account and clearing any stale error account.
fn record_verification_success(verified_at_account: &str, error_account: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    let _ = oauth::write_keychain(verified_at_account, &now);
    let _ = oauth::delete_keychain(error_account);
    cache_secret(verified_at_account, &now);
    invalidate_cached_secret(error_account);
}

// Stamps "we tried and it failed" by writing the error reason to the
// error account and clearing any stale verified-at.
fn record_verification_failure(
    verified_at_account: &str,
    error_account: &str,
    reason: &str,
) {
    let _ = oauth::write_keychain(error_account, reason);
    let _ = oauth::delete_keychain(verified_at_account);
    cache_secret(error_account, reason);
    invalidate_cached_secret(verified_at_account);
}

fn invalidate_cached_secret(account: &str) {
    if let Ok(mut guard) = secrets_cache().lock() {
        guard.remove(account);
    }
}

// Reads the cached verified-at and error reason for an integration
// and folds them into an IntegrationStatus, given the provided
// "is configured?" check and the per-integration extra payload. Use
// this in every get_*_status command so the shape stays consistent.
fn build_status_from_cache(
    configured: bool,
    verified_at_account: &str,
    error_account: &str,
    extra: serde_json::Value,
) -> IntegrationStatus {
    if !configured {
        return IntegrationStatus::not_configured_with_extra(extra);
    }
    let last_verified = oauth::read_keychain(verified_at_account);
    let last_error = oauth::read_keychain(error_account);
    if let Some(ts) = last_verified {
        IntegrationStatus::verified(ts, extra)
    } else {
        // Configured but never verified (or last verify failed).
        IntegrationStatus::failing(
            last_error.unwrap_or_else(|| "Not yet verified".to_string()),
            extra,
        )
    }
}

// ── Auth ────────────────────────────────────────────────────────────────

pub(crate) fn read_anthropic_key() -> Option<String> {
    // 1. Env var (dev convenience).
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    // 2. macOS Keychain (cached after first hit).
    cached_secret(KEYRING_USER)
}

// ── Verify-on-save helpers (v0.41) ──────────────────────────────────────
//
// Each helper does the cheapest call that proves the credential is
// actually alive — not just that it exists in Keychain. Returns
// Result<(), String> so callers can stamp success/failure metadata in
// one spot. Errors flow through verbatim to the user; the goal is for
// Settings to surface the real reason ("Invalid API key", "Base not
// found", "missing scope") rather than pretending everything is fine.

async fn verify_anthropic(api_key: &str) -> Result<(), String> {
    // GET /v1/models is free and proves the key is valid.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let resp = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Anthropic: {}", e))?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    if status.as_u16() == 401 {
        return Err("Invalid API key (Anthropic returned 401)".to_string());
    }
    Err(format!("Anthropic {}: {}", status.as_u16(), body))
}

async fn verify_slack_webhook(url: &str) -> Result<(), String> {
    // Post a single "Companion connection check" message. Slack returns
    // plain text "ok" on success; anything else means the webhook is
    // dead, revoked, or the URL is wrong.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let body = serde_json::json!({ "text": "Companion connection check" });
    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Slack webhook: {}", e))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status.is_success() {
        let trimmed = text.trim();
        if trimmed.eq_ignore_ascii_case("ok") {
            return Ok(());
        }
        // 200 but body wasn't "ok" — surface what Slack said.
        return Err(format!("Slack webhook unexpected response: {}", trimmed));
    }
    if status.as_u16() == 404 {
        return Err("Webhook not found (404). The webhook may have been deleted.".to_string());
    }
    if status.as_u16() == 410 {
        return Err("Webhook revoked (410). Generate a new one in Slack.".to_string());
    }
    Err(format!("Slack webhook {}: {}", status.as_u16(), text))
}

async fn verify_airtable(api_key: &str, base_id: &str) -> Result<(), String> {
    // Hit the meta endpoint — proves PAT + Base ID + scopes in one
    // shot. 401 = invalid PAT; 403 = missing scope; 404 = base not
    // found; 422 = malformed.
    let url = format!("{}/meta/bases/{}/tables", AIRTABLE_API, base_id);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let resp = client
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Airtable: {}", e))?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    let snippet = body.chars().take(240).collect::<String>();
    let reason = match status.as_u16() {
        401 => "Invalid personal access token (Airtable returned 401)".to_string(),
        403 => format!(
            "Token missing required scope. Airtable said: {}",
            snippet.trim()
        ),
        404 => "Base not found. Check the Base ID is correct.".to_string(),
        422 => format!("Airtable rejected the request: {}", snippet.trim()),
        code => format!("Airtable {}: {}", code, snippet.trim()),
    };
    Err(reason)
}

async fn verify_slack_oauth() -> Result<(), String> {
    // auth.test is the cheapest call that proves the user-token is
    // alive. We don't reach into slack.rs for this because the OAuth
    // verify path doesn't need the team_id — just yes-or-no liveness.
    let token = oauth::ensure_fresh_token(&slack::SLACK)
        .await
        .map_err(|e| format!("Slack token: {}", e))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let url = format!("{}/auth.test", slack::SLACK_API_BASE);
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Slack: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Slack {}: {}", status.as_u16(), body));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Parse: {}", e))?;
    if body.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        Ok(())
    } else {
        let err = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        Err(format!("Slack auth.test failed: {}", err))
    }
}

async fn verify_granola() -> Result<(), String> {
    // Granola exposes only the MCP surface and OAuth metadata; there's
    // no introspection endpoint we can hit cheaply. Wrapping
    // ensure_fresh_token covers the failure modes that matter most:
    // expired access token + revoked refresh token = surfaces here.
    // If Caitlin's refresh token is alive, we treat the integration as
    // verified; the next pull will fail loud if the token is somehow
    // accepted but rejected by MCP.
    oauth::ensure_fresh_token(&granola::GRANOLA)
        .await
        .map_err(|e| format!("Granola token refresh failed: {}", e))?;
    Ok(())
}

async fn verify_google() -> Result<(), String> {
    // userinfo is the canonical liveness probe for Google OAuth tokens.
    let token = oauth::ensure_fresh_token(&google::GOOGLE)
        .await
        .map_err(|e| format!("Google token: {}", e))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let resp = client
        .get("https://www.googleapis.com/oauth2/v3/userinfo")
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Google: {}", e))?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    let snippet = body.chars().take(240).collect::<String>();
    match status.as_u16() {
        401 => Err("Google token revoked or expired (401). Re-authorise.".to_string()),
        403 => Err(format!(
            "Google denied the call (403). Required scope may be missing: {}",
            snippet.trim()
        )),
        code => Err(format!("Google {}: {}", code, snippet.trim())),
    }
}

#[tauri::command]
fn get_api_key_status() -> IntegrationStatus {
    build_status_from_cache(
        read_anthropic_key().is_some(),
        KEYRING_ANTHROPIC_LAST_VERIFIED_AT,
        KEYRING_ANTHROPIC_LAST_ERROR,
        serde_json::Value::Object(serde_json::Map::new()),
    )
}

#[tauri::command]
async fn save_api_key(key: String) -> Result<IntegrationStatus, String> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("Empty key".to_string());
    }
    let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| format!("Keychain error: {}", e))?;
    entry
        .set_password(trimmed)
        .map_err(|e| format!("Keychain write error: {}", e))?;
    cache_secret(KEYRING_USER, trimmed);

    match verify_anthropic(trimmed).await {
        Ok(()) => {
            record_verification_success(
                KEYRING_ANTHROPIC_LAST_VERIFIED_AT,
                KEYRING_ANTHROPIC_LAST_ERROR,
            );
        }
        Err(reason) => {
            record_verification_failure(
                KEYRING_ANTHROPIC_LAST_VERIFIED_AT,
                KEYRING_ANTHROPIC_LAST_ERROR,
                &reason,
            );
        }
    }
    Ok(get_api_key_status())
}

fn read_slack_webhook() -> Option<String> {
    cached_secret(KEYRING_SLACK)
}

#[tauri::command]
fn get_slack_status() -> IntegrationStatus {
    build_status_from_cache(
        read_slack_webhook().is_some(),
        KEYRING_SLACK_WEBHOOK_LAST_VERIFIED_AT,
        KEYRING_SLACK_WEBHOOK_LAST_ERROR,
        serde_json::Value::Object(serde_json::Map::new()),
    )
}

#[tauri::command]
async fn save_slack_webhook(url: String) -> Result<IntegrationStatus, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("Empty URL".to_string());
    }
    if !trimmed.starts_with("https://hooks.slack.com/") {
        return Err("Expected a Slack incoming-webhook URL (hooks.slack.com/...)".to_string());
    }
    let entry = Entry::new(KEYRING_SERVICE, KEYRING_SLACK)
        .map_err(|e| format!("Keychain error: {}", e))?;
    entry
        .set_password(trimmed)
        .map_err(|e| format!("Keychain write error: {}", e))?;
    cache_secret(KEYRING_SLACK, trimmed);

    match verify_slack_webhook(trimmed).await {
        Ok(()) => {
            record_verification_success(
                KEYRING_SLACK_WEBHOOK_LAST_VERIFIED_AT,
                KEYRING_SLACK_WEBHOOK_LAST_ERROR,
            );
        }
        Err(reason) => {
            record_verification_failure(
                KEYRING_SLACK_WEBHOOK_LAST_VERIFIED_AT,
                KEYRING_SLACK_WEBHOOK_LAST_ERROR,
                &reason,
            );
        }
    }
    Ok(get_slack_status())
}

// ── Airtable ───────────────────────────────────────────────────────────

pub(crate) fn read_airtable_creds() -> Option<(String, String)> {
    let key = cached_secret(KEYRING_AIRTABLE_KEY)?;
    let base = cached_secret(KEYRING_AIRTABLE_BASE)?;
    Some((key, base))
}

#[tauri::command]
fn get_airtable_status() -> IntegrationStatus {
    build_status_from_cache(
        read_airtable_creds().is_some(),
        KEYRING_AIRTABLE_LAST_VERIFIED_AT,
        KEYRING_AIRTABLE_LAST_ERROR,
        serde_json::Value::Object(serde_json::Map::new()),
    )
}

#[tauri::command]
async fn save_airtable_credentials(
    api_key: String,
    base_id: String,
) -> Result<IntegrationStatus, String> {
    let key = api_key.trim();
    let base = base_id.trim();
    if key.is_empty() || base.is_empty() {
        return Err("Both API key and base ID required".to_string());
    }
    if !base.starts_with("app") {
        return Err("Base ID must start with 'app' (find it at airtable.com/api)".to_string());
    }
    Entry::new(KEYRING_SERVICE, KEYRING_AIRTABLE_KEY)
        .map_err(|e| format!("Keychain: {}", e))?
        .set_password(key)
        .map_err(|e| format!("Save key: {}", e))?;
    Entry::new(KEYRING_SERVICE, KEYRING_AIRTABLE_BASE)
        .map_err(|e| format!("Keychain: {}", e))?
        .set_password(base)
        .map_err(|e| format!("Save base: {}", e))?;
    cache_secret(KEYRING_AIRTABLE_KEY, key);
    cache_secret(KEYRING_AIRTABLE_BASE, base);

    match verify_airtable(key, base).await {
        Ok(()) => {
            record_verification_success(
                KEYRING_AIRTABLE_LAST_VERIFIED_AT,
                KEYRING_AIRTABLE_LAST_ERROR,
            );
        }
        Err(reason) => {
            record_verification_failure(
                KEYRING_AIRTABLE_LAST_VERIFIED_AT,
                KEYRING_AIRTABLE_LAST_ERROR,
                &reason,
            );
        }
    }
    Ok(get_airtable_status())
}

// ── Manual re-verify command (v0.41) ───────────────────────────────────
//
// Frontend's "Test again" button on a failing row hits this. Runs the
// integration's verification helper, stamps the result, and returns the
// updated status so the row re-renders without a follow-up status call.

#[tauri::command]
async fn verify_integration(name: String) -> Result<IntegrationStatus, String> {
    match name.as_str() {
        "anthropic" => {
            let key = match read_anthropic_key() {
                Some(k) => k,
                None => return Ok(get_api_key_status()),
            };
            match verify_anthropic(&key).await {
                Ok(()) => record_verification_success(
                    KEYRING_ANTHROPIC_LAST_VERIFIED_AT,
                    KEYRING_ANTHROPIC_LAST_ERROR,
                ),
                Err(reason) => record_verification_failure(
                    KEYRING_ANTHROPIC_LAST_VERIFIED_AT,
                    KEYRING_ANTHROPIC_LAST_ERROR,
                    &reason,
                ),
            }
            Ok(get_api_key_status())
        }
        "slack_webhook" => {
            let url = match read_slack_webhook() {
                Some(u) => u,
                None => return Ok(get_slack_status()),
            };
            match verify_slack_webhook(&url).await {
                Ok(()) => record_verification_success(
                    KEYRING_SLACK_WEBHOOK_LAST_VERIFIED_AT,
                    KEYRING_SLACK_WEBHOOK_LAST_ERROR,
                ),
                Err(reason) => record_verification_failure(
                    KEYRING_SLACK_WEBHOOK_LAST_VERIFIED_AT,
                    KEYRING_SLACK_WEBHOOK_LAST_ERROR,
                    &reason,
                ),
            }
            Ok(get_slack_status())
        }
        "airtable" => {
            let (key, base) = match read_airtable_creds() {
                Some(c) => c,
                None => return Ok(get_airtable_status()),
            };
            match verify_airtable(&key, &base).await {
                Ok(()) => record_verification_success(
                    KEYRING_AIRTABLE_LAST_VERIFIED_AT,
                    KEYRING_AIRTABLE_LAST_ERROR,
                ),
                Err(reason) => record_verification_failure(
                    KEYRING_AIRTABLE_LAST_VERIFIED_AT,
                    KEYRING_AIRTABLE_LAST_ERROR,
                    &reason,
                ),
            }
            Ok(get_airtable_status())
        }
        "granola" => {
            if !oauth::is_connected(&granola::GRANOLA) {
                return Ok(get_granola_status());
            }
            match verify_granola().await {
                Ok(()) => record_verification_success(
                    KEYRING_GRANOLA_LAST_VERIFIED_AT,
                    KEYRING_GRANOLA_LAST_ERROR,
                ),
                Err(reason) => record_verification_failure(
                    KEYRING_GRANOLA_LAST_VERIFIED_AT,
                    KEYRING_GRANOLA_LAST_ERROR,
                    &reason,
                ),
            }
            Ok(get_granola_status())
        }
        "google" => {
            if !oauth::is_connected(&google::GOOGLE) {
                return Ok(get_google_status());
            }
            match verify_google().await {
                Ok(()) => record_verification_success(
                    KEYRING_GOOGLE_LAST_VERIFIED_AT,
                    KEYRING_GOOGLE_LAST_ERROR,
                ),
                Err(reason) => record_verification_failure(
                    KEYRING_GOOGLE_LAST_VERIFIED_AT,
                    KEYRING_GOOGLE_LAST_ERROR,
                    &reason,
                ),
            }
            Ok(get_google_status())
        }
        "slack_oauth" => {
            if !oauth::is_connected(&slack::SLACK) {
                return Ok(get_slack_oauth_status());
            }
            match verify_slack_oauth().await {
                Ok(()) => record_verification_success(
                    KEYRING_SLACK_OAUTH_LAST_VERIFIED_AT,
                    KEYRING_SLACK_OAUTH_LAST_ERROR,
                ),
                Err(reason) => record_verification_failure(
                    KEYRING_SLACK_OAUTH_LAST_VERIFIED_AT,
                    KEYRING_SLACK_OAUTH_LAST_ERROR,
                    &reason,
                ),
            }
            Ok(get_slack_oauth_status())
        }
        other => Err(format!("Unknown integration: {}", other)),
    }
}

// ── Airtable write helpers (v0.9) ───────────────────────────────────────
//
// Returns are intentionally Result<(), String>: callers treat Airtable
// filing as best-effort. If the user is offline or the PAT is rotated,
// the workflow still completes locally — we just lose the cloud copy.

pub(crate) async fn airtable_create_record(
    table: &str,
    fields: serde_json::Value,
) -> Result<String, String> {
    let (api_key, base_id) = read_airtable_creds().ok_or("Airtable not configured")?;
    let url = format!("{}/{}/{}", AIRTABLE_API, base_id, table);
    let body = serde_json::json!({
        "records": [{ "fields": fields }],
        "typecast": true
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let resp = client
        .post(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Airtable post: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Airtable {}: {}", status.as_u16(), body));
    }
    let parsed: serde_json::Value = resp.json().await.map_err(|e| format!("Parse: {}", e))?;
    let record_id = parsed["records"][0]["id"]
        .as_str()
        .ok_or("Airtable response missing record id")?
        .to_string();
    Ok(record_id)
}

pub(crate) async fn airtable_update_record(
    table: &str,
    record_id: &str,
    fields: serde_json::Value,
) -> Result<(), String> {
    let (api_key, base_id) = read_airtable_creds().ok_or("Airtable not configured")?;
    let url = format!("{}/{}/{}/{}", AIRTABLE_API, base_id, table, record_id);
    let body = serde_json::json!({ "fields": fields });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let resp = client
        .patch(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Airtable patch: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Airtable {}: {}", status.as_u16(), body));
    }
    Ok(())
}

// Find a Clients record by code so Receipts.client can link to it.
// Returns None when the code is unknown to Airtable yet (e.g. brand-new
// receipt with a code Caitlin hasn't added) — caller files the receipt
// without a client link rather than failing.
async fn airtable_find_client_by_code(code: &str) -> Result<Option<String>, String> {
    if code.is_empty() {
        return Ok(None);
    }
    let escaped = code.replace('\'', "");
    let formula = format!("{{code}}='{}'", escaped);
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=code",
        urlencode(&formula)
    );
    let data = airtable_get("Clients", &qs).await?;
    Ok(data["records"][0]["id"].as_str().map(String::from))
}

pub(crate) fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

// File a receipt to the Airtable Receipts table. Best-effort: errors
// logged but never bubble up to fail the workflow.
//
// v0.29 (Block F foundation): after the row is created, fire two
// additive enrichments in sequence:
//   1. auto-link to a Project row when the receipt's `project` field is a
//      full project code (e.g. NCT-2026-06-tour-launch) that matches a
//      Projects.code in Airtable.
//   2. auto-summarise + auto-tag via Haiku 4.5, then PATCH the row with
//      the resulting `summary` and `tags`.
//
// Both enrichments are best-effort. The receipt is filed first; tagging
// and linking are layered on after. A failure in either step logs and
// returns silently — the row stays as-is.
async fn file_receipt_to_airtable(receipt_json: &str) {
    let parsed: serde_json::Value = match serde_json::from_str(receipt_json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("file_receipt_to_airtable: parse failed: {}", e);
            return;
        }
    };

    let id = parsed["id"].as_str().unwrap_or("");
    let workflow = parsed["workflow"].as_str().unwrap_or("strategic-thinking");
    let title = parsed["title"].as_str().unwrap_or("Receipt");
    let date = parsed["date"].as_str().unwrap_or("");

    // Receipt's `project` field is a project code like NCT-2026-06-tour-launch
    // or a bare client code like NCT. Pull the leading 3-4 letters as the
    // client code.
    let project_field = parsed["project"].as_str().unwrap_or("");
    let client_code = project_field
        .split('-')
        .next()
        .filter(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_uppercase()) && c.len() <= 4)
        .unwrap_or("INC")
        .to_string();

    // A "full" project code looks like CLIENT-YYYY-MM-slug (4+ hyphen-
    // separated parts). A bare client code like "NCT" or studio default
    // "in-cahoots-studio" doesn't qualify and we skip the project link.
    let project_code_candidate: Option<String> = {
        let segs: Vec<&str> = project_field.split('-').collect();
        if segs.len() >= 4
            && segs[0].chars().all(|ch| ch.is_ascii_uppercase())
            && segs[1].len() == 4
            && segs[1].chars().all(|ch| ch.is_ascii_digit())
        {
            Some(project_field.to_string())
        } else {
            None
        }
    };

    let ticked = count_ticked(&parsed);

    // Convert ISO-ish date string to YYYY-MM-DD if possible. Receipts in
    // Studio currently say "Saturday 04 May 2026" which Airtable's date
    // field rejects, so we send today's date in ISO form when the source
    // string isn't ISO already.
    let iso_date: String = if date.len() == 10 && date.chars().nth(4) == Some('-') {
        date.to_string()
    } else {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    };

    let mut fields = serde_json::json!({
        "id": id,
        "workflow": workflow,
        "title": title,
        "date": iso_date,
        "json": receipt_json,
        "ticked_count": ticked,
        "posted_to_slack": false,
    });

    // Link to the client row if we can find it.
    match airtable_find_client_by_code(&client_code).await {
        Ok(Some(client_record_id)) => {
            fields["client"] = serde_json::json!([client_record_id]);
        }
        Ok(None) => {
            eprintln!(
                "file_receipt_to_airtable: client code '{}' not found in Airtable",
                client_code
            );
        }
        Err(e) => {
            eprintln!("file_receipt_to_airtable: client lookup failed: {}", e);
        }
    }

    // Link to the Project row if the receipt's project field is a full
    // project code that matches an existing Projects.code.
    if let Some(ref pcode) = project_code_candidate {
        match airtable_find_project_by_code(pcode).await {
            Ok(Some(project_record_id)) => {
                fields["project"] = serde_json::json!([project_record_id]);
            }
            Ok(None) => {
                eprintln!(
                    "file_receipt_to_airtable: project code '{}' not found in Airtable",
                    pcode
                );
            }
            Err(e) => {
                eprintln!("file_receipt_to_airtable: project lookup failed: {}", e);
            }
        }
    }

    let receipt_record_id = match airtable_create_record("Receipts", fields).await {
        Ok(rid) => rid,
        Err(e) => {
            eprintln!("file_receipt_to_airtable: create failed: {}", e);
            return;
        }
    };

    // Fire the auto-tag + auto-summary enrichment. This is additive — the
    // row is already filed, so any failure here just leaves the row
    // un-summarised and un-tagged. Drift will pick it up later.
    if let Err(e) = enrich_receipt_with_haiku(&receipt_record_id, &parsed).await {
        eprintln!("file_receipt_to_airtable: enrichment failed: {}", e);
    }
}

// v0.29 — auto-tag + auto-summarise a freshly filed Receipt. Calls Haiku
// 4.5 with the receipt body and the canonical tag list, parses the JSON
// the model returns, then PATCHes the row with `summary` and `tags`.
async fn enrich_receipt_with_haiku(
    record_id: &str,
    receipt: &serde_json::Value,
) -> Result<(), String> {
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;

    // Compact body for the prompt: title, workflow, project, sections.
    let title = receipt["title"].as_str().unwrap_or("");
    let workflow = receipt["workflow"].as_str().unwrap_or("");
    let project = receipt["project"].as_str().unwrap_or("");
    let sections = receipt["sections"].clone();
    let position = receipt["position"].clone();

    let body_for_prompt = serde_json::json!({
        "title": title,
        "workflow": workflow,
        "project": project,
        "sections": sections,
        "position": position,
    })
    .to_string();

    let tag_list = RECEIPT_TAG_CHOICES.join(", ");

    let system = format!(
        "You read In Cahoots receipts and return tagging metadata.\n\n\
         You return ONLY a JSON object with two keys:\n\
         - summary: one line, plain English, max 200 chars, no preamble. \
         Australian spelling. Plain words (no 'lands', no 'the work', no \
         'the room'). Describe what the receipt records.\n\
         - tags: an array of 1 to 3 tags chosen ONLY from this fixed list: \
         {}.\n\n\
         Return the JSON object alone. No fenced block, no prose, no \
         comment. Example: {{\"summary\": \"Locked the new client \
         scope at $4,500 plus GST.\", \"tags\": [\"scope\", \"decision\"]}}",
        tag_list
    );

    let body = AnthropicRequest {
        model: HAIKU_MODEL_ID,
        max_tokens: 300,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: body_for_prompt,
        }],
        mcp_servers: None,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;

    let response = client
        .post(ANTHROPIC_API)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Haiku {}: {}", status.as_u16(), text));
    }

    let parsed_resp: AnthropicResponse =
        response.json().await.map_err(|e| format!("Parse: {}", e))?;
    let text = collect_text_blocks(&parsed_resp.content);

    // Haiku usually returns a bare JSON object, but extract_json_block
    // handles both a fenced block and a bare-braces variant.
    let json_str = extract_json_block(&text)?;
    let enrichment: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("Enrichment parse: {}", e))?;

    let mut summary = enrichment["summary"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if summary.chars().count() > 200 {
        summary = summary.chars().take(200).collect();
    }

    let tags: Vec<String> = enrichment["tags"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_lowercase())
                .filter(|s| RECEIPT_TAG_CHOICES.contains(&s.as_str()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if summary.is_empty() && tags.is_empty() {
        return Err("Haiku returned no usable enrichment".to_string());
    }

    let mut update_fields = serde_json::Map::new();
    if !summary.is_empty() {
        update_fields.insert("summary".to_string(), serde_json::Value::String(summary));
    }
    if !tags.is_empty() {
        update_fields.insert(
            "tags".to_string(),
            serde_json::Value::Array(tags.into_iter().map(serde_json::Value::String).collect()),
        );
    }

    airtable_update_record(
        "Receipts",
        record_id,
        serde_json::Value::Object(update_fields),
    )
    .await?;
    Ok(())
}

// v0.29 — stamp the autonomy level onto a receipt JSON string. Used by
// every workflow at the point we either parse Claude's response or build
// a pure-Airtable receipt. Receipts that already carry a level (e.g. a
// future workflow that nests a different level per section) are left
// alone; otherwise we add one at the envelope.
//
// L1 read-only inferences. L2 internal state changes. L3 internal comms.
// L4 drafts of external content. L5 external actions.
//
// Defaults across the existing workflows:
//   strategic-thinking, new-client-onboarding, new-campaign-scope,
//   subcontractor-onboarding, monthly-checkin, quarterly-review → L4
//   schedule-social-post, log-time, edit-project → L2
fn stamp_autonomy_level(receipt_json: &str, level: &str) -> String {
    let mut parsed: serde_json::Value = match serde_json::from_str(receipt_json) {
        Ok(v) => v,
        Err(_) => return receipt_json.to_string(),
    };
    if let Some(obj) = parsed.as_object_mut() {
        if !obj.contains_key("autonomy_level") {
            obj.insert(
                "autonomy_level".to_string(),
                serde_json::Value::String(level.to_string()),
            );
        }
    }
    serde_json::to_string(&parsed).unwrap_or_else(|_| receipt_json.to_string())
}

fn count_ticked(receipt: &serde_json::Value) -> u32 {
    let mut n = 0u32;
    if let Some(sections) = receipt["sections"].as_array() {
        for section in sections {
            if let Some(items) = section["items"].as_array() {
                for item in items {
                    if item["type"].as_str() == Some("task")
                        && item["done"].as_bool() == Some(true)
                    {
                        n += 1;
                    }
                }
            }
        }
    }
    n
}

// Look up a Receipts row by its Studio id (the rcpt_... primary key). Used
// when ticking an item so we update the corresponding Airtable row's
// ticked_count.
async fn airtable_find_receipt_record_id(receipt_id: &str) -> Result<Option<String>, String> {
    if receipt_id.is_empty() {
        return Ok(None);
    }
    let escaped = receipt_id.replace('\'', "");
    let formula = format!("{{id}}='{}'", escaped);
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=id",
        urlencode(&formula)
    );
    let data = airtable_get("Receipts", &qs).await?;
    Ok(data["records"][0]["id"].as_str().map(String::from))
}

pub(crate) async fn airtable_get(table: &str, query: &str) -> Result<serde_json::Value, String> {
    let (api_key, base_id) = read_airtable_creds().ok_or("Airtable not configured")?;
    let qs = if query.is_empty() {
        String::new()
    } else {
        format!("?{}", query)
    };
    let url = format!("{}/{}/{}{}", AIRTABLE_API, base_id, table, qs);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let resp = client
        .get(&url)
        .bearer_auth(&api_key)
        .send()
        .await
        .map_err(|e| format!("Network: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Airtable {}: {}", status.as_u16(), body));
    }
    resp.json().await.map_err(|e| format!("Parse: {}", e))
}

#[tauri::command]
async fn list_airtable_clients() -> Result<String, String> {
    // Returns active clients with the full set of fields the per-client view
    // header reads. Kept narrow (status='active') so the sidebar list stays
    // tidy. Per-client view filters and renders client-by-code from this list.
    let data = airtable_get(
        "Clients",
        "filterByFormula=%7Bstatus%7D%3D%27active%27\
&fields%5B%5D=code\
&fields%5B%5D=name\
&fields%5B%5D=status\
&fields%5B%5D=primary_contact_name\
&fields%5B%5D=primary_contact_email\
&fields%5B%5D=abn\
&fields%5B%5D=dropbox_folder\
&fields%5B%5D=gmail_thread_filter\
&fields%5B%5D=drive_folder_id\
&fields%5B%5D=notes",
    )
    .await?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_airtable_projects() -> Result<String, String> {
    // Returns non-archived projects with the full set of fields the per-client
    // Projects section reads (code, name, status, type, dates, budget, brief
    // link). Existing callers that only read code/name/status keep working.
    let data = airtable_get(
        "Projects",
        "filterByFormula=NOT(%7Bstatus%7D%3D%27archive%27)\
&fields%5B%5D=code\
&fields%5B%5D=name\
&fields%5B%5D=status\
&fields%5B%5D=client\
&fields%5B%5D=campaign_type\
&fields%5B%5D=start_date\
&fields%5B%5D=end_date\
&fields%5B%5D=budget_total\
&fields%5B%5D=brief_link\
&fields%5B%5D=notes",
    )
    .await?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

async fn post_to_slack(webhook: &str, message: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let body = serde_json::json!({ "text": message });
    let resp = client
        .post(webhook)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Slack post: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Slack returned {}", resp.status().as_u16()));
    }
    Ok(())
}

// ── Anthropic API call ─────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Serialize, Debug)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<AnthropicMessage>,
    // Optional MCP connector fields. When `mcp_servers` is non-empty the
    // request must also carry the `anthropic-beta: mcp-client-2025-11-20`
    // header. These serialise as nothing when None, keeping the existing
    // pure-prompt workflows byte-identical to v0.21.x.
    #[serde(skip_serializing_if = "Option::is_none")]
    mcp_servers: Option<Vec<McpServerSpec>>,
}

#[derive(Serialize, Debug)]
struct McpServerSpec {
    #[serde(rename = "type")]
    server_type: &'static str, // always "url"
    name: String,
    url: String,
    authorization_token: String,
    // Allowlist tools per call to keep token overhead down. The Messages
    // API connector takes a `tool_configuration` object with
    // `enabled: bool` and an `allowed_tools` array.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_configuration: Option<McpToolConfiguration>,
}

#[derive(Serialize, Debug)]
struct McpToolConfiguration {
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_tools: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)] // MCP fields captured for future debug surface
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
    // MCP tool-use / tool-result blocks. We don't render these in the
    // receipt itself — they're tool calls Claude made on the user's
    // behalf — but we keep them on the parsed response so a future
    // debug surface can expose them. The frontend doesn't see this
    // type today; only the joined text from `text` blocks reaches it.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<serde_json::Value>,
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    is_error: Option<bool>,
}

// Walks a content array and returns just the text. MCP tool-use and
// tool-result blocks are skipped here (see also the receipt builder
// which writes them into the `_mcp_log` field for debug).
fn collect_text_blocks(blocks: &[AnthropicContentBlock]) -> String {
    blocks
        .iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("")
}

// Note: MCP tool-use / tool-result blocks land on the response with
// types "mcp_tool_use" and "mcp_tool_result" (and the legacy
// "tool_use" / "tool_result" shapes). collect_text_blocks ignores
// them and returns just the assistant's text. The `name`, `input`,
// `content`, and `is_error` fields on AnthropicContentBlock are
// captured so a future debug surface can show what tool calls
// happened. They're deliberately not surfaced today — the user-
// visible response is just the assistant's prose.

fn extract_json_block(text: &str) -> Result<String, String> {
    // Try fenced ```json block first.
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return Ok(after[..end].trim().to_string());
        }
    }
    // Fallback: first '{' to last '}'.
    let first = text.find('{');
    let last = text.rfind('}');
    if let (Some(s), Some(e)) = (first, last) {
        if e > s {
            return Ok(text[s..=e].to_string());
        }
    }
    Err("Claude's response did not contain a JSON receipt".to_string())
}

#[tauri::command]
async fn run_strategic_thinking(
    input: String,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    if input.trim().is_empty() {
        return Err("Empty input".to_string());
    }

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: STRATEGIC_THINKING_PROMPT,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: input,
        }],
        mcp_servers: None,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client init: {}", e))?;

    let response = client
        .post(ANTHROPIC_API)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic API {}: {}", status.as_u16(), text));
    }

    let parsed: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let text = collect_text_blocks(&parsed.content);

    let json = stamp_autonomy_level(&extract_json_block(&text)?, "L4");

    // Persist to SQLite so it survives relaunches.
    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }

    // File to Airtable Receipts table. Best-effort — never fails the
    // workflow if Airtable is unreachable or unconfigured.
    file_receipt_to_airtable(&json).await;

    Ok(json)
}

// ── New Client Onboarding ──────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct NewClientOnboardingInput {
    client_name: String,
    contact_email: Option<String>,
    project_type: String,
    first_call_notes: String,
    budget_signal: Option<String>,
    timeline_signal: Option<String>,
}

#[tauri::command]
async fn run_new_client_onboarding(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;

    let parsed: NewClientOnboardingInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;

    if parsed.client_name.trim().is_empty() || parsed.first_call_notes.trim().is_empty() {
        return Err("Client name and first-call notes are required".to_string());
    }

    // Format the structured input into a single user message Claude can read.
    let user_message = format!(
        "## New client intake\n\n\
         **Client name:** {}\n\
         **Contact email:** {}\n\
         **Project type:** {}\n\
         **Budget signal:** {}\n\
         **Timeline signal:** {}\n\n\
         ## First-call notes\n\n{}\n\n\
         ## Today's date\n\n{}",
        parsed.client_name.trim(),
        parsed.contact_email.as_deref().unwrap_or("(not given)"),
        parsed.project_type.trim(),
        parsed.budget_signal.as_deref().unwrap_or("(not given)"),
        parsed.timeline_signal.as_deref().unwrap_or("(not given)"),
        parsed.first_call_notes.trim(),
        chrono::Local::now().format("%A %d %B %Y"),
    );

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: NEW_CLIENT_ONBOARDING_PROMPT,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client init: {}", e))?;

    let response = client
        .post(ANTHROPIC_API)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic API {}: {}", status.as_u16(), text));
    }

    let api_response: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let text = collect_text_blocks(&api_response.content);

    let json = stamp_autonomy_level(&extract_json_block(&text)?, "L4");

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }

    file_receipt_to_airtable(&json).await;

    Ok(json)
}

// ── Monthly Check-in ───────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct MonthlyCheckinInput {
    client_code: String,
    extra_notes: Option<String>,
    // v0.22: optional Granola transcript bundle. Set by the "Pull from
    // Granola" button on the modal (which calls `pull_granola_transcripts`
    // and concatenates the result into this field). Manual paste also
    // works — the user can edit the textarea before submitting. Plain
    // text; no shape contract.
    #[serde(default)]
    transcript_notes: Option<String>,
}

// Pull recent receipts from local SQLite for a given client code. Used as
// context for the Monthly Check-in. Filter by project field starting with
// the code (so NCT-2026-06-tour-launch and bare NCT both match).
fn recent_receipts_for_client(
    conn: &Connection,
    code: &str,
    days: i64,
) -> Result<Vec<String>, String> {
    let cutoff = chrono::Local::now().timestamp() - (days * 86400);
    let pattern = format!("{}%", code);
    let mut stmt = conn
        .prepare(
            "SELECT json FROM receipts \
             WHERE project LIKE ?1 AND created_at >= ?2 \
             ORDER BY created_at DESC LIMIT 50",
        )
        .map_err(|e| format!("DB prepare: {}", e))?;
    let rows: Vec<String> = stmt
        .query_map(params![pattern, cutoff], |row| row.get::<_, String>(0))
        .map_err(|e| format!("DB query: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// Compress a receipt to a small string Claude can read. Pulls title, date,
// and item-text-only content. Keeps the bundle small enough to fit in the
// context window for a multi-receipt prompt.
fn summarise_receipt(json: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let title = parsed["title"].as_str().unwrap_or("Receipt");
    let date = parsed["date"].as_str().unwrap_or("");
    let workflow = parsed["workflow"].as_str().unwrap_or("");
    let mut items: Vec<String> = Vec::new();
    if let Some(sections) = parsed["sections"].as_array() {
        for section in sections {
            let header = section["header"].as_str().unwrap_or("");
            if !header.is_empty() {
                items.push(format!("  [{}]", header));
            }
            if let Some(arr) = section["items"].as_array() {
                for item in arr {
                    let qty = item["qty"].as_str().unwrap_or("");
                    let text = item["text"].as_str().unwrap_or("");
                    let is_task = item["type"].as_str() == Some("task");
                    // `is_open` is true when the JSON `done` field is false
                    // (i.e. the task is still open / unticked). Keep the name
                    // explicit so a future reader doesn't "fix" this into a
                    // real inversion.
                    let is_open = item["done"].as_bool() == Some(false);
                    let prefix = if is_task && is_open {
                        "  ☐"
                    } else if is_task {
                        "  ☑"
                    } else {
                        "  -"
                    };
                    items.push(format!("{} {} {}", prefix, qty, text));
                }
            }
        }
    }
    format!(
        "### {} ({}, {})\n{}",
        title,
        workflow,
        date,
        items.join("\n")
    )
}

#[tauri::command]
async fn run_monthly_checkin(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;

    let parsed: MonthlyCheckinInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let code = parsed.client_code.trim().to_uppercase();
    if code.is_empty() {
        return Err("Pick a client first".to_string());
    }

    // Pull client metadata from Airtable.
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=code&fields%5B%5D=name&fields%5B%5D=status&fields%5B%5D=primary_contact_name&fields%5B%5D=primary_contact_email",
        urlencode(&format!("{{code}}='{}'", code))
    );
    let client_data = airtable_get("Clients", &qs).await.unwrap_or(serde_json::Value::Null);
    let client_record = &client_data["records"][0]["fields"];
    let client_name = client_record["name"].as_str().unwrap_or(&code);
    let client_status = client_record["status"].as_str().unwrap_or("active");

    // Pull recent receipts from local SQLite (last 30 days).
    let receipts: Vec<String> = {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        recent_receipts_for_client(&conn, &code, 30)?
    };
    let summaries: Vec<String> = receipts.iter().map(|j| summarise_receipt(j)).collect();
    let bundle = if summaries.is_empty() {
        "(No receipts in the last 30 days for this client.)".to_string()
    } else {
        summaries.join("\n\n")
    };

    let transcript_section = match parsed.transcript_notes.as_deref() {
        Some(t) if !t.trim().is_empty() => format!(
            "\n\n## Recent call transcripts / notes\n\n{}",
            t.trim()
        ),
        _ => String::new(),
    };

    let user_message = format!(
        "## Monthly check-in for {} ({})\n\n\
         **Status:** {}\n\
         **Today's date:** {}\n\
         **Window:** last 30 days\n\n\
         ## User flags from the studio side\n\n{}\n\n\
         ## Recent receipts\n\n{}{}",
        client_name,
        code,
        client_status,
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.extra_notes.as_deref().unwrap_or("(none)"),
        bundle,
        transcript_section,
    );

    // v0.31: monthly-checkin now runs on the richer SKILL.md prompt
    // (~/.claude/skills/anthropic-skills/monthly-checkin/SKILL.md), wrapped
    // with the shared Companion receipt envelope so the model still emits
    // a JSON receipt block alongside the markdown check-in doc.
    let system = skill_system_prompt(SKILL_MONTHLY_CHECKIN);

    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let receipt_hint = format!(
        "\n\n## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: {}\n\
         - workflow: monthly-checkin\n\
         - title: RECEIPT — MONTHLY CHECK-IN\n\
         - paid_block.customer: {}\n",
        receipt_id, code, client_name
    );
    let user_message = format!("{}{}", user_message, receipt_hint);

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;

    let response = http
        .post(ANTHROPIC_API)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic API {}: {}", status.as_u16(), text));
    }

    let api_response: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let text = collect_text_blocks(&api_response.content);

    let json = stamp_autonomy_level(&extract_json_block(&text)?, "L4");

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }

    file_receipt_to_airtable(&json).await;

    Ok(json)
}

// ── Quarterly Review ───────────────────────────────────────────────────
//
// Same shape as Monthly Check-in but with a 90-day window. Reuses the
// recent_receipts_for_client + summarise_receipt helpers.

#[derive(Deserialize, Debug)]
struct QuarterlyReviewInput {
    client_code: String,
    extra_notes: Option<String>,
    // v0.22: optional Granola transcript bundle. Same semantics as
    // Monthly Check-in's transcript_notes — populated by the "Pull from
    // Granola" button or manual paste.
    #[serde(default)]
    transcript_notes: Option<String>,
}

#[tauri::command]
async fn run_quarterly_review(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;

    let parsed: QuarterlyReviewInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let code = parsed.client_code.trim().to_uppercase();
    if code.is_empty() {
        return Err("Pick a client first".to_string());
    }

    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=code&fields%5B%5D=name&fields%5B%5D=status",
        urlencode(&format!("{{code}}='{}'", code))
    );
    let client_data = airtable_get("Clients", &qs).await.unwrap_or(serde_json::Value::Null);
    let client_record = &client_data["records"][0]["fields"];
    let client_name = client_record["name"].as_str().unwrap_or(&code);
    let client_status = client_record["status"].as_str().unwrap_or("active");

    let receipts: Vec<String> = {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        recent_receipts_for_client(&conn, &code, 90)?
    };
    let summaries: Vec<String> = receipts.iter().map(|j| summarise_receipt(j)).collect();
    let bundle = if summaries.is_empty() {
        "(No receipts in the last 90 days for this client.)".to_string()
    } else {
        summaries.join("\n\n")
    };

    let transcript_section = match parsed.transcript_notes.as_deref() {
        Some(t) if !t.trim().is_empty() => format!(
            "\n\n## Recent call transcripts / notes\n\n{}",
            t.trim()
        ),
        _ => String::new(),
    };

    let user_message = format!(
        "## Quarterly review for {} ({})\n\n\
         **Status:** {}\n\
         **Today's date:** {}\n\
         **Window:** last 90 days\n\n\
         ## User flags from the studio side\n\n{}\n\n\
         ## Recent receipts (last quarter)\n\n{}{}",
        client_name,
        code,
        client_status,
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.extra_notes.as_deref().unwrap_or("(none)"),
        bundle,
        transcript_section,
    );

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: QUARTERLY_REVIEW_PROMPT,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;

    let response = http
        .post(ANTHROPIC_API)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic API {}: {}", status.as_u16(), text));
    }

    let api_response: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let text = collect_text_blocks(&api_response.content);

    let json = stamp_autonomy_level(&extract_json_block(&text)?, "L4");

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }

    file_receipt_to_airtable(&json).await;

    Ok(json)
}

// ── New Campaign Scope ─────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct NewCampaignScopeInput {
    client_code: String,
    campaign_name: String,  // human-readable, e.g. "June tour launch"
    project_slug: String,   // kebab-case, e.g. "tour-launch"
    campaign_type: String,
    start_date: String,     // ISO YYYY-MM-DD
    end_date: Option<String>,
    budget_signal: Option<String>,
    brief_notes: String,
}

fn slug_from_string(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[tauri::command]
async fn run_new_campaign_scope(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;

    let parsed: NewCampaignScopeInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;

    let code = parsed.client_code.trim().to_uppercase();
    if code.is_empty() {
        return Err("Pick a client first".to_string());
    }
    if parsed.campaign_name.trim().is_empty() {
        return Err("Campaign name required".to_string());
    }
    if parsed.brief_notes.trim().is_empty() {
        return Err("Brief notes required".to_string());
    }

    // Compute project code: {CLIENT}-{YYYY}-{MM}-{slug}
    let slug = if parsed.project_slug.trim().is_empty() {
        slug_from_string(&parsed.campaign_name)
    } else {
        slug_from_string(&parsed.project_slug)
    };
    // Try to parse YYYY-MM from the start_date for the code.
    let (year, month): (String, String) = if parsed.start_date.len() >= 7
        && parsed.start_date.chars().nth(4) == Some('-')
    {
        (parsed.start_date[..4].to_string(), parsed.start_date[5..7].to_string())
    } else {
        let now = chrono::Local::now();
        (now.format("%Y").to_string(), now.format("%m").to_string())
    };
    let project_code = format!("{}-{}-{}-{}", code, year, month, slug);

    // Lookup client name from Airtable for the prompt context.
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=code&fields%5B%5D=name",
        urlencode(&format!("{{code}}='{}'", code))
    );
    let client_data = airtable_get("Clients", &qs).await.unwrap_or(serde_json::Value::Null);
    let client_name = client_data["records"][0]["fields"]["name"]
        .as_str()
        .unwrap_or(&code)
        .to_string();

    let user_message = format!(
        "## New campaign scope\n\n\
         **Client:** {} ({})\n\
         **Campaign name:** {}\n\
         **Project code:** {}\n\
         **Type:** {}\n\
         **Start:** {}\n\
         **End:** {}\n\
         **Budget signal:** {}\n\
         **Today's date:** {}\n\n\
         ## Brief notes\n\n{}",
        client_name,
        code,
        parsed.campaign_name.trim(),
        project_code,
        parsed.campaign_type.trim(),
        parsed.start_date.trim(),
        parsed.end_date.as_deref().unwrap_or("(not given)"),
        parsed.budget_signal.as_deref().unwrap_or("(not given)"),
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.brief_notes.trim(),
    );

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: NEW_CAMPAIGN_SCOPE_PROMPT,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;

    let response = http
        .post(ANTHROPIC_API)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic API {}: {}", status.as_u16(), text));
    }

    let api_response: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let text = collect_text_blocks(&api_response.content);

    let json = stamp_autonomy_level(&extract_json_block(&text)?, "L4");

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }

    file_receipt_to_airtable(&json).await;

    Ok(json)
}

// ── Airtable Projects write (used by airtable:create-project on_done) ──

#[derive(Deserialize, Debug)]
struct CreateProjectArgs {
    code: String,
    name: String,
    client_code: String,
    campaign_type: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    budget_total: Option<f64>,
    notes: Option<String>,
}

#[tauri::command]
async fn create_airtable_project(args: serde_json::Value) -> Result<String, String> {
    let parsed: CreateProjectArgs = serde_json::from_value(args)
        .map_err(|e| format!("Invalid args: {}", e))?;

    let project_code = parsed.code.trim();
    if project_code.is_empty() {
        return Err("Project code required".to_string());
    }
    let client_code = parsed.client_code.trim().to_uppercase();
    if client_code.is_empty() {
        return Err("Client code required".to_string());
    }

    // Find the Clients record so we can link to it.
    let client_record_id = airtable_find_client_by_code(&client_code)
        .await?
        .ok_or_else(|| format!("Client {} not in Airtable", client_code))?;

    let mut fields = serde_json::json!({
        "code": project_code,
        "name": parsed.name.trim(),
        "client": [client_record_id],
        "status": "scoping",
    });
    if let Some(t) = parsed.campaign_type.as_deref() {
        if !t.trim().is_empty() {
            fields["type"] = serde_json::Value::String(t.trim().to_string());
        }
    }
    if let Some(s) = parsed.start_date.as_deref() {
        if !s.trim().is_empty() {
            fields["start_date"] = serde_json::Value::String(s.trim().to_string());
        }
    }
    if let Some(e) = parsed.end_date.as_deref() {
        if !e.trim().is_empty() {
            fields["end_date"] = serde_json::Value::String(e.trim().to_string());
        }
    }
    if let Some(b) = parsed.budget_total {
        fields["budget_total"] = serde_json::json!(b);
    }
    if let Some(n) = parsed.notes.as_deref() {
        if !n.trim().is_empty() {
            fields["notes"] = serde_json::Value::String(n.trim().to_string());
        }
    }

    airtable_create_record("Projects", fields).await
}

// ── Subcontractor Onboarding ───────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct SubcontractorOnboardingInput {
    name: String,
    role: String,
    start_date: Option<String>,
    hourly_rate: Option<f64>,
    email: Option<String>,
    notes: String,
}

#[tauri::command]
async fn run_subcontractor_onboarding(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;

    let parsed: SubcontractorOnboardingInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    if parsed.name.trim().is_empty() {
        return Err("Subcontractor name required".to_string());
    }
    if parsed.role.trim().is_empty() {
        return Err("Role required".to_string());
    }
    if parsed.notes.trim().is_empty() {
        return Err("Notes required".to_string());
    }

    let user_message = format!(
        "## Subcontractor onboarding\n\n\
         **Name:** {}\n\
         **Role:** {}\n\
         **Start date:** {}\n\
         **Hourly rate:** {}\n\
         **Email:** {}\n\
         **Today's date:** {}\n\n\
         ## Notes\n\n{}",
        parsed.name.trim(),
        parsed.role.trim(),
        parsed.start_date.as_deref().unwrap_or("(not given)"),
        parsed.hourly_rate.map(|r| format!("${:.2}/hr", r)).unwrap_or_else(|| "(not given)".to_string()),
        parsed.email.as_deref().unwrap_or("(not given)"),
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.notes.trim(),
    );

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: SUBCONTRACTOR_ONBOARDING_PROMPT,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;

    let response = http
        .post(ANTHROPIC_API)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic API {}: {}", status.as_u16(), text));
    }

    let api_response: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let text = collect_text_blocks(&api_response.content);

    let json = stamp_autonomy_level(&extract_json_block(&text)?, "L4");

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }

    file_receipt_to_airtable(&json).await;

    Ok(json)
}

// ── v0.31 Block F — Skills batch 1 workflows ──────────────────────────
//
// Five workflows wired to ~/.claude/skills/anthropic-skills/<skill>/SKILL.md
// prompts (bundled into the binary at compile time). The shared receipt
// envelope lives in prompts/skills/_receipt-envelope.md and gets appended
// to every skill body, so the model produces (markdown content) +
// (fenced JSON receipt). All workflows file at L4 — drafts requiring
// Caitlin's sign-off before the external action.
//
// monthly-checkin was upgraded in place (see run_monthly_checkin above);
// the four below are new.

#[derive(Deserialize, Debug)]
struct NctCaptionInput {
    topic: String,
    reference_url: Option<String>,
    voice_override: Option<String>,
}

#[tauri::command]
async fn run_nct_caption(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;

    let parsed: NctCaptionInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let topic = parsed.topic.trim();
    if topic.is_empty() {
        return Err("Topic required".to_string());
    }

    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let voice = parsed
        .voice_override
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && *s != "default")
        .unwrap_or("default (venue voice)");
    let user_message = format!(
        "## Northcote Theatre social caption\n\n\
         **Today's date:** {}\n\
         **Topic / brief:** {}\n\
         **Reference URL:** {}\n\
         **Voice override:** {}\n\n\
         Produce three caption variants Caitlin can choose from. Each \
         variant should be a complete, post-ready caption following the \
         skill's structure. Number them 1, 2, 3. Keep variants \
         meaningfully different (e.g. one tighter, one warmer, one with \
         more context).\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: NCT\n\
         - workflow: nct-caption\n\
         - title: RECEIPT — NCT SOCIAL CAPTION\n\
         - paid_block.customer: Northcote Theatre\n\
         - sections[0].header: CAPTION VARIANTS\n\
         - For each of the 3 variants, include one item with `qty: \"1\"` \
           and `text` set to the FULL caption text (line breaks allowed).\n",
        chrono::Local::now().format("%A %d %B %Y"),
        topic,
        parsed.reference_url.as_deref().unwrap_or("(none)"),
        voice,
        receipt_id,
    );

    let system = skill_system_prompt(SKILL_NCT_CAPTION);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let json = call_anthropic_for_receipt(&key, &body, "L4").await?;

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;

    Ok(json)
}

#[derive(Deserialize, Debug)]
struct InCahootsSocialInput {
    topic: String,
    platform: String,
    pillar: String,
}

#[tauri::command]
async fn run_in_cahoots_social_post(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;

    let parsed: InCahootsSocialInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let topic = parsed.topic.trim();
    if topic.is_empty() {
        return Err("Topic required".to_string());
    }

    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let user_message = format!(
        "## In Cahoots social post draft\n\n\
         **Today's date:** {}\n\
         **Platform:** {}\n\
         **Content pillar:** {}\n\
         **Topic / observation:** {}\n\n\
         Produce a single ready-to-post draft for the platform above, \
         following the platform-specific guidance in the skill. Lead \
         with the observation, not a preamble. Sound like Caitlin would \
         say it out loud to a smart colleague.\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: INC\n\
         - workflow: in-cahoots-social-post\n\
         - title: RECEIPT — IN CAHOOTS SOCIAL POST\n\
         - paid_block.customer: In Cahoots\n\
         - sections[0].header: DRAFT POST\n\
         - The first item in sections[0] must be `{{\"qty\": \"1\", \
           \"text\": \"<the full draft post text, line breaks allowed>\"}}`.\n",
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.platform.trim(),
        parsed.pillar.trim(),
        topic,
        receipt_id,
    );

    let system = skill_system_prompt(SKILL_IN_CAHOOTS_SOCIAL);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let json = call_anthropic_for_receipt(&key, &body, "L4").await?;

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;

    Ok(json)
}

#[derive(Deserialize, Debug)]
struct CampaignWrapReportInput {
    project_code: String,
    extra_notes: Option<String>,
}

#[tauri::command]
async fn run_campaign_wrap_report(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;

    let parsed: CampaignWrapReportInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let project_code = parsed.project_code.trim().to_string();
    if project_code.is_empty() {
        return Err("Pick a project first".to_string());
    }

    // Pull project metadata from Airtable.
    let qs = format!(
        "filterByFormula={}&maxRecords=1\
&fields%5B%5D=code\
&fields%5B%5D=name\
&fields%5B%5D=status\
&fields%5B%5D=campaign_type\
&fields%5B%5D=start_date\
&fields%5B%5D=end_date\
&fields%5B%5D=budget_total\
&fields%5B%5D=notes\
&fields%5B%5D=client",
        urlencode(&format!("{{code}}='{}'", project_code))
    );
    let project_data = airtable_get("Projects", &qs)
        .await
        .unwrap_or(serde_json::Value::Null);
    let project_record = &project_data["records"][0]["fields"];
    let project_name = project_record["name"]
        .as_str()
        .unwrap_or(&project_code)
        .to_string();
    let campaign_type = project_record["campaign_type"]
        .as_str()
        .unwrap_or("(unspecified)")
        .to_string();
    let start_date = project_record["start_date"]
        .as_str()
        .unwrap_or("(unknown)")
        .to_string();
    let end_date = project_record["end_date"]
        .as_str()
        .unwrap_or("(unknown)")
        .to_string();
    let budget_total = project_record["budget_total"]
        .as_f64()
        .map(|n| format!("${:.0}", n))
        .unwrap_or_else(|| "(unspecified)".to_string());

    // Pull recent receipts for this project (last 365 days — campaigns
    // run long).
    let receipts: Vec<String> = {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        recent_receipts_for_client(&conn, &project_code, 365)?
    };
    let summaries: Vec<String> =
        receipts.iter().map(|j| summarise_receipt(j)).collect();
    let bundle = if summaries.is_empty() {
        "(No receipts on file for this project.)".to_string()
    } else {
        summaries.join("\n\n")
    };

    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let user_message = format!(
        "## Campaign wrap report\n\n\
         **Project:** {} ({})\n\
         **Campaign type:** {}\n\
         **Run dates:** {} → {}\n\
         **Marketing budget:** {}\n\
         **Today's date:** {}\n\n\
         ## User flags / notes\n\n{}\n\n\
         ## Receipts on file (newest first)\n\n{}\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: {}\n\
         - workflow: campaign-wrap-report\n\
         - title: RECEIPT — CAMPAIGN WRAP REPORT\n\
         - paid_block.customer: {}\n\
         - The markdown wrap report (above the JSON block) is the body \
           Caitlin will save to Dropbox and adapt for the client. Use \
           the full skill structure (Campaign Overview, Ticket Sales, \
           Channel Performance, Paid Media, What Worked, What Didn't, \
           Key Learnings, Recommendations).\n",
        project_name,
        project_code,
        campaign_type,
        start_date,
        end_date,
        budget_total,
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.extra_notes.as_deref().unwrap_or("(none)"),
        bundle,
        receipt_id,
        project_code,
        project_name,
    );

    let system = skill_system_prompt(SKILL_CAMPAIGN_WRAP_REPORT);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 8192,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    // Need both the markdown body and the JSON receipt. The markdown is
    // everything before the fenced ```json block.
    let raw = call_anthropic_raw_text(&key, &body).await?;
    let json = stamp_autonomy_level(&extract_json_block(&raw)?, "L4");
    let markdown_body = strip_json_block(&raw);

    // Write the wrap markdown to Dropbox for client delivery.
    let wrap_path = wrap_report_path(&project_code);
    if let Err(e) = std::fs::write(&wrap_path, markdown_body.as_bytes()) {
        eprintln!(
            "run_campaign_wrap_report: failed to write {}: {}",
            wrap_path, e
        );
    }

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;

    Ok(json)
}

fn wrap_report_path(project_code: &str) -> String {
    format!(
        "{}/Library/CloudStorage/Dropbox/IN CAHOOTS/08 CAMPAIGN SNAPSHOTS/{}-wrap.md",
        std::env::var("HOME").unwrap_or_default(),
        project_code
    )
}

// ── Campaign Launch Checklist (v0.35) ──────────────────────────────────
//
// Deterministic L3 workflow. The frontend sends the tick state per item,
// we build a Receipt envelope from the SOP's six phases, persist it,
// file to Airtable, and post a "Launch ready" summary to #client-work.
// No Anthropic call. The Meta Ads Manager publish click stays manual at
// L5 — the receipt notes the launch is ready, Caitlin clicks Publish in
// the third-party tool herself.

#[derive(Deserialize, Debug)]
struct CampaignLaunchChecklistItem {
    text: String,
    #[serde(default)]
    ticked: bool,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Deserialize, Debug)]
struct CampaignLaunchChecklistPhase {
    title: String,
    items: Vec<CampaignLaunchChecklistItem>,
}

#[derive(Deserialize, Debug)]
struct CampaignLaunchChecklistInput {
    project_code: String,
    phases: Vec<CampaignLaunchChecklistPhase>,
    extra_notes: Option<String>,
}

#[tauri::command]
async fn run_campaign_launch_checklist(
    input: serde_json::Value,
    state: State<'_, DbState>,
) -> Result<String, String> {
    let parsed: CampaignLaunchChecklistInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let project_code = parsed.project_code.trim().to_string();
    if project_code.is_empty() {
        return Err("Pick a project first".to_string());
    }
    if parsed.phases.is_empty() {
        return Err("No phases supplied".to_string());
    }

    // Pull project metadata so the receipt's paid_block.customer is the
    // client's actual name, not the code.
    let qs = format!(
        "filterByFormula={}&maxRecords=1\
&fields%5B%5D=code\
&fields%5B%5D=name\
&fields%5B%5D=status\
&fields%5B%5D=campaign_type\
&fields%5B%5D=client",
        urlencode(&format!("{{code}}='{}'", project_code))
    );
    let project_data = airtable_get("Projects", &qs)
        .await
        .unwrap_or(serde_json::Value::Null);
    let project_record = &project_data["records"][0]["fields"];
    let project_name = project_record["name"]
        .as_str()
        .unwrap_or(&project_code)
        .to_string();
    let campaign_type = project_record["campaign_type"]
        .as_str()
        .unwrap_or("campaign")
        .to_string();

    // Tally totals.
    let total_items: usize = parsed.phases.iter().map(|p| p.items.len()).sum();
    let ticked_items: usize = parsed
        .phases
        .iter()
        .map(|p| p.items.iter().filter(|i| i.ticked).count())
        .sum();
    let phases_done: usize = parsed
        .phases
        .iter()
        .filter(|p| !p.items.is_empty() && p.items.iter().all(|i| i.ticked))
        .count();
    let total_phases = parsed.phases.len();
    let launch_ready = ticked_items == total_items;

    // Build receipt sections. Each phase becomes a section with task-type
    // items so Caitlin can keep ticking after filing if she wants to.
    let mut sections: Vec<serde_json::Value> = Vec::new();
    for (idx, phase) in parsed.phases.iter().enumerate() {
        let header = format!("{} — {}", idx + 1, phase.title.to_uppercase());
        let mut items: Vec<serde_json::Value> = Vec::new();
        for item in &phase.items {
            let note_suffix = match item.note.as_deref().map(|s| s.trim()) {
                Some(n) if !n.is_empty() => format!(" — {}", n),
                _ => String::new(),
            };
            let text = format!("{}{}", item.text, note_suffix);
            items.push(serde_json::json!({
                "type": "task",
                "text": text,
                "done": item.ticked,
            }));
        }
        sections.push(serde_json::json!({
            "header": header,
            "items": items,
        }));
    }

    // Action items: file the launch summary, schedule the day-1 check.
    // The Slack post on the action item ticks via the existing on_done
    // hook so a re-post can fire from the receipt later if needed.
    let extra_notes_clean = parsed
        .extra_notes
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(notes) = &extra_notes_clean {
        sections.push(serde_json::json!({
            "header": "EXTRA NOTES",
            "items": [
                { "qty": "·", "text": notes }
            ],
        }));
    }
    sections.push(serde_json::json!({
        "header": "ACTION ITEMS",
        "items": [
            {
                "type": "task",
                "text": "Click Publish in Meta Ads Manager",
                "done": false,
            },
            {
                "type": "task",
                "text": "Repost launch summary to #client-work",
                "done": false,
                "on_done": "slack:#client-work",
            },
            {
                "type": "task",
                "text": "Schedule first 24-hour performance check",
                "done": false,
                "on_done": "calendar:wip",
            }
        ],
    }));

    let position_quote = if launch_ready {
        "Launch ready. Hit Publish in Ads Manager."
    } else {
        "Not launch-ready yet. Outstanding items above."
    };
    let status_line = if launch_ready {
        "launch ready".to_string()
    } else {
        format!("{} of {} items outstanding", total_items - ticked_items, total_items)
    };

    let id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let receipt = serde_json::json!({
        "id": id,
        "project": project_code,
        "workflow": "campaign-launch-checklist",
        "title": "RECEIPT — CAMPAIGN LAUNCH CHECKLIST",
        "date": chrono::Local::now().format("%A %d %B %Y").to_string(),
        "sections": sections,
        "position": {
            "header": "LAUNCH READY",
            "quote": position_quote,
        },
        "totals": [
            { "label": "TICKED", "value": ticked_items.to_string() },
            { "label": "ITEMS", "value": total_items.to_string() },
            { "label": "PHASES DONE", "value": format!("{}/{}", phases_done, total_phases), "grand": true }
        ],
        "paid_block": {
            "stamp": if launch_ready { "LAUNCH READY" } else { "PRE-LAUNCH" },
            "method": "six-phase pre-launch SOP",
            "issued_by": "the studio",
            "customer": project_name,
            "status": status_line,
        },
        "footer_note": "Run it every time. Non-negotiable.",
    });

    let json_raw = serde_json::to_string(&receipt)
        .map_err(|e| format!("Serialise receipt: {}", e))?;
    let json = stamp_autonomy_level(&json_raw, "L3");

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;

    // Best-effort Slack post to #client-work. Doesn't fail the workflow
    // if Slack isn't configured.
    if let Some(webhook) = read_slack_webhook() {
        let mut summary = String::new();
        if launch_ready {
            summary.push_str(&format!(
                ":rocket: *Launch ready: {} ({})*\n",
                project_name, project_code
            ));
        } else {
            summary.push_str(&format!(
                ":construction: *Pre-launch: {} ({})*\n",
                project_name, project_code
            ));
        }
        summary.push_str(&format!(
            "{} of {} items ticked across {} phases ({}/{} fully done).\n",
            ticked_items, total_items, total_phases, phases_done, total_phases
        ));
        summary.push_str(&format!("Type: {}.\n", campaign_type));
        if let Some(notes) = &extra_notes_clean {
            summary.push_str(&format!("Notes: {}\n", notes));
        }
        if launch_ready {
            summary.push_str("Next: hit Publish in Ads Manager.");
        } else {
            summary.push_str("Outstanding items in the receipt.");
        }
        if let Err(e) = post_to_slack(&webhook, &summary).await {
            eprintln!("run_campaign_launch_checklist: Slack post failed: {}", e);
        }
    }

    Ok(json)
}

// Strip the trailing fenced ```json ... ``` block from a raw model
// response, returning whatever sits above it. Used for skill workflows
// that need both the human-readable markdown and the receipt JSON.
fn strip_json_block(raw: &str) -> String {
    if let Some(idx) = raw.find("```json") {
        return raw[..idx].trim_end().to_string();
    }
    if let Some(idx) = raw.rfind("```") {
        // Fallback: strip last fenced block of any kind.
        if let Some(open) = raw[..idx].rfind("```") {
            return raw[..open].trim_end().to_string();
        }
    }
    raw.trim_end().to_string()
}

#[derive(Deserialize, Debug)]
struct ScopeOfWorkInput {
    client_code: Option<String>,
    new_client_name: Option<String>,
    project_type: String,
    length: Option<String>,
    deliverables: String,
    budget_range: Option<String>,
}

#[tauri::command]
async fn run_scope_of_work(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;

    let parsed: ScopeOfWorkInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;

    if parsed.deliverables.trim().is_empty() {
        return Err("Deliverables required".to_string());
    }

    // Resolve client identity. Either an existing client code or a free
    // text "new client" name (lead pre-Client).
    let (client_code, client_name) = match parsed.client_code.as_deref() {
        Some(code) if !code.trim().is_empty() => {
            let upper = code.trim().to_uppercase();
            // Look up name from Airtable.
            let qs = format!(
                "filterByFormula={}&maxRecords=1&fields%5B%5D=code&fields%5B%5D=name",
                urlencode(&format!("{{code}}='{}'", upper))
            );
            let data = airtable_get("Clients", &qs)
                .await
                .unwrap_or(serde_json::Value::Null);
            let name = data["records"][0]["fields"]["name"]
                .as_str()
                .unwrap_or(&upper)
                .to_string();
            (upper, name)
        }
        _ => {
            let name = parsed
                .new_client_name
                .as_deref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or("Pick a client or enter a new client name")?;
            ("LEAD".to_string(), name)
        }
    };

    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let date_slug = chrono::Local::now().format("%Y-%m-%d").to_string();
    let user_message = format!(
        "## Scope of Work\n\n\
         **Client:** {} ({})\n\
         **Project type:** {}\n\
         **Engagement length:** {}\n\
         **Budget range:** {}\n\
         **Today's date:** {}\n\n\
         ## Deliverables / brief notes\n\n{}\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: {}\n\
         - workflow: scope-of-work\n\
         - title: RECEIPT — SCOPE OF WORK\n\
         - paid_block.customer: {}\n\
         - The markdown SOW (above the JSON block) is what gets saved \
           to Dropbox for typst rendering and CSA generation. Follow \
           the full skill structure (Project Overview, Objectives, \
           Scope of Work, Out of Scope, Deliverables and Timeline, \
           Client Responsibilities, Fees, Approval and Revisions, \
           Start Date and Duration). Use Caitlin's voice — clear, \
           anti-corporate, peer-to-peer.\n",
        client_name,
        client_code,
        parsed.project_type.trim(),
        parsed.length.as_deref().unwrap_or("(unspecified)"),
        parsed.budget_range.as_deref().unwrap_or("(unspecified)"),
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.deliverables.trim(),
        receipt_id,
        client_code,
        client_name,
    );

    let system = skill_system_prompt(SKILL_SCOPE_OF_WORK_BUILDER);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 8192,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let raw = call_anthropic_raw_text(&key, &body).await?;
    let json = stamp_autonomy_level(&extract_json_block(&raw)?, "L4");
    let markdown_body = strip_json_block(&raw);

    // Save SOW JSON envelope (markdown body + metadata) for typst
    // rendering. v0.33 will pick this up to generate the CSA.
    let scope_path = scope_payload_path(&client_code, &date_slug);
    let payload = serde_json::json!({
        "client_code": client_code,
        "client_name": client_name,
        "project_type": parsed.project_type.trim(),
        "length": parsed.length,
        "budget_range": parsed.budget_range,
        "deliverables_brief": parsed.deliverables.trim(),
        "scope_markdown": markdown_body,
        "generated_at": chrono::Local::now().to_rfc3339(),
        "receipt_id": receipt_id,
    });
    if let Some(parent) = std::path::Path::new(&scope_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&scope_path, payload.to_string().as_bytes()) {
        eprintln!(
            "run_scope_of_work: failed to write {}: {}",
            scope_path, e
        );
    }

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;

    Ok(json)
}

fn scope_payload_path(client_code: &str, date_slug: &str) -> String {
    format!(
        "{}/Library/CloudStorage/Dropbox/IN CAHOOTS/05 TEMPLATES/RECEIPT-DOCS/typst/data/scopes/{}-{}.json",
        std::env::var("HOME").unwrap_or_default(),
        client_code.to_lowercase(),
        date_slug
    )
}

// ── v0.32 Block F — Skills batch 2 workflows ──────────────────────────
//
// Seven new workflows wired from skill SKILL.md files. All accept a
// `context_blob` from the source-picker pattern (string returned by
// fetch_workflow_context). The blob is prepended to the user message so
// the model has the original brief verbatim. All are L4 autonomy except
// hook-generator and the two edit-pass skills which are L2 (intermediate
// — Caitlin reviews each output before pasting back into the source
// surface).

#[derive(Deserialize, Debug)]
struct PressReleaseInput {
    #[serde(default)]
    client_code: Option<String>,
    #[serde(default)]
    context_blob: Option<String>,
    angle: String,
    #[serde(default)]
    extra_notes: Option<String>,
}

#[tauri::command]
async fn run_press_release(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    let parsed: PressReleaseInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let angle = parsed.angle.trim();
    if angle.is_empty() {
        return Err("Angle / topic required".to_string());
    }
    let project = parsed
        .client_code
        .as_deref()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "INC".to_string());

    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let user_message = format!(
        "## Press release brief\n\n\
         **Today's date:** {}\n\
         **Project / client code:** {}\n\
         **Angle:** {}\n\n\
         {}\n\n\
         ## Extra notes\n\n{}\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: {}\n\
         - workflow: press-release\n\
         - title: RECEIPT — PRESS RELEASE DRAFT\n\
         - paid_block.customer: {}\n\
         - sections[0].header: PRESS RELEASE\n\
         - The first item in sections[0] should be the full press \
           release as a single `qty: \"✓\"` item (or split paragraphs \
           across items if cleaner).\n",
        chrono::Local::now().format("%A %d %B %Y"),
        project,
        angle,
        parsed.context_blob.as_deref().unwrap_or("(no source)"),
        parsed.extra_notes.as_deref().unwrap_or("(none)"),
        receipt_id,
        project,
        project,
    );

    let system = skill_system_prompt(SKILL_PRESS_RELEASE_WRITER);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let json = call_anthropic_for_receipt(&key, &body, "L4").await?;
    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;
    Ok(json)
}

#[derive(Deserialize, Debug)]
struct EdmWriterInput {
    #[serde(default)]
    client_code: Option<String>,
    #[serde(default)]
    context_blob: Option<String>,
    purpose: String,
    #[serde(default)]
    audience: Option<String>,
}

#[tauri::command]
async fn run_edm_writer(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    let parsed: EdmWriterInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    if parsed.purpose.trim().is_empty() {
        return Err("EDM purpose required".to_string());
    }
    let project = parsed
        .client_code
        .as_deref()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "INC".to_string());

    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let user_message = format!(
        "## EDM brief\n\n\
         **Today's date:** {}\n\
         **Project / client code:** {}\n\
         **Purpose:** {}\n\
         **Audience:** {}\n\n\
         {}\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: {}\n\
         - workflow: edm-writer\n\
         - title: RECEIPT — EDM DRAFT\n\
         - paid_block.customer: {}\n\
         - sections[0].header: EDM DRAFT\n\
         - First item: subject line variants (qty: \"1\" each).\n\
         - Second item: full EDM body with line breaks.\n",
        chrono::Local::now().format("%A %d %B %Y"),
        project,
        parsed.purpose.trim(),
        parsed.audience.as_deref().unwrap_or("(unspecified)"),
        parsed.context_blob.as_deref().unwrap_or("(no source)"),
        receipt_id,
        project,
        project,
    );

    let system = skill_system_prompt(SKILL_EDM_WRITER);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let json = call_anthropic_for_receipt(&key, &body, "L4").await?;
    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;
    Ok(json)
}

// ── v0.39 Block F — Draft dossier (multi-source) ──────────────────────
//
// Reuses the source-picker (Granola, Gmail, Slack, Calendar, Form,
// manual) to pull a brief blob, then runs the client-onboarding skill
// against the In Cahoots 12-section dossier template. Saves the
// markdown body to `Dropbox/IN CAHOOTS/03 CLIENT DOSSIERS/{slug}.md`.
// If a dossier already exists at that path, writes to
// `{slug}-draft-{YYYY-MM-DD}.md` instead so Caitlin can manually merge.
//
// L4 autonomy: Caitlin reviews the draft before publishing.

#[derive(Deserialize, Debug)]
struct DraftDossierInput {
    // For an existing client: pass the code only.
    #[serde(default)]
    client_code: Option<String>,
    // For a brand new lead with no Airtable record yet, pass the name +
    // optional explicit slug. The frontend collects these in "new lead"
    // mode of the modal.
    #[serde(default)]
    new_client_name: Option<String>,
    #[serde(default)]
    new_client_slug: Option<String>,
    // Source-picker output (already wrapped by fetch_workflow_context).
    #[serde(default)]
    context_blob: Option<String>,
    // Optional extras to weave into the dossier draft.
    #[serde(default)]
    extra_notes: Option<String>,
}

fn dossier_dir() -> String {
    format!(
        "{}/Library/CloudStorage/Dropbox/IN CAHOOTS/03 CLIENT DOSSIERS",
        std::env::var("HOME").unwrap_or_default()
    )
}

fn dossier_path(slug: &str) -> String {
    format!("{}/{}.md", dossier_dir(), slug)
}

fn dossier_draft_path(slug: &str) -> String {
    let date = chrono::Local::now().format("%Y-%m-%d");
    format!("{}/{}-draft-{}.md", dossier_dir(), slug, date)
}

// Resolve the client's friendly name from Airtable for the modal flow
// where only the code was passed.
async fn lookup_client_name_by_code(code: &str) -> Option<String> {
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=name",
        urlencode(&format!("{{code}}='{}'", code))
    );
    let data = airtable_get("Clients", &qs).await.ok()?;
    data["records"][0]["fields"]["name"]
        .as_str()
        .map(|s| s.to_string())
}

#[tauri::command]
async fn run_draft_dossier(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    let parsed: DraftDossierInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;

    // Resolve client identity. Existing client → code + name from
    // Airtable. New lead → name + slug supplied by the frontend.
    let (client_code, client_name, slug) = if let Some(code) = parsed
        .client_code
        .as_deref()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
    {
        let name = lookup_client_name_by_code(&code).await.unwrap_or_else(|| code.clone());
        let slug = slugify_client_name(&name);
        (code, name, slug)
    } else {
        let name = parsed
            .new_client_name
            .as_deref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or("Either client_code or new_client_name is required")?;
        let slug = parsed
            .new_client_slug
            .as_deref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| slugify_client_name(&name));
        // No Airtable code yet — use the slug uppercased as a placeholder
        // for the receipt project tag so the receipts log groups it
        // sensibly.
        let placeholder = slug.to_uppercase();
        (placeholder, name, slug)
    };

    if slug.is_empty() {
        return Err("Could not derive a client slug — pass new_client_slug".to_string());
    }

    // Decide write path now so the prompt can hint the right filename.
    let final_path = dossier_path(&slug);
    let collision = std::path::Path::new(&final_path).exists();
    let write_path = if collision {
        dossier_draft_path(&slug)
    } else {
        final_path.clone()
    };

    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    let user_message = format!(
        "## Draft a client dossier\n\n\
         **Today's date:** {today}\n\
         **Client name:** {name}\n\
         **Client slug:** {slug}\n\
         **Client code (for receipt project tag):** {code}\n\
         **Existing dossier on disk:** {collision_note}\n\
         **Write path:** {write_path}\n\n\
         ## Source\n\n{source}\n\n\
         ## Extra notes\n\n{notes}\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {receipt_id}\n\
         - project: {code}\n\
         - workflow: draft-dossier\n\
         - title: RECEIPT — DRAFT DOSSIER\n\
         - paid_block.customer: {name}\n\
         - sections[0].header: DOSSIER DRAFT\n\
         - First items (qty: \"✓\") name the sections you filled.\n\
         - Then items (qty: \"1\") naming sections left as TBC.\n\
         - Add `type: task, done: false` items for the open follow-ups \
           Caitlin needs to chase to fill the gaps.\n\
         - Output the full 12-section dossier markdown above the JSON \
           block. Companion writes that markdown to: {write_path}\n",
        today = chrono::Local::now().format("%A %d %B %Y"),
        name = client_name,
        slug = slug,
        code = client_code,
        collision_note = if collision {
            "Yes — write a draft alongside, do not overwrite."
        } else {
            "No — write a fresh dossier."
        },
        write_path = write_path,
        source = parsed.context_blob.as_deref().unwrap_or("(no source provided)"),
        notes = parsed.extra_notes.as_deref().unwrap_or("(none)"),
        receipt_id = receipt_id,
    );

    let system = skill_system_prompt(SKILL_CLIENT_ONBOARDING);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 8192,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let raw = call_anthropic_raw_text(&key, &body).await?;
    let json = stamp_autonomy_level(&extract_json_block(&raw)?, "L4");
    let markdown_body = strip_json_block(&raw);

    // Make sure the dossier directory exists (it does today, but a
    // fresh install or a renamed Dropbox folder shouldn't error out).
    if let Err(e) = std::fs::create_dir_all(dossier_dir()) {
        eprintln!("run_draft_dossier: create_dir_all failed: {}", e);
    }
    if let Err(e) = std::fs::write(&write_path, markdown_body.as_bytes()) {
        eprintln!(
            "run_draft_dossier: failed to write {}: {}",
            write_path, e
        );
        return Err(format!("Could not write dossier to {}: {}", write_path, e));
    }

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;

    Ok(json)
}

#[derive(Deserialize, Debug)]
struct ReelsScriptingInput {
    #[serde(default)]
    client_code: Option<String>,
    #[serde(default)]
    context_blob: Option<String>,
    topic: String,
    #[serde(default)]
    reference_url: Option<String>,
}

#[tauri::command]
async fn run_reels_scripting(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    let parsed: ReelsScriptingInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    if parsed.topic.trim().is_empty() {
        return Err("Reel topic required".to_string());
    }
    let project = parsed
        .client_code
        .as_deref()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "INC".to_string());

    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let user_message = format!(
        "## Reel script brief\n\n\
         **Today's date:** {}\n\
         **Project / client code:** {}\n\
         **Topic:** {}\n\
         **Reference reel URL:** {}\n\n\
         {}\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: {}\n\
         - workflow: reels-scripting\n\
         - title: RECEIPT — REEL SCRIPT\n\
         - paid_block.customer: {}\n\
         - sections[0].header: REEL SCRIPT\n",
        chrono::Local::now().format("%A %d %B %Y"),
        project,
        parsed.topic.trim(),
        parsed.reference_url.as_deref().unwrap_or("(none)"),
        parsed.context_blob.as_deref().unwrap_or("(no source)"),
        receipt_id,
        project,
        project,
    );

    let system = skill_system_prompt(SKILL_REELS_SCRIPTING);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let json = call_anthropic_for_receipt(&key, &body, "L4").await?;
    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;
    Ok(json)
}

#[derive(Deserialize, Debug)]
struct HookGeneratorInput {
    topic: String,
}

#[tauri::command]
async fn run_hook_generator(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    let parsed: HookGeneratorInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    if parsed.topic.trim().is_empty() {
        return Err("Hook topic required".to_string());
    }
    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let user_message = format!(
        "## Hook generation brief\n\n\
         **Today's date:** {}\n\
         **Topic:** {}\n\n\
         Produce six two-line hook variants per the skill structure. Number \
         them 1-6. Keep each line within 40 characters.\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: INC\n\
         - workflow: hook-generator\n\
         - title: RECEIPT — HOOK VARIANTS\n\
         - paid_block.customer: In Cahoots\n\
         - sections[0].header: HOOKS\n\
         - Each variant goes in as one item with `qty: \"1\"` and the full \
           two-line hook as `text` (line break separates them).\n",
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.topic.trim(),
        receipt_id,
    );

    let system = skill_system_prompt(SKILL_HOOK_GENERATOR);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 2048,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    // L2 — intermediate / inline tool. Caitlin picks one variant before
    // it goes back into the social composer.
    let json = call_anthropic_for_receipt(&key, &body, "L2").await?;
    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;
    Ok(json)
}

#[derive(Deserialize, Debug)]
struct ClientEmailInput {
    client_code: String,
    #[serde(default)]
    context_blob: Option<String>,
    purpose: String,
    #[serde(default)]
    extra_notes: Option<String>,
}

#[tauri::command]
async fn run_client_email(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    let parsed: ClientEmailInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let code = parsed.client_code.trim().to_uppercase();
    if code.is_empty() {
        return Err("Pick a client first".to_string());
    }
    if parsed.purpose.trim().is_empty() {
        return Err("Email purpose required".to_string());
    }

    // Resolve client name for the email recipient framing.
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=name&fields%5B%5D=primary_contact_name&fields%5B%5D=primary_contact_email",
        urlencode(&format!("{{code}}='{}'", code))
    );
    let data = airtable_get("Clients", &qs)
        .await
        .unwrap_or(serde_json::Value::Null);
    let client_name = data["records"][0]["fields"]["name"]
        .as_str()
        .unwrap_or(&code)
        .to_string();
    let contact_name = data["records"][0]["fields"]["primary_contact_name"]
        .as_str()
        .unwrap_or("(unknown)")
        .to_string();

    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let user_message = format!(
        "## Client email brief\n\n\
         **Today's date:** {}\n\
         **Client:** {} ({})\n\
         **Primary contact:** {}\n\
         **Purpose:** {}\n\n\
         {}\n\n\
         ## Extra notes\n\n{}\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: {}\n\
         - workflow: client-email\n\
         - title: RECEIPT — CLIENT EMAIL DRAFT\n\
         - paid_block.customer: {}\n\
         - sections[0].header: EMAIL DRAFT\n\
         - First item: subject line. Second item: full email body.\n",
        chrono::Local::now().format("%A %d %B %Y"),
        client_name,
        code,
        contact_name,
        parsed.purpose.trim(),
        parsed.context_blob.as_deref().unwrap_or("(no source)"),
        parsed.extra_notes.as_deref().unwrap_or("(none)"),
        receipt_id,
        code,
        client_name,
    );

    let system = skill_system_prompt(SKILL_CLIENT_EMAIL);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let json = call_anthropic_for_receipt(&key, &body, "L4").await?;
    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;
    Ok(json)
}

// Edit-pass skills (humanizer + copy-editor) take a single `text` input
// and return the rewritten version. Both run at L2 because Caitlin
// previews the output before applying the change to the source draft.

#[derive(Deserialize, Debug)]
struct HumanizerInput {
    text: String,
}

#[tauri::command]
async fn run_humanizer(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    let parsed: HumanizerInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    if parsed.text.trim().is_empty() {
        return Err("Text to humanise required".to_string());
    }
    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let user_message = format!(
        "## Humaniser pass\n\n\
         **Today's date:** {}\n\n\
         ## Source text\n\n{}\n\n\
         Produce the rewritten version per the skill rules. Plain prose only.\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: INC\n\
         - workflow: humanizer\n\
         - title: RECEIPT — HUMANISED DRAFT\n\
         - paid_block.customer: In Cahoots\n\
         - sections[0].header: REWRITTEN\n\
         - sections[0].items[0].qty: \"✓\"\n\
         - sections[0].items[0].text: full rewritten text (line breaks allowed).\n",
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.text.trim(),
        receipt_id,
    );

    let system = skill_system_prompt(SKILL_HUMANIZER);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let json = call_anthropic_for_receipt(&key, &body, "L2").await?;
    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;
    Ok(json)
}

#[derive(Deserialize, Debug)]
struct CopyEditorInput {
    text: String,
}

#[tauri::command]
async fn run_copy_editor(
    input: serde_json::Value,
    state: State<'_, DbState>,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;
    let key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    let parsed: CopyEditorInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    if parsed.text.trim().is_empty() {
        return Err("Text to edit required".to_string());
    }
    let receipt_id = format!(
        "rcpt_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let user_message = format!(
        "## In Cahoots copy-editor pass\n\n\
         **Today's date:** {}\n\n\
         ## Source text\n\n{}\n\n\
         Edit the source per the skill rules. Match Caitlin's voice. Australian \
         spelling. No em dashes. No marketing clichés.\n\n\
         ## Companion receipt envelope hints\n\n\
         - id: {}\n\
         - project: INC\n\
         - workflow: in-cahoots-copy-editor\n\
         - title: RECEIPT — COPY-EDITOR PASS\n\
         - paid_block.customer: In Cahoots\n\
         - sections[0].header: EDITED\n\
         - sections[0].items[0].qty: \"✓\"\n\
         - sections[0].items[0].text: full edited text (line breaks allowed).\n",
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.text.trim(),
        receipt_id,
    );

    let system = skill_system_prompt(SKILL_IN_CAHOOTS_COPY_EDITOR);
    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: &system,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: None,
    };

    let json = call_anthropic_for_receipt(&key, &body, "L2").await?;
    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }
    file_receipt_to_airtable(&json).await;
    Ok(json)
}

// Shared HTTP path for skill workflows. Posts the request, parses the
// response, extracts the fenced JSON receipt, and stamps the autonomy
// level. Returns the receipt JSON ready to persist.
async fn call_anthropic_for_receipt(
    api_key: &str,
    body: &AnthropicRequest<'_>,
    autonomy_level: &str,
) -> Result<String, String> {
    let raw = call_anthropic_raw_text(api_key, body).await?;
    let json = stamp_autonomy_level(&extract_json_block(&raw)?, autonomy_level);
    Ok(json)
}

// As above but returns the raw concatenated text so callers can also
// keep the markdown body that sits above the JSON block.
async fn call_anthropic_raw_text(
    api_key: &str,
    body: &AnthropicRequest<'_>,
) -> Result<String, String> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;

    let response = http
        .post(ANTHROPIC_API)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic API {}: {}", status.as_u16(), text));
    }

    let api_response: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    Ok(collect_text_blocks(&api_response.content))
}

#[derive(Deserialize, Debug)]
struct CreateSubcontractorArgs {
    code: String,
    name: String,
    role: Option<String>,
    start_date: Option<String>,
    hourly_rate: Option<f64>,
    email: Option<String>,
    notes: Option<String>,
}

#[tauri::command]
async fn create_airtable_subcontractor(args: serde_json::Value) -> Result<String, String> {
    let parsed: CreateSubcontractorArgs = serde_json::from_value(args)
        .map_err(|e| format!("Invalid args: {}", e))?;

    let code_upper = parsed.code.trim().to_uppercase();
    if code_upper.is_empty() {
        return Err("Code required (e.g. ROS-S)".to_string());
    }
    // Allow shapes like "ROS-S" and "ROS"
    if !code_upper.chars().all(|c| c.is_ascii_alphabetic() || c == '-') {
        return Err("Code must be letters and optional hyphen".to_string());
    }

    let mut fields = serde_json::json!({
        "code": code_upper,
        "name": parsed.name.trim(),
        "status": "active",
    });
    if let Some(r) = parsed.role.as_deref() {
        if !r.trim().is_empty() {
            fields["role"] = serde_json::Value::String(r.trim().to_string());
        }
    }
    if let Some(s) = parsed.start_date.as_deref() {
        if !s.trim().is_empty() {
            fields["started_at"] = serde_json::Value::String(s.trim().to_string());
        }
    }
    if let Some(r) = parsed.hourly_rate {
        fields["hourly_rate"] = serde_json::json!(r);
    }
    if let Some(e) = parsed.email.as_deref() {
        if !e.trim().is_empty() {
            fields["email"] = serde_json::Value::String(e.trim().to_string());
        }
    }
    if let Some(n) = parsed.notes.as_deref() {
        if !n.trim().is_empty() {
            fields["notes"] = serde_json::Value::String(n.trim().to_string());
        }
    }

    airtable_create_record("Subcontractors", fields).await
}

// ── Airtable Clients write (used by airtable:create-client on_done) ────

#[derive(Deserialize, Debug)]
struct CreateClientArgs {
    code: String,
    name: String,
    status: Option<String>,
    primary_contact_email: Option<String>,
    notes: Option<String>,
}

#[tauri::command]
async fn create_airtable_client(args: serde_json::Value) -> Result<String, String> {
    let parsed: CreateClientArgs = serde_json::from_value(args)
        .map_err(|e| format!("Invalid args: {}", e))?;

    let code_upper = parsed.code.trim().to_uppercase();
    if code_upper.is_empty() || code_upper.len() > 4 {
        return Err("Client code must be 1-4 uppercase letters".to_string());
    }
    if !code_upper.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err("Client code must be letters only".to_string());
    }

    // Refuse if a client with this code already exists.
    if airtable_find_client_by_code(&code_upper).await?.is_some() {
        return Err(format!("Client {} already exists in Airtable", code_upper));
    }

    let mut fields = serde_json::json!({
        "code": code_upper,
        "name": parsed.name.trim(),
        "status": parsed.status.as_deref().unwrap_or("active"),
        "onboarded_at": chrono::Local::now().format("%Y-%m-%d").to_string(),
    });
    if let Some(email) = parsed.primary_contact_email.as_deref() {
        if !email.trim().is_empty() {
            fields["primary_contact_email"] = serde_json::Value::String(email.trim().to_string());
        }
    }
    if let Some(notes) = parsed.notes.as_deref() {
        if !notes.trim().is_empty() {
            fields["notes"] = serde_json::Value::String(notes.trim().to_string());
        }
    }

    airtable_create_record("Clients", fields).await
}

// ── v0.33 Block F — Lead-to-Client one-click promotion ─────────────────
//
// "Promote Lead" cascades five steps in one Tauri command. Each step is
// best-effort up to the first hard failure (Airtable rows + folder are
// left in place; Caitlin can clean up manually). The composite Receipt
// files at L4 because the CSA send + calendar invite + Lead status flip
// are L5 and gated by Caitlin's confirmation in the modal that follows.
//
// Steps:
//   1. Create Client row (strip `-L` suffix from Lead.code, copy name +
//      contact, prefix notes with "Promoted from lead: ...").
//   2. Create first Project row: code = {CLIENT}-{YYYY}-{MM}-discovery,
//      type=discovery, status=active, start_date=today.
//   3. Create Dropbox client folder at Dropbox/CLIENTS/{slug}/ with the
//      standard six-folder template + 99_archive. Update Client row's
//      dropbox_folder URL.
//   4. Draft CSA from 05 TEMPLATES/agreement-template.md with merge
//      fields filled. Save to {client}/01_contracts/CSA-{slug}-{YYYY-MM-DD}.md
//   5. Suggest three 30-minute discovery slots from free/busy in next 7
//      days (don't auto-create event — surface for Caitlin to confirm).
//
// Each step files its own Receipt. A summary Receipt covers the workflow
// at L4 with workflow="promote-lead".

#[derive(Deserialize, Debug)]
struct PromoteLeadArgs {
    lead_record_id: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct CalendarSlot {
    pub start: String,        // RFC3339
    pub end: String,
    pub label: String,        // e.g. "Wed 7 May, 10:00am"
}

#[derive(Serialize, Debug)]
pub struct PromoteLeadResult {
    pub client_record_id: String,
    pub client_code: String,
    pub client_name: String,
    pub client_slug: String,
    pub project_record_id: String,
    pub project_code: String,
    pub dropbox_folder_path: String,
    pub dropbox_folder_url: String,
    pub csa_file_path: String,
    pub calendar_slots: Vec<CalendarSlot>,
    pub primary_contact_email: String,
}

// Find a Lead by its Airtable record id. Returns the fields we need to
// populate the Client row + downstream steps.
async fn airtable_get_lead(record_id: &str) -> Result<serde_json::Value, String> {
    let (api_key, base_id) = read_airtable_creds().ok_or("Airtable not configured")?;
    let url = format!("{}/{}/Leads/{}", AIRTABLE_API, base_id, record_id);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let resp = client
        .get(&url)
        .bearer_auth(&api_key)
        .send()
        .await
        .map_err(|e| format!("Airtable get: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Airtable Leads/{} {}: {}", record_id, status.as_u16(), body));
    }
    resp.json::<serde_json::Value>().await.map_err(|e| format!("Parse: {}", e))
}

// Strip a trailing `-L` suffix from a lead code so e.g. NCT-L → NCT.
// Also trims whitespace and uppercases. Returns empty string if input
// is empty.
fn strip_lead_suffix(raw: &str) -> String {
    let upper = raw.trim().to_uppercase();
    if let Some(stripped) = upper.strip_suffix("-L") {
        stripped.to_string()
    } else {
        upper
    }
}

// Lowercase + spaces-to-hyphens + strip non-[a-z0-9-] for filesystem-safe
// folder names. e.g. "Northcote Theatre" → "northcote-theatre".
fn slugify(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_hyphen = false;
    for c in raw.trim().chars() {
        let lower = c.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            last_was_hyphen = false;
        } else if lower == '-' || lower == ' ' || lower == '_' || lower == '/' {
            if !last_was_hyphen && !out.is_empty() {
                out.push('-');
                last_was_hyphen = true;
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

// Build the URL-encoded Dropbox web link for the team-shared CLIENTS root.
// Mirrors Caitlin's CLAUDE.md formula: /home/ + URL-encoded relative path.
fn dropbox_web_url_for_client_folder(slug: &str) -> String {
    format!("https://www.dropbox.com/home/CLIENTS/{}", urlencode(slug))
}

// Resolve the local Dropbox CLIENTS root. Honour DROPBOX_CLIENTS_ROOT for
// tests; default to the standard path under the user's home.
fn dropbox_clients_root() -> std::path::PathBuf {
    if let Ok(custom) = std::env::var("DROPBOX_CLIENTS_ROOT") {
        if !custom.trim().is_empty() {
            return std::path::PathBuf::from(custom);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/caitlinreilly".to_string());
    std::path::PathBuf::from(home)
        .join("Library")
        .join("CloudStorage")
        .join("Dropbox")
        .join("CLIENTS")
}

// Path to the agreement template inside the team-shared 05 TEMPLATES folder.
fn agreement_template_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/caitlinreilly".to_string());
    std::path::PathBuf::from(home)
        .join("Library")
        .join("CloudStorage")
        .join("Dropbox")
        .join("IN CAHOOTS")
        .join("05 TEMPLATES")
        .join("agreement-template.md")
}

// Create the standard six-folder template (plus 99_archive) under
// CLIENTS/{slug}/. Idempotent — `mkdir -p` style. Returns the absolute
// path to the client root.
fn create_client_folder_template(slug: &str) -> Result<std::path::PathBuf, String> {
    let root = dropbox_clients_root();
    if !root.exists() {
        return Err(format!(
            "Dropbox CLIENTS root not found at {}",
            root.display()
        ));
    }
    let client_root = root.join(slug);
    let subfolders = [
        "00_intake",
        "01_contracts",
        "02_briefs",
        "03_creative",
        "04_reporting",
        "05_admin",
        "99_archive",
    ];
    for sub in subfolders.iter() {
        let p = client_root.join(sub);
        std::fs::create_dir_all(&p).map_err(|e| {
            format!("mkdir -p {}: {}", p.display(), e)
        })?;
    }
    Ok(client_root)
}

// Read the agreement template, replace merge fields, return the filled
// markdown body. Leaves FEE_TOTAL / PAYMENT_SCHEDULE / DELIVERABLES /
// CAMPAIGN_DELIVERABLES blank for Caitlin to fill before sending.
fn fill_agreement_template(
    client_name: &str,
    project_name: &str,
    today: &str,
    primary_contact: &str,
    primary_contact_email: &str,
) -> Result<String, String> {
    let template_path = agreement_template_path();
    let raw = std::fs::read_to_string(&template_path)
        .map_err(|e| format!("Read agreement template at {}: {}", template_path.display(), e))?;

    // Caitlin's merge fields. Two patterns coexist in the template
    // (single + double braces are both treated as merge tokens here).
    let replacements: &[(&str, &str)] = &[
        ("{{CLIENT_NAME}}", client_name),
        ("{{PROJECT_NAME}}", project_name),
        ("{{DATE}}", today),
        ("{{CONTRACTOR_NAME}}", "Caitlin Reilly"),
        ("{{CONTRACTOR_EMAIL}}", "psst@incahoots.marketing"),
        ("{{WEBSITE}}", "incahoots.marketing"),
        ("{{CLIENT_CONTACT}}", primary_contact),
        ("{{CLIENT_EMAIL}}", primary_contact_email),
    ];
    let mut filled = raw;
    for (k, v) in replacements {
        filled = filled.replace(k, v);
    }
    // FEE_TOTAL / PAYMENT_SCHEDULE / DELIVERABLES / CAMPAIGN_DELIVERABLES
    // are intentionally left as merge tokens so Caitlin can find and fill
    // them quickly before sending via Dropbox Sign.
    Ok(filled)
}

// Suggest three 30-minute slots in the next 7 days. Reads the user's
// busy times via the Calendar v3 freeBusy endpoint and walks 10:00–17:00
// local-time work hours, skipping weekends, returning the first three
// gaps that fit a 30-minute meeting with at least 30 minutes' buffer
// before the next busy block.
//
// Returns an empty Vec when Google isn't connected — the modal then
// surfaces a "Pick a slot manually" message rather than failing the
// cascade.
async fn suggest_calendar_slots() -> Vec<CalendarSlot> {
    use chrono::{Datelike, Duration, Local, NaiveTime, TimeZone, Timelike};

    let token = match crate::oauth::ensure_fresh_token(&crate::google::GOOGLE).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[promote-lead] calendar slots: oauth: {}", e);
            return Vec::new();
        }
    };

    let now = Local::now();
    let start = now;
    let end = now + Duration::days(7);

    // Pull free/busy across the user's primary calendar. The freeBusy
    // endpoint returns a flat list of busy intervals — walk them after
    // sorting by start time.
    let body = serde_json::json!({
        "timeMin": start.to_rfc3339(),
        "timeMax": end.to_rfc3339(),
        "items": [{ "id": "primary" }],
    });
    let url = format!("{}/freeBusy", crate::google::CALENDAR_API_BASE);
    let http = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let resp = match http
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[promote-lead] freeBusy network: {}", e);
            return Vec::new();
        }
    };
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        eprintln!("[promote-lead] freeBusy status: {}", body);
        return Vec::new();
    }
    let parsed: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[promote-lead] freeBusy parse: {}", e);
            return Vec::new();
        }
    };
    let busy_arr = parsed
        .pointer("/calendars/primary/busy")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut busy: Vec<(chrono::DateTime<Local>, chrono::DateTime<Local>)> = busy_arr
        .iter()
        .filter_map(|b| {
            let s = b["start"].as_str()?;
            let e = b["end"].as_str()?;
            let s = chrono::DateTime::parse_from_rfc3339(s).ok()?.with_timezone(&Local);
            let e = chrono::DateTime::parse_from_rfc3339(e).ok()?.with_timezone(&Local);
            Some((s, e))
        })
        .collect();
    busy.sort_by(|a, b| a.0.cmp(&b.0));

    let mut slots: Vec<CalendarSlot> = Vec::new();
    let work_start = NaiveTime::from_hms_opt(10, 0, 0).unwrap();
    let work_end = NaiveTime::from_hms_opt(17, 0, 0).unwrap();
    let slot_len = Duration::minutes(30);

    // Walk day-by-day from tomorrow to start+7d. Skip weekends.
    let mut day_cursor = (now + Duration::days(1)).date_naive();
    let final_day = end.date_naive();
    while day_cursor <= final_day && slots.len() < 3 {
        let weekday = day_cursor.weekday().number_from_monday();
        if weekday >= 6 {
            day_cursor = day_cursor.succ_opt().unwrap_or(day_cursor);
            continue;
        }
        let day_start = match Local
            .from_local_datetime(&day_cursor.and_time(work_start))
            .single()
        {
            Some(t) => t,
            None => {
                day_cursor = day_cursor.succ_opt().unwrap_or(day_cursor);
                continue;
            }
        };
        let day_end = match Local
            .from_local_datetime(&day_cursor.and_time(work_end))
            .single()
        {
            Some(t) => t,
            None => {
                day_cursor = day_cursor.succ_opt().unwrap_or(day_cursor);
                continue;
            }
        };

        // Walk the day in 30-minute increments. Only mark a slot when a
        // 30-minute window fits without colliding with a busy block.
        let mut cursor = day_start;
        while cursor + slot_len <= day_end && slots.len() < 3 {
            let cursor_end = cursor + slot_len;
            let collides = busy.iter().any(|(bs, be)| cursor < *be && cursor_end > *bs);
            if !collides {
                slots.push(CalendarSlot {
                    start: cursor.to_rfc3339(),
                    end: cursor_end.to_rfc3339(),
                    label: format_slot_label(&cursor),
                });
                // Step forward an hour after picking a slot so the three
                // suggestions don't bunch up back-to-back.
                cursor = cursor + Duration::hours(1);
            } else {
                cursor = cursor + Duration::minutes(30);
            }
            // Defend against a malformed clock that doesn't advance.
            if cursor.hour() == day_start.hour() && cursor.minute() == day_start.minute() {
                break;
            }
        }
        day_cursor = day_cursor.succ_opt().unwrap_or(day_cursor);
    }
    slots
}

fn format_slot_label(dt: &chrono::DateTime<chrono::Local>) -> String {
    use chrono::Timelike;
    // "Wed 7 May, 10:00am" — short and human, mirrors the Calendar
    // section's fmtDateTime in render.js.
    let day = dt.format("%a %-d %b").to_string();
    let mut hour = dt.hour();
    let minute = dt.minute();
    let suffix = if hour >= 12 { "pm" } else { "am" };
    if hour == 0 {
        hour = 12;
    } else if hour > 12 {
        hour -= 12;
    }
    if minute == 0 {
        format!("{}, {}{}", day, hour, suffix)
    } else {
        format!("{}, {}:{:02}{}", day, hour, minute, suffix)
    }
}

// File a small "step" Receipt. Used for each cascade step plus the
// summary at the end. All filed at L4 (drafts gated by the modal's L5
// confirmations); the final summary inherits the same level.
async fn file_promote_step_receipt(
    state: &State<'_, DbState>,
    workflow: &str,
    title: &str,
    project: &str,
    items: Vec<serde_json::Value>,
    autonomy: &str,
) {
    let now = chrono::Local::now();
    let receipt_id = format!("rcpt_{}_{}", workflow, now.format("%Y-%m-%d_%H-%M-%S_%f"));
    let sections = serde_json::json!([{ "items": items }]);
    let receipt_json = stamp_autonomy_level(
        &build_pure_receipt(&receipt_id, workflow, title, project, sections),
        autonomy,
    );
    {
        if let Ok(conn) = state.0.lock() {
            let _ = persist_receipt(&conn, &receipt_json);
        }
    }
    file_receipt_to_airtable(&receipt_json).await;
}

#[tauri::command]
async fn promote_lead(
    args: serde_json::Value,
    state: State<'_, DbState>,
) -> Result<PromoteLeadResult, String> {
    let parsed: PromoteLeadArgs = serde_json::from_value(args)
        .map_err(|e| format!("Invalid args: {}", e))?;
    let lead_record_id = parsed.lead_record_id.trim();
    if lead_record_id.is_empty() {
        return Err("Lead record id required".to_string());
    }

    // ── Read the Lead row ─────────────────────────────────────────────
    let lead = airtable_get_lead(lead_record_id).await?;
    let fields = &lead["fields"];
    let lead_code_raw = fields["code"].as_str().unwrap_or("").to_string();
    let lead_name = fields["name"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if lead_name.is_empty() {
        return Err("Lead is missing a name — fill it in Airtable first".to_string());
    }
    let primary_contact = fields["primary_contact_name"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    let primary_contact_email = fields["primary_contact_email"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    let lead_source = fields["source"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    let lead_notes = fields["notes"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    let client_code = strip_lead_suffix(&lead_code_raw);
    if client_code.is_empty() {
        return Err("Lead is missing a code — set one in Airtable first".to_string());
    }
    if client_code.len() > 4 || !client_code.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(format!(
            "Lead code {} doesn't strip to a 1-4 letter client code",
            lead_code_raw
        ));
    }

    // Refuse if this client code already exists. Otherwise the cascade
    // would create a duplicate Client row.
    if airtable_find_client_by_code(&client_code).await?.is_some() {
        return Err(format!(
            "Client {} already exists in Airtable — promotion would duplicate. Resolve manually.",
            client_code
        ));
    }

    let slug = slugify(&lead_name);
    if slug.is_empty() {
        return Err("Couldn't slugify client name".to_string());
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let yyyy_mm = chrono::Local::now().format("%Y-%m").to_string();

    // ── Step 1: Create Client row ─────────────────────────────────────
    let mut client_notes = String::new();
    if !lead_source.is_empty() {
        client_notes.push_str(&format!("Promoted from lead. Source: {}.", lead_source));
    } else {
        client_notes.push_str("Promoted from lead.");
    }
    if !lead_notes.is_empty() {
        client_notes.push_str("\n\n");
        client_notes.push_str(&lead_notes);
    }

    let mut client_fields = serde_json::json!({
        "code": client_code,
        "name": lead_name,
        "status": "active",
        "onboarded_at": today,
        "notes": client_notes,
    });
    if !primary_contact.is_empty() {
        client_fields["primary_contact_name"] = serde_json::Value::String(primary_contact.clone());
    }
    if !primary_contact_email.is_empty() {
        client_fields["primary_contact_email"] =
            serde_json::Value::String(primary_contact_email.clone());
    }
    let client_record_id = airtable_create_record("Clients", client_fields).await?;
    file_promote_step_receipt(
        &state,
        "promote-lead-client",
        &format!("RECEIPT — PROMOTE LEAD · client row · {}", client_code),
        &client_code,
        vec![
            serde_json::json!({ "qty": "✓", "text": format!("Created Clients row ({})", client_record_id) }),
            serde_json::json!({ "qty": "1", "text": format!("Code: {}", client_code) }),
            serde_json::json!({ "qty": "1", "text": format!("Name: {}", lead_name) }),
        ],
        "L4",
    )
    .await;

    // ── Step 2: Create Discovery Project row ──────────────────────────
    let project_code = format!("{}-{}-discovery", client_code, yyyy_mm);
    let project_fields = serde_json::json!({
        "code": project_code,
        "name": "Discovery + onboarding",
        "client": [client_record_id.clone()],
        "type": "discovery",
        "status": "active",
        "start_date": today,
    });
    let project_record_id = airtable_create_record("Projects", project_fields).await?;
    file_promote_step_receipt(
        &state,
        "promote-lead-project",
        &format!("RECEIPT — PROMOTE LEAD · discovery project · {}", project_code),
        &project_code,
        vec![
            serde_json::json!({ "qty": "✓", "text": format!("Created Projects row ({})", project_record_id) }),
            serde_json::json!({ "qty": "1", "text": format!("Code: {}", project_code) }),
            serde_json::json!({ "qty": "1", "text": "Type: discovery, status: active" }),
        ],
        "L4",
    )
    .await;

    // ── Step 3: Dropbox folder template ───────────────────────────────
    let client_folder = create_client_folder_template(&slug)?;
    let dropbox_folder_path = client_folder.to_string_lossy().to_string();
    let dropbox_folder_url = dropbox_web_url_for_client_folder(&slug);
    let _ = airtable_update_record(
        "Clients",
        &client_record_id,
        serde_json::json!({ "dropbox_folder": dropbox_folder_url.clone() }),
    )
    .await;
    file_promote_step_receipt(
        &state,
        "promote-lead-folder",
        &format!("RECEIPT — PROMOTE LEAD · folder · {}", slug),
        &client_code,
        vec![
            serde_json::json!({ "qty": "✓", "text": format!("Created folder at {}", dropbox_folder_path) }),
            serde_json::json!({ "qty": "1", "text": "Subfolders: 00_intake, 01_contracts, 02_briefs, 03_creative, 04_reporting, 05_admin, 99_archive" }),
            serde_json::json!({ "qty": "1", "text": format!("Web link: {}", dropbox_folder_url) }),
        ],
        "L4",
    )
    .await;

    // ── Step 4: Draft CSA from agreement-template.md ──────────────────
    let project_label = "Discovery + onboarding".to_string();
    let csa_body = fill_agreement_template(
        &lead_name,
        &project_label,
        &today,
        &primary_contact,
        &primary_contact_email,
    )?;
    let csa_filename = format!("CSA-{}-{}.md", slug, today);
    let csa_path = client_folder.join("01_contracts").join(&csa_filename);
    std::fs::write(&csa_path, csa_body)
        .map_err(|e| format!("Write CSA at {}: {}", csa_path.display(), e))?;
    let csa_file_path = csa_path.to_string_lossy().to_string();
    file_promote_step_receipt(
        &state,
        "promote-lead-csa",
        &format!("RECEIPT — PROMOTE LEAD · CSA draft · {}", slug),
        &client_code,
        vec![
            serde_json::json!({ "qty": "✓", "text": format!("Drafted CSA at {}", csa_file_path) }),
            serde_json::json!({ "qty": "0", "type": "task", "text": "Fill FEE_TOTAL, PAYMENT_SCHEDULE, DELIVERABLES, CAMPAIGN_DELIVERABLES" }),
            serde_json::json!({ "qty": "0", "type": "task", "text": "Upload to Dropbox Sign and send" }),
        ],
        "L4",
    )
    .await;

    // ── Step 5: Suggest three calendar slots ──────────────────────────
    let calendar_slots = suggest_calendar_slots().await;
    let slot_items: Vec<serde_json::Value> = if calendar_slots.is_empty() {
        vec![serde_json::json!({ "qty": "0", "type": "task", "text": "Pick a discovery slot manually — Google Calendar wasn't reachable" })]
    } else {
        calendar_slots
            .iter()
            .map(|s| serde_json::json!({ "qty": "1", "text": s.label }))
            .collect()
    };
    file_promote_step_receipt(
        &state,
        "promote-lead-slots",
        &format!("RECEIPT — PROMOTE LEAD · slot suggestions · {}", client_code),
        &client_code,
        slot_items,
        "L4",
    )
    .await;

    // ── Summary Receipt ───────────────────────────────────────────────
    file_promote_step_receipt(
        &state,
        "promote-lead",
        &format!("RECEIPT — PROMOTE LEAD · {} → {}", lead_code_raw, client_code),
        &client_code,
        vec![
            serde_json::json!({ "qty": "✓", "text": format!("Client {} created ({})", client_code, client_record_id) }),
            serde_json::json!({ "qty": "✓", "text": format!("Project {} created ({})", project_code, project_record_id) }),
            serde_json::json!({ "qty": "✓", "text": format!("Folder ready at {}", dropbox_folder_url) }),
            serde_json::json!({ "qty": "✓", "text": format!("CSA drafted at {}", csa_file_path) }),
            serde_json::json!({ "qty": "1", "text": format!("Slot suggestions returned: {}", calendar_slots.len()) }),
            serde_json::json!({ "qty": "0", "type": "task", "text": "Confirm discovery slot and send invite (L5)" }),
            serde_json::json!({ "qty": "0", "type": "task", "text": "Upload CSA to Dropbox Sign and send (L5)" }),
            serde_json::json!({ "qty": "0", "type": "task", "text": "Mark Lead as won (L5)" }),
        ],
        "L4",
    )
    .await;

    Ok(PromoteLeadResult {
        client_record_id,
        client_code,
        client_name: lead_name,
        client_slug: slug,
        project_record_id,
        project_code,
        dropbox_folder_path,
        dropbox_folder_url,
        csa_file_path,
        calendar_slots,
        primary_contact_email,
    })
}

// List Leads in pipeline. Used by the Pipeline view + the Today
// dashboard's Pipeline section. Active = anything not won/lost; the
// caller filters further.
#[tauri::command]
async fn list_airtable_leads() -> Result<String, String> {
    let data = airtable_get(
        "Leads",
        "filterByFormula=AND(NOT(%7Bstatus%7D%3D%27won%27)%2CNOT(%7Bstatus%7D%3D%27lost%27))\
&fields%5B%5D=code\
&fields%5B%5D=name\
&fields%5B%5D=status\
&fields%5B%5D=primary_contact_name\
&fields%5B%5D=primary_contact_email\
&fields%5B%5D=source\
&fields%5B%5D=notes\
&fields%5B%5D=created_at",
    )
    .await?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

// Mark a Lead as won (L5). Called from the confirmation modal after
// Caitlin's explicit click. Doesn't touch the Client/Project rows — the
// promote_lead cascade has already created them.
#[tauri::command]
async fn mark_lead_won(
    args: serde_json::Value,
    state: State<'_, DbState>,
) -> Result<(), String> {
    let parsed: PromoteLeadArgs = serde_json::from_value(args)
        .map_err(|e| format!("Invalid args: {}", e))?;
    let lead_record_id = parsed.lead_record_id.trim();
    if lead_record_id.is_empty() {
        return Err("Lead record id required".to_string());
    }
    airtable_update_record(
        "Leads",
        lead_record_id,
        serde_json::json!({ "status": "won" }),
    )
    .await?;
    file_promote_step_receipt(
        &state,
        "mark-lead-won",
        "RECEIPT — LEAD MARKED WON",
        "in-cahoots-studio",
        vec![
            serde_json::json!({ "qty": "✓", "text": format!("Lead {} status flipped to won", lead_record_id) }),
        ],
        "L5",
    )
    .await;
    Ok(())
}

// Build a Google Calendar prefilled-event URL for the chosen slot.
// Companion's Google scope is read-only so we don't create the event
// directly — instead we open the standard "render?action=TEMPLATE"
// composer URL in Caitlin's browser. She clicks Save, Google sends the
// invite. Files an L5 receipt because the URL open is the explicit
// confirmation moment.
#[derive(Deserialize, Debug)]
struct ConfirmSlotArgs {
    client_code: String,
    client_name: String,
    primary_contact_email: Option<String>,
    start: String, // RFC3339
    end: String,
}

#[derive(Serialize, Debug)]
pub struct ConfirmSlotResult {
    pub calendar_url: String,
}

#[tauri::command]
async fn confirm_discovery_slot(
    args: serde_json::Value,
    state: State<'_, DbState>,
) -> Result<ConfirmSlotResult, String> {
    let parsed: ConfirmSlotArgs = serde_json::from_value(args)
        .map_err(|e| format!("Invalid args: {}", e))?;
    let title = format!("Discovery — {} + In Cahoots", parsed.client_name.trim());
    let details = format!(
        "Discovery call for {}. Booked via Companion v0.33 promote-lead.",
        parsed.client_name.trim()
    );
    // Convert RFC3339 → Google's compact format YYYYMMDDTHHMMSSZ.
    let start_compact = compact_calendar_ts(&parsed.start)
        .ok_or("Invalid slot start")?;
    let end_compact = compact_calendar_ts(&parsed.end)
        .ok_or("Invalid slot end")?;
    let mut url = format!(
        "https://calendar.google.com/calendar/u/0/r/eventedit?text={}&dates={}/{}&details={}",
        urlencode(&title),
        start_compact,
        end_compact,
        urlencode(&details),
    );
    if let Some(email) = parsed.primary_contact_email.as_deref() {
        let email = email.trim();
        if !email.is_empty() {
            url.push_str("&add=");
            url.push_str(&urlencode(email));
        }
    }
    file_promote_step_receipt(
        &state,
        "confirm-discovery-slot",
        &format!("RECEIPT — DISCOVERY SLOT CONFIRMED · {}", parsed.client_code),
        parsed.client_code.trim(),
        vec![
            serde_json::json!({ "qty": "✓", "text": format!("Slot: {} → {}", parsed.start, parsed.end) }),
            serde_json::json!({ "qty": "1", "text": "Opens Google Calendar event composer prefilled — Caitlin clicks Save to send the invite" }),
        ],
        "L5",
    )
    .await;
    Ok(ConfirmSlotResult { calendar_url: url })
}

fn compact_calendar_ts(rfc3339: &str) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(rfc3339).ok()?;
    Some(dt.with_timezone(&chrono::Utc).format("%Y%m%dT%H%M%SZ").to_string())
}

// ── List Subcontractors (v0.21) ────────────────────────────────────────
//
// Used by the Log time picker so we can default the rate by who's logging
// (Caitlin $110, Rose $66). Active subs only.
#[tauri::command]
async fn list_airtable_subcontractors() -> Result<String, String> {
    let data = airtable_get(
        "Subcontractors",
        "filterByFormula=%7Bstatus%7D%3D%27active%27\
&fields%5B%5D=code\
&fields%5B%5D=name\
&fields%5B%5D=role\
&fields%5B%5D=hourly_rate\
&fields%5B%5D=status",
    )
    .await?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

// ── Per-Subcontractor view (v0.38) ─────────────────────────────────────
//
// Rose's home in Companion. Sidebar shows active subcontractors; clicking
// one opens a view scoped to that person — header, assigned workstreams,
// open commitments, hours-this-month, recent receipts, and a small skill
// strip. The two commands below feed that view.

// Trim list of active subcontractors used for the sidebar. Same data as
// list_airtable_subcontractors above but pre-shaped so the JS layer can
// render without parsing Airtable's record envelope.
#[derive(serde::Serialize)]
struct SubcontractorSummary {
    code: String,
    name: String,
    role: String,
}

#[tauri::command]
async fn list_active_subcontractors() -> Result<Vec<SubcontractorSummary>, String> {
    let data = airtable_get(
        "Subcontractors",
        "filterByFormula=%7Bstatus%7D%3D%27active%27\
&fields%5B%5D=code\
&fields%5B%5D=name\
&fields%5B%5D=role\
&fields%5B%5D=status",
    )
    .await?;
    let mut out: Vec<SubcontractorSummary> = Vec::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let code = f["code"].as_str().unwrap_or("").trim().to_uppercase();
            if code.is_empty() {
                continue;
            }
            out.push(SubcontractorSummary {
                code,
                name: f["name"].as_str().unwrap_or("").to_string(),
                role: f["role"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.code.cmp(&b.code));
    Ok(out)
}

// Find the full Subcontractors row by code. Returns the raw Airtable
// record so the caller can read whatever fields it needs.
async fn airtable_find_subcontractor_record_by_code(
    code: &str,
) -> Result<Option<serde_json::Value>, String> {
    if code.is_empty() {
        return Ok(None);
    }
    let escaped = code.replace('\'', "");
    let formula = format!("{{code}}='{}'", escaped);
    let qs = format!(
        "filterByFormula={}&maxRecords=1\
&fields%5B%5D=code\
&fields%5B%5D=name\
&fields%5B%5D=role\
&fields%5B%5D=hourly_rate\
&fields%5B%5D=email\
&fields%5B%5D=status\
&fields%5B%5D=start_date\
&fields%5B%5D=notes",
        urlencode(&formula)
    );
    let data = airtable_get("Subcontractors", &qs).await?;
    Ok(data["records"]
        .as_array()
        .and_then(|a| a.first().cloned()))
}

// Build the {date} >= "YYYY-MM-01" formula so we can pull hours for the
// current calendar month. Airtable's formula language treats date strings
// as ISO 8601 when compared with IS_AFTER / DATETIME_PARSE.
fn first_of_this_month_iso() -> String {
    let now = chrono::Local::now();
    format!("{:04}-{:02}-01", now.format("%Y"), now.format("%m"))
}

#[tauri::command]
async fn load_subcontractor_view(code: String) -> Result<serde_json::Value, String> {
    let upper = code.trim().to_uppercase();
    if upper.is_empty() {
        return Err("Empty subcontractor code".to_string());
    }

    // 1. Header — full Subcontractors row.
    let record = airtable_find_subcontractor_record_by_code(&upper)
        .await?
        .ok_or_else(|| format!("Subcontractor {} not found", upper))?;
    let record_id = record["id"].as_str().unwrap_or("").to_string();
    let f = &record["fields"];
    let header = serde_json::json!({
        "code": f["code"].as_str().unwrap_or(&upper),
        "name": f["name"].as_str().unwrap_or(""),
        "role": f["role"].as_str().unwrap_or(""),
        "hourly_rate": f["hourly_rate"].as_f64(),
        "email": f["email"].as_str().unwrap_or(""),
        "start_date": f["start_date"].as_str().unwrap_or(""),
        "status": f["status"].as_str().unwrap_or(""),
        "notes": f["notes"].as_str().unwrap_or(""),
    });

    // 2. Workstreams linked to this subcontractor. {subcontractor} is
    //    a multipleRecordLinks field on Workstreams. FIND() over the
    //    rendered string returns >0 when our record id is in the list.
    let escaped_id = record_id.replace('\'', "");
    let ws_formula = format!(
        "AND(OR({{status}}='active',{{status}}='blocked'),FIND('{}',ARRAYJOIN({{subcontractor}})))",
        escaped_id
    );
    let ws_qs = format!(
        "filterByFormula={}&pageSize=50\
&fields%5B%5D=code\
&fields%5B%5D=title\
&fields%5B%5D=status\
&fields%5B%5D=phase\
&fields%5B%5D=next_action\
&fields%5B%5D=last_touch_at\
&fields%5B%5D=client\
&fields%5B%5D=project",
        urlencode(&ws_formula)
    );
    let workstreams = match airtable_get("Workstreams", &ws_qs).await {
        Ok(v) => v["records"].clone(),
        Err(e) => {
            eprintln!("load_subcontractor_view: workstreams fetch failed: {}", e);
            serde_json::Value::Array(vec![])
        }
    };

    // 3. Open commitments owned by this subcontractor.
    let cm_formula = format!(
        "AND(OR({{status}}='open',{{status}}='overdue'),FIND('{}',ARRAYJOIN({{subcontractor}})))",
        escaped_id
    );
    let cm_qs = format!(
        "filterByFormula={}&pageSize=50\
&fields%5B%5D=id\
&fields%5B%5D=title\
&fields%5B%5D=due_at\
&fields%5B%5D=status\
&fields%5B%5D=priority\
&fields%5B%5D=client\
&fields%5B%5D=project",
        urlencode(&cm_formula)
    );
    let commitments = match airtable_get("Commitments", &cm_qs).await {
        Ok(v) => v["records"].clone(),
        // Subcontractor link on Commitments is optional in the schema
        // (older bases may not have it). Treat missing field as empty.
        Err(e) => {
            eprintln!(
                "load_subcontractor_view: commitments fetch failed (non-fatal): {}",
                e
            );
            serde_json::Value::Array(vec![])
        }
    };

    // 4. Hours this month. TimeLogs filtered by the linked subcontractor
    //    record + date >= first of month. Sum across logs, group by
    //    client_code from the linked rollup field.
    let month_start = first_of_this_month_iso();
    let tl_formula = format!(
        "AND(IS_AFTER({{date}},'{}'),FIND('{}',ARRAYJOIN({{subcontractor}})))",
        month_start, escaped_id
    );
    // Fetch enough rows to cover a busy month without paging.
    let tl_qs = format!(
        "filterByFormula={}&pageSize=100\
&fields%5B%5D=date\
&fields%5B%5D=hours\
&fields%5B%5D=client_code\
&fields%5B%5D=client\
&fields%5B%5D=billable",
        urlencode(&tl_formula)
    );
    let timelogs = match airtable_get("TimeLogs", &tl_qs).await {
        Ok(v) => v["records"].clone(),
        Err(e) => {
            eprintln!("load_subcontractor_view: timelogs fetch failed: {}", e);
            serde_json::Value::Array(vec![])
        }
    };

    // 5. Recent receipts created by this person. Today this is best-effort
    //    on the JSON payload — some receipts carry `created_by` /
    //    `subcontractor` in the JSON, others (legacy) don't. Newest 50;
    //    JS layer filters down by JSON contents.
    let rec_qs = "pageSize=50\
&fields%5B%5D=id\
&fields%5B%5D=title\
&fields%5B%5D=date\
&fields%5B%5D=workflow\
&fields%5B%5D=ticked_count\
&fields%5B%5D=json\
&sort%5B0%5D%5Bfield%5D=date\
&sort%5B0%5D%5Bdirection%5D=desc";
    let receipts = match airtable_get("Receipts", rec_qs).await {
        Ok(v) => v["records"].clone(),
        Err(e) => {
            eprintln!("load_subcontractor_view: receipts fetch failed: {}", e);
            serde_json::Value::Array(vec![])
        }
    };

    Ok(serde_json::json!({
        "record_id": record_id,
        "header": header,
        "workstreams": workstreams,
        "commitments": commitments,
        "timelogs": timelogs,
        "receipts": receipts,
        "month_start": month_start,
    }))
}

// "I am" identity. Companion has no users today — but Rose is about to
// start using it from her own Mac, so we need to know which Subcontractor
// is logged in for receipt filtering and per-Subcontractor view picking.
//
// Two layers:
//   1. OS user from `whoami` (read-only fallback).
//   2. An override stored in the Keychain ("caitlin", "rose", or "other").
//
// JS uses get_i_am to populate the Settings dropdown and to scope filters
// like "receipts by Rose". Default is "auto" — read OS user via whoami_user.

const KEYRING_I_AM: &str = "i-am-override";

#[tauri::command]
async fn whoami_user() -> Result<String, String> {
    let output = tokio::process::Command::new("whoami")
        .output()
        .await
        .map_err(|e| format!("whoami failed: {}", e))?;
    if !output.status.success() {
        return Err("whoami exited non-zero".to_string());
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(s)
}

#[tauri::command]
fn get_i_am() -> String {
    cached_secret(KEYRING_I_AM).unwrap_or_else(|| "auto".to_string())
}

#[tauri::command]
fn save_i_am(value: String) -> Result<(), String> {
    let trimmed = value.trim().to_lowercase();
    let allowed = ["auto", "caitlin", "rose", "other"];
    if !allowed.contains(&trimmed.as_str()) {
        return Err(format!(
            "Unknown identity {}. Use one of: {}",
            trimmed,
            allowed.join(", ")
        ));
    }
    let entry = Entry::new(KEYRING_SERVICE, KEYRING_I_AM)
        .map_err(|e| format!("Keychain error: {}", e))?;
    entry
        .set_password(&trimmed)
        .map_err(|e| format!("Keychain write error: {}", e))?;
    cache_secret(KEYRING_I_AM, &trimmed);
    Ok(())
}

// ── Pure-Airtable workflows (v0.21) ────────────────────────────────────
//
// Three workflows that don't hit Anthropic — pure forms that write to
// Airtable and file a Receipt for traceability. Schedule social post,
// Log time, Edit project.

// Builds a tiny receipt JSON with one section + one task line per field.
// Used by the v0.21 pure-Airtable workflows so each write leaves a trail
// in the Receipts table identical to the Anthropic-backed workflows.
fn build_pure_receipt(
    receipt_id: &str,
    workflow: &str,
    title: &str,
    project: &str,
    sections: serde_json::Value,
) -> String {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let receipt = serde_json::json!({
        "id": receipt_id,
        "project": project,
        "workflow": workflow,
        "title": title,
        "date": date,
        "sections": sections,
    });
    receipt.to_string()
}

#[derive(Deserialize, Debug)]
struct SocialPostPayload {
    title: Option<String>,
    platform: Option<String>,
    scheduled_at: Option<String>,
    status: Option<String>,
    channel: Option<String>,
    client_code: Option<String>,
    copy: String,
    image_path: Option<String>,
    approval_status: Option<String>,
    notes: Option<String>,
}

#[tauri::command]
async fn create_social_post(
    payload: serde_json::Value,
    state: State<'_, DbState>,
) -> Result<String, String> {
    let parsed: SocialPostPayload = serde_json::from_value(payload)
        .map_err(|e| format!("Invalid payload: {}", e))?;

    let copy = parsed.copy.trim();
    if copy.is_empty() {
        return Err("Copy required".to_string());
    }

    // Derive a short id + title.
    let now = chrono::Local::now();
    let post_id = format!("sp_{}", now.format("%Y-%m-%d_%H-%M-%S"));
    let title = parsed
        .title
        .as_deref()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| {
            let head: String = copy.chars().take(60).collect();
            if copy.chars().count() > 60 {
                format!("{}...", head)
            } else {
                head
            }
        });

    let mut fields = serde_json::json!({
        "id": post_id,
        "title": title,
        "copy": copy,
    });
    if let Some(p) = parsed.platform.as_deref() {
        if !p.trim().is_empty() {
            fields["platform"] = serde_json::Value::String(p.trim().to_string());
        }
    }
    if let Some(s) = parsed.scheduled_at.as_deref() {
        if !s.trim().is_empty() {
            fields["scheduled_at"] = serde_json::Value::String(s.trim().to_string());
        }
    }
    let status = parsed.status.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("draft");
    fields["status"] = serde_json::Value::String(status.to_string());
    if let Some(c) = parsed.channel.as_deref() {
        if !c.trim().is_empty() {
            fields["channel"] = serde_json::Value::String(c.trim().to_string());
        }
    }
    let approval = parsed.approval_status.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("pending");
    fields["approval_status"] = serde_json::Value::String(approval.to_string());
    if let Some(img) = parsed.image_path.as_deref() {
        if !img.trim().is_empty() {
            fields["image_path"] = serde_json::Value::String(img.trim().to_string());
        }
    }
    if let Some(n) = parsed.notes.as_deref() {
        if !n.trim().is_empty() {
            fields["notes"] = serde_json::Value::String(n.trim().to_string());
        }
    }

    // Link to Clients by code (best-effort).
    let mut client_code_for_project = String::new();
    if let Some(code) = parsed.client_code.as_deref() {
        let upper = code.trim().to_uppercase();
        if !upper.is_empty() {
            client_code_for_project = upper.clone();
            match airtable_find_client_by_code(&upper).await {
                Ok(Some(record_id)) => {
                    fields["client"] = serde_json::json!([record_id]);
                }
                Ok(None) => {
                    eprintln!("create_social_post: client {} not found", upper);
                }
                Err(e) => {
                    eprintln!("create_social_post: client lookup failed: {}", e);
                }
            }
        }
    }

    let record_id = airtable_create_record("SocialPosts", fields).await?;

    // Build + file a Receipt for traceability.
    let receipt_project = if client_code_for_project.is_empty() {
        "INC".to_string()
    } else {
        client_code_for_project
    };
    let receipt_id = format!("rcpt_{}", now.format("%Y-%m-%d_%H-%M-%S"));
    let mut items = vec![
        serde_json::json!({ "qty": "✓", "text": format!("Filed to SocialPosts ({})", record_id) }),
        serde_json::json!({ "qty": "1", "text": format!("Status: {}", status) }),
        serde_json::json!({ "qty": "1", "text": format!("Approval: {}", approval) }),
    ];
    if let Some(p) = parsed.platform.as_deref().filter(|s| !s.trim().is_empty()) {
        items.push(serde_json::json!({ "qty": "1", "text": format!("Platform: {}", p) }));
    }
    if let Some(s) = parsed.scheduled_at.as_deref().filter(|s| !s.trim().is_empty()) {
        items.push(serde_json::json!({ "qty": "1", "text": format!("Scheduled: {}", s) }));
    }
    let sections = serde_json::json!([{ "items": items }]);
    let receipt_json = stamp_autonomy_level(
        &build_pure_receipt(
            &receipt_id,
            "schedule-social-post",
            &format!("RECEIPT — SOCIAL POST · {}", title),
            &receipt_project,
            sections,
        ),
        "L2",
    );

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &receipt_json)?;
    }
    file_receipt_to_airtable(&receipt_json).await;

    Ok(record_id)
}

#[derive(Deserialize, Debug)]
struct TimeLogPayload {
    date: Option<String>,
    hours: f64,
    subcontractor_code: Option<String>,
    client_code: Option<String>,
    project_code: Option<String>,
    task_description: Option<String>,
    billable: Option<bool>,
    rate: Option<f64>,
    notes: Option<String>,
}

// Find Subcontractors record by code.
async fn airtable_find_subcontractor_by_code(code: &str) -> Result<Option<String>, String> {
    if code.is_empty() {
        return Ok(None);
    }
    let escaped = code.replace('\'', "");
    let formula = format!("{{code}}='{}'", escaped);
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=code",
        urlencode(&formula)
    );
    let data = airtable_get("Subcontractors", &qs).await?;
    Ok(data["records"][0]["id"].as_str().map(String::from))
}

// Find Projects record by code.
async fn airtable_find_project_by_code(code: &str) -> Result<Option<String>, String> {
    if code.is_empty() {
        return Ok(None);
    }
    let escaped = code.replace('\'', "");
    let formula = format!("{{code}}='{}'", escaped);
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=code",
        urlencode(&formula)
    );
    let data = airtable_get("Projects", &qs).await?;
    Ok(data["records"][0]["id"].as_str().map(String::from))
}

#[tauri::command]
async fn create_time_log(
    payload: serde_json::Value,
    state: State<'_, DbState>,
) -> Result<String, String> {
    let parsed: TimeLogPayload = serde_json::from_value(payload)
        .map_err(|e| format!("Invalid payload: {}", e))?;

    if parsed.hours <= 0.0 {
        return Err("Hours must be greater than 0".to_string());
    }
    if parsed.hours > 24.0 {
        return Err("Hours can't exceed 24 in one log".to_string());
    }

    let now = chrono::Local::now();
    let log_id = format!("tl_{}", now.format("%Y-%m-%d_%H-%M-%S"));
    let date = parsed
        .date
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| now.format("%Y-%m-%d").to_string());

    let mut fields = serde_json::json!({
        "id": log_id,
        "date": date,
        "hours": parsed.hours,
        "billable": parsed.billable.unwrap_or(true),
    });
    if let Some(t) = parsed.task_description.as_deref() {
        if !t.trim().is_empty() {
            fields["task_description"] = serde_json::Value::String(t.trim().to_string());
        }
    }
    if let Some(r) = parsed.rate {
        fields["rate"] = serde_json::json!(r);
    }
    if let Some(n) = parsed.notes.as_deref() {
        if !n.trim().is_empty() {
            fields["notes"] = serde_json::Value::String(n.trim().to_string());
        }
    }

    // Link sub by code.
    let mut sub_label = String::new();
    if let Some(code) = parsed.subcontractor_code.as_deref() {
        let upper = code.trim().to_uppercase();
        if !upper.is_empty() {
            sub_label = upper.clone();
            match airtable_find_subcontractor_by_code(&upper).await {
                Ok(Some(record_id)) => {
                    fields["subcontractor"] = serde_json::json!([record_id]);
                }
                Ok(None) => {
                    eprintln!("create_time_log: sub {} not found", upper);
                }
                Err(e) => {
                    eprintln!("create_time_log: sub lookup failed: {}", e);
                }
            }
        }
    }

    // Link client by code.
    let mut client_code_for_project = String::new();
    if let Some(code) = parsed.client_code.as_deref() {
        let upper = code.trim().to_uppercase();
        if !upper.is_empty() {
            client_code_for_project = upper.clone();
            match airtable_find_client_by_code(&upper).await {
                Ok(Some(record_id)) => {
                    fields["client"] = serde_json::json!([record_id]);
                }
                Ok(None) => eprintln!("create_time_log: client {} not found", upper),
                Err(e) => eprintln!("create_time_log: client lookup failed: {}", e),
            }
        }
    }

    // Link project by code (optional).
    if let Some(pcode) = parsed.project_code.as_deref() {
        let trimmed = pcode.trim();
        if !trimmed.is_empty() {
            match airtable_find_project_by_code(trimmed).await {
                Ok(Some(record_id)) => {
                    fields["project"] = serde_json::json!([record_id]);
                }
                Ok(None) => eprintln!("create_time_log: project {} not found", trimmed),
                Err(e) => eprintln!("create_time_log: project lookup failed: {}", e),
            }
        }
    }

    let record_id = airtable_create_record("TimeLogs", fields).await?;

    // Receipt.
    let receipt_project = if client_code_for_project.is_empty() {
        "INC".to_string()
    } else {
        client_code_for_project
    };
    let receipt_id = format!("rcpt_{}", now.format("%Y-%m-%d_%H-%M-%S"));
    let title = if sub_label.is_empty() {
        format!("RECEIPT — TIME LOG · {:.2}h", parsed.hours)
    } else {
        format!("RECEIPT — TIME LOG · {} · {:.2}h", sub_label, parsed.hours)
    };
    let mut items = vec![
        serde_json::json!({ "qty": "✓", "text": format!("Filed to TimeLogs ({})", record_id) }),
        serde_json::json!({ "qty": format!("{:.2}", parsed.hours), "text": "Hours logged" }),
        serde_json::json!({ "qty": "1", "text": format!("Date: {}", date) }),
    ];
    if !sub_label.is_empty() {
        items.push(serde_json::json!({ "qty": "1", "text": format!("Who: {}", sub_label) }));
    }
    if let Some(r) = parsed.rate {
        items.push(serde_json::json!({ "qty": "1", "text": format!("Rate: ${:.2}/hr", r) }));
    }
    if let Some(t) = parsed.task_description.as_deref().filter(|s| !s.trim().is_empty()) {
        items.push(serde_json::json!({ "qty": "1", "text": format!("Task: {}", t) }));
    }
    let sections = serde_json::json!([{ "items": items }]);
    let receipt_json = stamp_autonomy_level(
        &build_pure_receipt(
            &receipt_id,
            "log-time",
            &title,
            &receipt_project,
            sections,
        ),
        "L2",
    );

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &receipt_json)?;
    }
    file_receipt_to_airtable(&receipt_json).await;

    Ok(record_id)
}

#[derive(Deserialize, Debug)]
struct ProjectFields {
    name: Option<String>,
    status: Option<String>,
    campaign_type: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    budget_total: Option<f64>,
    notes: Option<String>,
}

// Pull a single Projects row by Airtable record id. Used by update_project
// so we can capture the before-state for the diff in the receipt.
async fn airtable_get_project(record_id: &str) -> Result<serde_json::Value, String> {
    let (api_key, base_id) = read_airtable_creds().ok_or("Airtable not configured")?;
    let url = format!("{}/{}/Projects/{}", AIRTABLE_API, base_id, record_id);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let resp = client
        .get(&url)
        .bearer_auth(&api_key)
        .send()
        .await
        .map_err(|e| format!("Network: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Airtable {}: {}", status.as_u16(), body));
    }
    resp.json().await.map_err(|e| format!("Parse: {}", e))
}

#[tauri::command]
async fn update_project(
    record_id: String,
    fields: serde_json::Value,
    state: State<'_, DbState>,
) -> Result<(), String> {
    if record_id.trim().is_empty() {
        return Err("Missing record_id".to_string());
    }
    let parsed: ProjectFields = serde_json::from_value(fields)
        .map_err(|e| format!("Invalid fields: {}", e))?;

    // Capture the before-state so we can diff into the receipt.
    let before = airtable_get_project(&record_id).await?;
    let before_fields = before["fields"].clone();
    let project_code = before_fields["code"].as_str().unwrap_or("").to_string();

    // Build the patch payload, only including fields the user supplied.
    let mut patch = serde_json::Map::new();
    if let Some(v) = parsed.name.as_deref() {
        if !v.trim().is_empty() {
            patch.insert("name".into(), serde_json::Value::String(v.trim().to_string()));
        }
    }
    if let Some(v) = parsed.status.as_deref() {
        if !v.trim().is_empty() {
            patch.insert("status".into(), serde_json::Value::String(v.trim().to_string()));
        }
    }
    if let Some(v) = parsed.campaign_type.as_deref() {
        if !v.trim().is_empty() {
            patch.insert("type".into(), serde_json::Value::String(v.trim().to_string()));
        }
    }
    if let Some(v) = parsed.start_date.as_deref() {
        let trimmed = v.trim();
        // Allow clearing by sending empty string, but Airtable wants null.
        if trimmed.is_empty() {
            patch.insert("start_date".into(), serde_json::Value::Null);
        } else {
            patch.insert("start_date".into(), serde_json::Value::String(trimmed.to_string()));
        }
    }
    if let Some(v) = parsed.end_date.as_deref() {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            patch.insert("end_date".into(), serde_json::Value::Null);
        } else {
            patch.insert("end_date".into(), serde_json::Value::String(trimmed.to_string()));
        }
    }
    if let Some(v) = parsed.budget_total {
        patch.insert("budget_total".into(), serde_json::json!(v));
    }
    if let Some(v) = parsed.notes.as_deref() {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            patch.insert("notes".into(), serde_json::Value::String(String::new()));
        } else {
            patch.insert("notes".into(), serde_json::Value::String(trimmed.to_string()));
        }
    }

    if patch.is_empty() {
        return Err("No fields to update".to_string());
    }

    let patch_value = serde_json::Value::Object(patch.clone());
    airtable_update_record("Projects", &record_id, patch_value.clone()).await?;

    // Build a diff for the receipt — old → new for each changed field.
    let now = chrono::Local::now();
    let receipt_id = format!("rcpt_{}", now.format("%Y-%m-%d_%H-%M-%S"));
    let receipt_project = if project_code.is_empty() {
        "INC".to_string()
    } else {
        // Project code is like NCT-2026-06-tour-launch — strip to client code.
        project_code.split('-').next().unwrap_or("INC").to_string()
    };

    let mut items = vec![serde_json::json!({
        "qty": "✓",
        "text": format!("Updated project {}", if project_code.is_empty() { record_id.as_str() } else { project_code.as_str() }),
    })];
    for (k, new_v) in patch.iter() {
        let label = match k.as_str() {
            "type" => "campaign_type",
            other => other,
        };
        // Look up the before-state by Airtable's actual field name (`type`,
        // not `campaign_type`).
        let old_v = before_fields.get(k.as_str()).cloned().unwrap_or(serde_json::Value::Null);
        let old_disp = json_for_display(&old_v);
        let new_disp = json_for_display(new_v);
        if old_disp != new_disp {
            items.push(serde_json::json!({
                "qty": "1",
                "text": format!("{}: {} → {}", label, old_disp, new_disp),
            }));
        }
    }

    let sections = serde_json::json!([{ "items": items }]);
    let title = if project_code.is_empty() {
        "RECEIPT — EDIT PROJECT".to_string()
    } else {
        format!("RECEIPT — EDIT PROJECT · {}", project_code)
    };

    // For traceability we also include the raw before/after JSON in the
    // receipt's notes section so the diff is recoverable later.
    let diff_payload = serde_json::json!({
        "record_id": record_id,
        "before": before_fields,
        "patch": patch_value,
    });
    let date = now.format("%Y-%m-%d").to_string();
    let receipt_obj = serde_json::json!({
        "id": receipt_id,
        "project": receipt_project,
        "workflow": "edit-project",
        "title": title,
        "date": date,
        "sections": sections,
        "diff": diff_payload,
    });
    let receipt_json = stamp_autonomy_level(&receipt_obj.to_string(), "L2");

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &receipt_json)?;
    }
    file_receipt_to_airtable(&receipt_json).await;

    Ok(())
}

// Pretty-print a JSON value for the diff display. Keeps strings unquoted,
// nulls as "(empty)", numbers/bools as-is.
fn json_for_display(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "(empty)".to_string(),
        serde_json::Value::String(s) => {
            if s.is_empty() { "(empty)".to_string() } else { s.clone() }
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

// ── Today dashboard reads (v0.18) ──────────────────────────────────────
//
// The Today view is the chief-of-staff dashboard. These commands expose
// raw Airtable list payloads so the JS layer can render whatever shape it
// wants. Filtering / sorting happens here in Rust so we ship Airtable's
// formula language (server-side filtering) rather than pulling everything
// then filtering on the client.

// All commitments where status is open or overdue. Fetches a wide window
// (latest 100 rows by created time) and lets the JS slice into "due today"
// vs "overdue" buckets. Cheap because the Commitments table is small.
#[tauri::command]
async fn list_airtable_commitments() -> Result<String, String> {
    let formula = "OR({status}='open',{status}='overdue')";
    let qs = format!(
        "filterByFormula={}&pageSize=100&fields%5B%5D=id&fields%5B%5D=title&fields%5B%5D=made_at&fields%5B%5D=due_at&fields%5B%5D=next_check_at&fields%5B%5D=status&fields%5B%5D=surface&fields%5B%5D=priority&fields%5B%5D=client&fields%5B%5D=project&fields%5B%5D=notes",
        urlencode(formula)
    );
    let data = airtable_get("Commitments", &qs).await?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_airtable_decisions() -> Result<String, String> {
    let formula = "{status}='open'";
    let qs = format!(
        "filterByFormula={}&pageSize=100&fields%5B%5D=id&fields%5B%5D=title&fields%5B%5D=surfaced_at&fields%5B%5D=due_date&fields%5B%5D=status&fields%5B%5D=decision_type&fields%5B%5D=decision&fields%5B%5D=reasoning&fields%5B%5D=client&fields%5B%5D=project",
        urlencode(formula)
    );
    let data = airtable_get("Decisions", &qs).await?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_airtable_workstreams() -> Result<String, String> {
    let formula = "OR({status}='active',{status}='blocked')";
    let qs = format!(
        "filterByFormula={}&pageSize=100&fields%5B%5D=code&fields%5B%5D=title&fields%5B%5D=description&fields%5B%5D=status&fields%5B%5D=phase&fields%5B%5D=last_touch_at&fields%5B%5D=next_action&fields%5B%5D=blocker&fields%5B%5D=target_completion&fields%5B%5D=client&fields%5B%5D=project",
        urlencode(formula)
    );
    let data = airtable_get("Workstreams", &qs).await?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

// Most recent receipts for a single client (by Airtable record id), capped at
// `limit` rows. Used by the per-client view's "Recent receipts" panel. Sorts
// newest first. Pass an empty string for record_id and you'll get an empty
// page back (no escape needed). Limit is clamped to 1..=50 to keep the call
// cheap.
#[tauri::command]
async fn list_airtable_receipts_for_client(
    record_id: String,
    limit: Option<u32>,
) -> Result<String, String> {
    if record_id.trim().is_empty() {
        return Ok("{\"records\":[]}".to_string());
    }
    let n = limit.unwrap_or(10).clamp(1, 50);
    // {client} is a multipleRecordLinks field. FIND() over its rendered
    // string returns >0 when our record id is one of the linked rows.
    let escaped = record_id.replace('\'', "");
    let formula = format!("FIND('{}',ARRAYJOIN({{client}}))", escaped);
    let qs = format!(
        "filterByFormula={}&pageSize={}&fields%5B%5D=id&fields%5B%5D=title&fields%5B%5D=date&fields%5B%5D=workflow&fields%5B%5D=client&fields%5B%5D=ticked_count&fields%5B%5D=json&sort%5B0%5D%5Bfield%5D=date&sort%5B0%5D%5Bdirection%5D=desc",
        urlencode(&formula),
        n
    );
    let data = airtable_get("Receipts", &qs).await?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

// Receipts where ticked_count is below the total tickable items in the
// JSON payload. We pull the last 14 days and let the JS layer compute
// totals from the JSON since Airtable doesn't store a total_count column.
#[tauri::command]
async fn list_airtable_receipts_recent() -> Result<String, String> {
    // Airtable's IS_AFTER + DATEADD(NOW(), -14, 'days') is a stable filter.
    let formula = "IS_AFTER({date},DATEADD(TODAY(),-14,'days'))";
    let qs = format!(
        "filterByFormula={}&pageSize=100&fields%5B%5D=id&fields%5B%5D=title&fields%5B%5D=date&fields%5B%5D=workflow&fields%5B%5D=client&fields%5B%5D=ticked_count&fields%5B%5D=json&sort%5B0%5D%5Bfield%5D=date&sort%5B0%5D%5Bdirection%5D=desc",
        urlencode(formula)
    );
    let data = airtable_get("Receipts", &qs).await?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

// ── Form URLs (v0.30 Block F) ──────────────────────────────────────────
//
// Six Airtable Forms Caitlin builds once in the Airtable web UI (the API
// doesn't expose form creation). Companion stores their URLs in the
// `CompanionSettings` table and surfaces them as "Send Form" / "Share
// Form" buttons across Today, per-client, per-project, Team and
// Settings. One row per key. value is empty string until Caitlin fills.
//
// Keys:
//   form_lead_intake          — public form on Leads
//   form_discovery_pre_brief  — form on Projects, prefilled with project rec id
//   form_content_approval     — form on SocialPosts, prefilled with post rec id
//   form_post_campaign_feedback — form on Projects (post-wrap)
//   form_subcontractor_intake — form on Subcontractors
//   form_weekly_status        — Rose's weekly status (Receipts or
//                               ContractorStatus)

const COMPANION_SETTINGS_TABLE: &str = "CompanionSettings";
const FORM_URL_KEYS: &[&str] = &[
    "form_lead_intake",
    "form_discovery_pre_brief",
    "form_content_approval",
    "form_post_campaign_feedback",
    "form_subcontractor_intake",
    "form_weekly_status",
];

fn validate_form_key(key: &str) -> Result<(), String> {
    if FORM_URL_KEYS.contains(&key) {
        Ok(())
    } else {
        Err(format!("Unknown form key: {}", key))
    }
}

// Find the CompanionSettings row for a given key. Returns the Airtable
// record id and the current value (or None if the row doesn't exist).
async fn airtable_find_settings_row(
    key: &str,
) -> Result<Option<(String, String, Option<String>)>, String> {
    let escaped = key.replace('\'', "");
    let formula = format!("{{key}}='{}'", escaped);
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=key&fields%5B%5D=value&fields%5B%5D=updated_at",
        urlencode(&formula)
    );
    let data = airtable_get(COMPANION_SETTINGS_TABLE, &qs).await?;
    let rec = &data["records"][0];
    let id = rec["id"].as_str();
    if id.is_none() {
        return Ok(None);
    }
    let value = rec["fields"]["value"].as_str().unwrap_or("").to_string();
    let updated_at = rec["fields"]["updated_at"].as_str().map(String::from);
    Ok(Some((id.unwrap().to_string(), value, updated_at)))
}

#[tauri::command]
async fn get_form_url(key: String) -> Result<Option<String>, String> {
    validate_form_key(&key)?;
    match airtable_find_settings_row(&key).await? {
        Some((_, value, _)) if !value.trim().is_empty() => Ok(Some(value)),
        _ => Ok(None),
    }
}

#[tauri::command]
async fn set_form_url(key: String, value: String) -> Result<(), String> {
    validate_form_key(&key)?;
    let trimmed = value.trim();
    // Loose validation. Empty value clears the URL. Otherwise expect an
    // https Airtable form URL — we don't pin the host strictly because
    // Airtable has shipped a few form host variants over the years.
    if !trimmed.is_empty() && !trimmed.starts_with("https://") {
        return Err("Form URL must start with https://".to_string());
    }
    let now = chrono::Local::now().to_rfc3339();
    let fields = serde_json::json!({
        "value": trimmed,
        "updated_at": now,
    });
    match airtable_find_settings_row(&key).await? {
        Some((record_id, _, _)) => {
            airtable_update_record(COMPANION_SETTINGS_TABLE, &record_id, fields).await
        }
        None => {
            // Brand-new row. Should only happen if Caitlin's base is
            // missing the seeded row (e.g. she ran v0.30 against a base
            // without CompanionSettings).
            let mut create_fields = fields.clone();
            create_fields["key"] = serde_json::Value::String(key);
            airtable_create_record(COMPANION_SETTINGS_TABLE, create_fields)
                .await
                .map(|_| ())
        }
    }
}

#[tauri::command]
async fn list_form_urls() -> Result<String, String> {
    // One Airtable list call, then filter to known keys client-side. The
    // table only ever holds six rows so filterByFormula/key-by-key reads
    // would be more expensive.
    let qs = "pageSize=50&fields%5B%5D=key&fields%5B%5D=value&fields%5B%5D=updated_at";
    let data = airtable_get(COMPANION_SETTINGS_TABLE, qs).await?;
    let mut by_key: HashMap<String, serde_json::Value> = HashMap::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            if let Some(k) = r["fields"]["key"].as_str() {
                by_key.insert(
                    k.to_string(),
                    serde_json::json!({
                        "key": k,
                        "value": r["fields"]["value"].as_str().unwrap_or(""),
                        "updated_at": r["fields"]["updated_at"].as_str(),
                    }),
                );
            }
        }
    }
    // Return rows in canonical FORM_URL_KEYS order so the UI doesn't
    // need to sort and missing keys still slot in cleanly.
    let mut out = Vec::with_capacity(FORM_URL_KEYS.len());
    for k in FORM_URL_KEYS {
        if let Some(v) = by_key.remove(*k) {
            out.push(v);
        } else {
            out.push(serde_json::json!({
                "key": k,
                "value": "",
                "updated_at": null,
            }));
        }
    }
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

// Generic Airtable record patch from JS. Used by the commitment-detail and
// decision-capture modals. JS sends table name + record id + fields object,
// we relay to Airtable. Restricts table names to the four chief-of-staff
// tables to keep the surface narrow.
#[derive(Deserialize)]
struct UpdateAirtableArgs {
    table: String,
    record_id: String,
    fields: serde_json::Value,
}

#[tauri::command]
async fn update_airtable_record(args: UpdateAirtableArgs) -> Result<(), String> {
    let allowed = ["Commitments", "Decisions", "Workstreams"];
    if !allowed.contains(&args.table.as_str()) {
        return Err(format!("Table not allowed for update: {}", args.table));
    }
    if args.record_id.is_empty() {
        return Err("Missing record_id".to_string());
    }
    airtable_update_record(&args.table, &args.record_id, args.fields).await
}

// Studio version comes from Cargo's CARGO_PKG_VERSION (single source of
// truth — bumped via Cargo.toml + tauri.conf.json + package.json each
// release). Returning it from the backend means JS doesn't need to fetch
// the conf file at runtime.
#[tauri::command]
fn get_studio_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// HEAD request to a URL, returns 200 if OK or "down" otherwise. Used for
// contextfor.me uptime check from JS without CSP wrangling. Only allows
// https URLs so this isn't a generic SSRF tool.
#[tauri::command]
async fn check_url_up(url: String) -> Result<bool, String> {
    if !url.starts_with("https://") {
        return Err("Only https URLs allowed".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    match client.head(&url).send().await {
        Ok(resp) => Ok(resp.status().is_success()),
        Err(_) => Ok(false),
    }
}

// GitHub Actions latest run on main. Returns conclusion + status + url.
// Reads GITHUB_TOKEN env var if available so we don't hit anonymous
// rate limits, otherwise unauthenticated (60/hr is fine for a desktop app).
#[tauri::command]
async fn check_github_actions(repo: String) -> Result<String, String> {
    if !repo.contains('/') || repo.contains("..") {
        return Err("Invalid repo (expected owner/name)".to_string());
    }
    let url = format!(
        "https://api.github.com/repos/{}/actions/runs?branch=main&per_page=1",
        repo
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("in-cahoots-studio")
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let mut req = client.get(&url).header("Accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }
    let resp = req.send().await.map_err(|e| format!("Network: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub {}: {}", resp.status().as_u16(), repo));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| format!("Parse: {}", e))?;
    let run = &data["workflow_runs"][0];
    let conclusion = run["conclusion"].as_str().unwrap_or("none").to_string();
    let status = run["status"].as_str().unwrap_or("none").to_string();
    let html_url = run["html_url"].as_str().unwrap_or("").to_string();
    let updated_at = run["updated_at"].as_str().unwrap_or("").to_string();
    let out = serde_json::json!({
        "conclusion": conclusion,
        "status": status,
        "url": html_url,
        "updated_at": updated_at,
    });
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

// Read the latest morning briefing log. Looks under
// ~/.claude/scheduled-tasks/ for any subdir whose name contains
// "morning-briefing" then reads the newest log file. Returns the body or
// an empty string if there's nothing yet.
#[tauri::command]
fn read_morning_briefing() -> Result<String, String> {
    let home = home_dir()?;
    let root = format!("{}/.claude/scheduled-tasks", home);
    let dir = match std::fs::read_dir(&root) {
        Ok(d) => d,
        Err(_) => return Ok(String::new()),
    };
    let mut briefing_dirs: Vec<std::path::PathBuf> = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.contains("morning") && name.contains("briefing") {
            briefing_dirs.push(entry.path());
        }
    }
    let mut latest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for d in briefing_dirs {
        // Look for files inside (logs, runs, output.txt, etc).
        let inner = match std::fs::read_dir(&d) {
            Ok(i) => i,
            Err(_) => continue,
        };
        let mut stack: Vec<std::path::PathBuf> = inner.flatten().map(|e| e.path()).collect();
        while let Some(p) = stack.pop() {
            if p.is_dir() {
                if let Ok(sub) = std::fs::read_dir(&p) {
                    for e in sub.flatten() {
                        stack.push(e.path());
                    }
                }
                continue;
            }
            // Only consider text-ish files.
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !ext.is_empty()
                && ext != "log"
                && ext != "txt"
                && ext != "md"
                && ext != "json"
                && ext != "out"
            {
                continue;
            }
            let modified = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .ok();
            if let Some(m) = modified {
                if latest.as_ref().map(|(t, _)| m > *t).unwrap_or(true) {
                    latest = Some((m, p));
                }
            }
        }
    }
    match latest {
        Some((m, p)) => {
            let body = std::fs::read_to_string(&p).map_err(|e| format!("Read briefing: {}", e))?;
            // Cap at 8KB so a runaway log doesn't stuff the dashboard.
            // Use char-safe truncation so a multibyte UTF-8 boundary near
            // 8192 bytes (emoji, accented chars) doesn't panic the slice.
            let trimmed = if body.len() > 8192 {
                let cut = body
                    .char_indices()
                    .take_while(|(i, _)| *i < 8192)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!("{}\n\n... (truncated)", &body[..cut])
            } else {
                body
            };
            let ts = m
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let out = serde_json::json!({
                "text": trimmed,
                "generated_at": ts,
                "path": p.to_string_lossy().to_string(),
            });
            Ok(out.to_string())
        }
        None => Ok(String::new()),
    }
}

// ── Drift checker ──────────────────────────────────────────────────────
//
// Runs the ten checks defined in `drift.rs` and returns a JSON-serialised
// list. Cached in `DriftCache` for one hour. The dashboard reads from
// the cache on every Today render; the refresh button passes
// `force: true` to flush.
//
// Airtable-bound checks read from the existing list_* helpers so we
// don't duplicate Keychain / HTTP plumbing. If any of those calls fail
// (offline, rotated PAT) we log and skip — the rest of the checks
// still surface.

#[tauri::command]
async fn check_drift(
    cache: State<'_, DriftCache>,
    force: Option<bool>,
) -> Result<String, String> {
    let force = force.unwrap_or(false);

    if !force {
        if let Ok(guard) = cache.0.lock() {
            if let Some((at, items)) = guard.as_ref() {
                if at.elapsed() < DRIFT_TTL {
                    return serde_json::to_string(items)
                        .map_err(|e| format!("Serialise drift cache: {}", e));
                }
            }
        }
    }

    let mut items: Vec<drift::DriftItem> = Vec::new();

    // Pure filesystem checks — fast, run inline.
    items.extend(drift::check_version_stamps());
    items.extend(drift::check_design_system_sync());
    items.extend(drift::check_spec_reconciliation());
    items.extend(drift::check_lumin_references());
    items.extend(drift::check_tally_references());
    items.extend(drift::check_team_workflow_retired());
    items.extend(drift::check_archive_cleanup());

    // Airtable-bound checks — fail soft if any call errors.
    match list_airtable_workstreams().await {
        Ok(raw) => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                items.extend(drift::check_workstreams_stale(&v));
            }
        }
        Err(e) => eprintln!("drift: workstreams fetch failed: {}", e),
    }
    match list_airtable_commitments().await {
        Ok(raw) => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                items.extend(drift::check_overdue_commitments(&v));
            }
        }
        Err(e) => eprintln!("drift: commitments fetch failed: {}", e),
    }
    match list_airtable_decisions().await {
        Ok(raw) => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                items.extend(drift::check_overdue_decisions(&v));
            }
        }
        Err(e) => eprintln!("drift: decisions fetch failed: {}", e),
    }

    let out = serde_json::to_string(&items)
        .map_err(|e| format!("Serialise drift items: {}", e))?;

    if let Ok(mut guard) = cache.0.lock() {
        *guard = Some((Instant::now(), items));
    }

    Ok(out)
}

// ── Receipt CRUD ───────────────────────────────────────────────────────

#[tauri::command]
fn save_receipt(state: State<DbState>, json: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
    persist_receipt(&conn, &json)
}

#[tauri::command]
fn list_receipts(state: State<DbState>, limit: u32) -> Result<Vec<String>, String> {
    let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
    let mut stmt = conn
        .prepare("SELECT json FROM receipts ORDER BY created_at DESC LIMIT ?1")
        .map_err(|e| format!("DB prepare: {}", e))?;
    let rows: Vec<String> = stmt
        .query_map(params![limit], |row| row.get::<_, String>(0))
        .map_err(|e| format!("DB query: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

#[tauri::command]
fn delete_receipt(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
    conn.execute("DELETE FROM receipts WHERE id = ?1", params![id])
        .map_err(|e| format!("DB delete: {}", e))?;
    Ok(())
}

#[tauri::command]
async fn tick_item(
    state: State<'_, DbState>,
    receipt_id: String,
    item_index: usize,
) -> Result<String, String> {
    // Load existing JSON from DB (sync, lock dropped before any await).
    let original_json = {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT json FROM receipts WHERE id = ?1")
            .map_err(|e| format!("DB prepare: {}", e))?;
        stmt.query_row(params![receipt_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Receipt {} not found: {}", receipt_id, e))?
    };

    // Mutate the addressed item.done = true.
    let mut value: serde_json::Value =
        serde_json::from_str(&original_json).map_err(|e| format!("Parse: {}", e))?;

    let mut hook_target: Option<String> = None;
    let mut item_text = String::new();
    let mut found = false;

    {
        let title = value["title"].as_str().unwrap_or("Receipt").to_string();
        let mut idx = 0usize;
        if let Some(sections) = value["sections"].as_array_mut() {
            'outer: for section in sections.iter_mut() {
                if let Some(items) = section["items"].as_array_mut() {
                    for item in items.iter_mut() {
                        if idx == item_index {
                            if item["type"].as_str() == Some("task") {
                                item["done"] = serde_json::Value::Bool(true);
                                if let Some(t) = item["text"].as_str() {
                                    item_text = format!("{} (from {})", t, title);
                                }
                                if let Some(h) = item["on_done"].as_str() {
                                    hook_target = Some(h.to_string());
                                }
                                found = true;
                                break 'outer;
                            }
                        }
                        idx += 1;
                    }
                }
            }
        }
    }

    if !found {
        return Err(format!(
            "Item {} not found or not a task in receipt {}",
            item_index, receipt_id
        ));
    }

    // Save updated JSON back.
    let new_json = serde_json::to_string(&value).map_err(|e| format!("Serialize: {}", e))?;
    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        conn.execute(
            "UPDATE receipts SET json = ?1 WHERE id = ?2",
            params![new_json, receipt_id],
        )
        .map_err(|e| format!("DB update: {}", e))?;
    }

    // Update Airtable Receipts row's ticked_count + posted_to_slack flag
    // (best-effort, doesn't fail the tick if Airtable is unreachable).
    let updated_value: serde_json::Value =
        serde_json::from_str(&new_json).unwrap_or(serde_json::Value::Null);
    let new_ticked = count_ticked(&updated_value);

    // Fire on_done hook (best-effort) and capture whether Slack post fired.
    let mut slack_fired = false;
    if let Some(target) = hook_target {
        if let Some(channel) = target.strip_prefix("slack:") {
            if let Some(webhook) = read_slack_webhook() {
                let msg = format!(":white_check_mark: *{}*\nposted to {}", item_text, channel);
                match post_to_slack(&webhook, &msg).await {
                    Ok(_) => slack_fired = true,
                    Err(e) => eprintln!("Slack post failed: {}", e),
                }
            }
        }
    }

    // Best-effort Airtable sync.
    match airtable_find_receipt_record_id(&receipt_id).await {
        Ok(Some(airtable_record_id)) => {
            let mut update_fields = serde_json::json!({ "ticked_count": new_ticked });
            if slack_fired {
                update_fields["posted_to_slack"] = serde_json::Value::Bool(true);
            }
            if let Err(e) =
                airtable_update_record("Receipts", &airtable_record_id, update_fields).await
            {
                eprintln!("tick_item: Airtable update failed: {}", e);
            }
        }
        Ok(None) => {
            // Receipt not yet in Airtable (offline at creation, or filing
            // failed earlier). Skip silently.
        }
        Err(e) => {
            eprintln!("tick_item: Airtable lookup failed: {}", e);
        }
    }

    Ok(new_json)
}

// ── Munbyn reprint ─────────────────────────────────────────────────────

fn template_for_workflow(workflow: &str) -> &'static str {
    match workflow {
        "strategic-thinking" => "thermal/strategic-receipt.typ",
        // Default to the strategic receipt layout until other workflows
        // get their own templates.
        _ => "thermal/strategic-receipt.typ",
    }
}

// macOS apps launched via Finder get a stripped PATH (/usr/bin:/bin only).
// Augment so typst (Homebrew) and the print script's helpers are findable.
fn enriched_path() -> String {
    let existing = std::env::var("PATH").unwrap_or_default();
    format!(
        "/opt/homebrew/bin:/usr/local/bin:/opt/homebrew/sbin:/usr/local/sbin:{}",
        existing
    )
}

#[tauri::command]
async fn reprint_receipt(state: State<'_, DbState>, receipt_id: String) -> Result<(), String> {
    // Load receipt JSON + workflow from DB.
    let (json, workflow) = {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        conn.query_row(
            "SELECT json, workflow FROM receipts WHERE id = ?1",
            params![receipt_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|e| format!("Receipt {} not found: {}", receipt_id, e))?
    };

    let typst_dir = typst_dir()?;
    let print_script = print_script()?;

    // Bail early on installs without the templates folder.
    if !std::path::Path::new(&typst_dir).exists() {
        return Err(
            "Receipt templates not on this machine. Reprint runs on installs that have the In Cahoots Dropbox synced."
                .to_string(),
        );
    }

    // Write the receipt JSON into the typst data folder. Hidden filename
    // so it doesn't pollute the templates registry.
    let data_path = format!("{}/data/.studio-reprint.json", typst_dir);
    std::fs::write(&data_path, &json).map_err(|e| format!("Write temp data: {}", e))?;

    let template = template_for_workflow(&workflow);
    let safe_id = receipt_id.replace([':', '/', ' '], "-");
    let pdf_path = format!("/tmp/studio-reprint-{}.pdf", safe_id);
    let path = enriched_path();

    // Compile typst PDF from the typst dir as project root.
    let typst_out = tokio::process::Command::new("typst")
        .env("PATH", &path)
        .args([
            "compile",
            "--root",
            ".",
            template,
            &pdf_path,
            "--input",
            "datafile=../data/.studio-reprint.json",
        ])
        .current_dir(&typst_dir)
        .output()
        .await
        .map_err(|e| format!("typst spawn (is typst installed via Homebrew?): {}", e))?;
    if !typst_out.status.success() {
        let stderr = String::from_utf8_lossy(&typst_out.stderr);
        return Err(format!("typst compile failed: {}", stderr));
    }

    // Send to Munbyn via the existing thermal print script.
    let print_out = tokio::process::Command::new("bash")
        .env("PATH", &path)
        .args([&print_script, &pdf_path])
        .output()
        .await
        .map_err(|e| format!("print spawn: {}", e))?;
    if !print_out.status.success() {
        let stderr = String::from_utf8_lossy(&print_out.stderr);
        return Err(format!("print failed: {}", stderr));
    }

    Ok(())
}

// ── Granola integration (v0.22, Block C) ───────────────────────────────
//
// Three Tauri commands wrap the OAuth + MCP plumbing for the frontend:
//
//   - `get_granola_status()`  → { connected, has_client_id, last_pull_at }
//   - `save_granola_client_id(client_id)` → ()
//   - `connect_granola()`     → () — runs the browser OAuth flow.
//   - `disconnect_granola()`  → () — wipes Keychain entries.
//   - `pull_granola_transcripts(client_code, since_days)` → String
//        Calls the Anthropic Messages API with Granola's MCP server
//        attached, asks Claude to fetch transcripts for the named
//        client over the window, and returns a plain-text bundle the
//        frontend appends to the Monthly Check-in / Quarterly Review
//        textarea.
//
// First-call OAuth UX: if `pull_granola_transcripts` is invoked while
// disconnected, it returns a structured error the frontend uses to
// nudge the user to Settings → Connect Granola. We don't auto-trigger
// the browser flow from here because the call may have come from a
// modal action mid-typing and Caitlin shouldn't lose her place.

fn granola_extra() -> serde_json::Value {
    serde_json::json!({
        "has_client_id": oauth::read_keychain(granola::GRANOLA.client_id_keychain_key).is_some(),
        "last_pull_at": oauth::read_keychain(KEYRING_GRANOLA_LAST_PULL),
        // Backwards-compat: older callers (today/fetch.js, source-picker)
        // checked status.connected. Mirror it under extra so they keep
        // working until they migrate to status.state === "verified".
        "connected": oauth::is_connected(&granola::GRANOLA),
    })
}

#[tauri::command]
fn get_granola_status() -> IntegrationStatus {
    build_status_from_cache(
        oauth::is_connected(&granola::GRANOLA),
        KEYRING_GRANOLA_LAST_VERIFIED_AT,
        KEYRING_GRANOLA_LAST_ERROR,
        granola_extra(),
    )
}

#[tauri::command]
fn save_granola_client_id(client_id: String) -> Result<(), String> {
    let trimmed = client_id.trim();
    if trimmed.is_empty() {
        return Err("Empty client ID".to_string());
    }
    oauth::write_keychain(granola::GRANOLA.client_id_keychain_key, trimmed)
}

#[tauri::command]
async fn connect_granola() -> Result<IntegrationStatus, String> {
    oauth::start_oauth_flow(&granola::GRANOLA).await?;
    // Verify immediately after the OAuth callback so Settings can
    // render a fresh "verified just now" stamp without waiting for the
    // first pull. Failure here is rare (we just successfully exchanged
    // a code) but we still want to surface it if it happens.
    match verify_granola().await {
        Ok(()) => record_verification_success(
            KEYRING_GRANOLA_LAST_VERIFIED_AT,
            KEYRING_GRANOLA_LAST_ERROR,
        ),
        Err(reason) => record_verification_failure(
            KEYRING_GRANOLA_LAST_VERIFIED_AT,
            KEYRING_GRANOLA_LAST_ERROR,
            &reason,
        ),
    }
    Ok(get_granola_status())
}

#[tauri::command]
fn disconnect_granola() -> Result<(), String> {
    oauth::disconnect(&granola::GRANOLA)?;
    let _ = oauth::delete_keychain(KEYRING_GRANOLA_LAST_VERIFIED_AT);
    let _ = oauth::delete_keychain(KEYRING_GRANOLA_LAST_ERROR);
    invalidate_cached_secret(KEYRING_GRANOLA_LAST_VERIFIED_AT);
    invalidate_cached_secret(KEYRING_GRANOLA_LAST_ERROR);
    Ok(())
}

#[derive(Deserialize, Debug)]
struct PullGranolaInput {
    client_code: String,
    #[serde(default = "default_pull_window_days")]
    since_days: i64,
    // Optional: human-readable client name to pass into the prompt.
    // Saves an Airtable round-trip when the frontend already knows it.
    #[serde(default)]
    client_name: Option<String>,
}

fn default_pull_window_days() -> i64 {
    30
}

// v0.32 picker helper. Extracted from `pull_granola_transcripts` so the
// source_picker module can reuse the same MCP path with its own
// arguments. Returns the plain-text bundle (no envelope) — caller wraps.
pub(crate) async fn pull_granola_via_mcp(
    client_code: &str,
    since_days: i64,
    client_name: Option<&str>,
) -> Result<String, String> {
    let api_key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    let code = client_code.trim().to_uppercase();
    if code.is_empty() {
        return Err("Pick a client first".to_string());
    }

    let token = oauth::ensure_fresh_token(&granola::GRANOLA)
        .await
        .map_err(|e| format!("Granola: {}", e))?;

    let resolved_name = match client_name {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => {
            let qs = format!(
                "filterByFormula={}&maxRecords=1&fields%5B%5D=name",
                urlencode(&format!("{{code}}='{}'", code))
            );
            let data = airtable_get("Clients", &qs)
                .await
                .unwrap_or(serde_json::Value::Null);
            data["records"][0]["fields"]["name"]
                .as_str()
                .unwrap_or(&code)
                .to_string()
        }
    };

    let mcp_servers = vec![McpServerSpec {
        server_type: "url",
        name: granola::GRANOLA.name.to_string(),
        url: granola::GRANOLA_MCP_URL.to_string(),
        authorization_token: token,
        tool_configuration: Some(McpToolConfiguration {
            enabled: true,
            allowed_tools: Some(
                granola::MONTHLY_CHECKIN_TOOLS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
        }),
    }];

    let user_message = format!(
        "Use the Granola MCP tools to fetch meeting notes and \
         transcripts for the client \"{}\" (client code: {}) over \
         the last {} days.\n\n\
         Workflow:\n\
         1. Call `list_meetings` with a date range covering the last \
            {} days.\n\
         2. Filter to meetings whose title or attendees match the \
            client name above.\n\
         3. For each match, call `get_meeting_transcript`.\n\
         4. Return a plain-text bundle, one section per meeting:\n\n\
         ### YYYY-MM-DD — Meeting title\n\
         (1-2 sentence summary)\n\
         (raw transcript)\n\n\
         If no meetings match, reply with the single line: \
         `No Granola meetings found for {} in the last {} days.` and \
         nothing else.\n\n\
         Do not wrap the response in JSON or markdown fences. Plain \
         text only.",
        resolved_name, code, since_days, since_days, resolved_name, since_days,
    );

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 8192,
        system:
            "You are a faithful retrieval assistant. Use the Granola \
             MCP tools to fetch and return meeting transcripts. Do not \
             summarise or interpret beyond what's asked. Plain text only.",
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: Some(mcp_servers),
    };

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;

    let response = http
        .post(ANTHROPIC_API)
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", ANTHROPIC_MCP_BETA)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic API {}: {}", status.as_u16(), text));
    }

    let api_response: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let text = collect_text_blocks(&api_response.content);
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err(
            "Granola pull came back empty — try again, or paste manually".to_string(),
        );
    }

    let _ = oauth::write_keychain(
        KEYRING_GRANOLA_LAST_PULL,
        &chrono::Utc::now().to_rfc3339(),
    );
    Ok(trimmed)
}

// v0.32 picker helper. Reads a single Gmail thread (id or first match
// for a search expression) and returns it as plain markdown.
pub(crate) async fn fetch_gmail_for_picker(source_ref: &str) -> Result<String, String> {
    let token = oauth::ensure_fresh_token(&google::GOOGLE)
        .await
        .map_err(|e| format!("Google: {}", e))?;

    // Heuristic: thread IDs are hex strings (16-20 chars). Anything with
    // spaces or operators (`from:`, `is:`, etc.) goes through search.
    let looks_like_id =
        source_ref.len() >= 8 && source_ref.chars().all(|c| c.is_ascii_alphanumeric());
    let id = if looks_like_id {
        source_ref.to_string()
    } else {
        // Use a search and grab the first result.
        let q = format!("{}", source_ref);
        let url = format!(
            "{}/users/me/threads?maxResults=1&q={}",
            google::GMAIL_API_BASE,
            urlencode(&q),
        );
        let resp = http_get_json_bearer(&url, &token).await?;
        resp["threads"][0]["id"]
            .as_str()
            .ok_or_else(|| "No Gmail threads matched that search".to_string())?
            .to_string()
    };

    // Pull the full thread body — minimal format gives us all messages
    // with payload/parts so we can extract the plain-text body.
    let url = format!(
        "{}/users/me/threads/{}?format=full",
        google::GMAIL_API_BASE,
        urlencode(&id),
    );
    let val = http_get_json_bearer(&url, &token).await?;
    let messages = val["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if messages.is_empty() {
        return Err("Gmail thread has no messages".to_string());
    }

    let mut subject = String::new();
    let mut out = String::new();
    for (i, m) in messages.iter().enumerate() {
        let mut from = String::new();
        if let Some(headers) = m["payload"]["headers"].as_array() {
            for h in headers {
                let name = h["name"].as_str().unwrap_or("");
                let value = h["value"].as_str().unwrap_or("");
                if name.eq_ignore_ascii_case("Subject") && subject.is_empty() {
                    subject = value.to_string();
                } else if name.eq_ignore_ascii_case("From") {
                    from = value.to_string();
                }
            }
        }
        let body = extract_plain_body(&m["payload"]);
        if i == 0 && !subject.is_empty() {
            out.push_str(&format!("**Subject:** {}\n\n", subject));
        }
        out.push_str(&format!("### Message {} — From: {}\n\n{}\n\n", i + 1, from, body.trim()));
    }
    Ok(out.trim_end().to_string())
}

fn extract_plain_body(payload: &serde_json::Value) -> String {
    // Prefer text/plain part; fall back to snippet.
    if let Some(parts) = payload["parts"].as_array() {
        for p in parts {
            let mime = p["mimeType"].as_str().unwrap_or("");
            if mime == "text/plain" {
                if let Some(data) = p["body"]["data"].as_str() {
                    if let Some(decoded) = decode_base64url(data) {
                        return decoded;
                    }
                }
            }
        }
        // Recurse into multipart parts.
        for p in parts {
            let nested = extract_plain_body(p);
            if !nested.is_empty() {
                return nested;
            }
        }
    }
    if let Some(data) = payload["body"]["data"].as_str() {
        if let Some(decoded) = decode_base64url(data) {
            return decoded;
        }
    }
    String::new()
}

fn decode_base64url(s: &str) -> Option<String> {
    // Gmail uses URL-safe base64 with no padding. Tiny inline decoder so
    // we don't need to add a new dep.
    fn b64_index(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let cleaned: Vec<u8> = s
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
        .collect();
    let mut out: Vec<u8> = Vec::with_capacity(cleaned.len() * 3 / 4 + 2);
    let mut buf: u32 = 0;
    let mut bits: u8 = 0;
    for &c in &cleaned {
        let v = b64_index(c)?;
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8 & 0xFF);
        }
    }
    String::from_utf8(out).ok()
}

async fn http_get_json_bearer(
    url: &str,
    token: &str,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let resp = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Network: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status.as_u16(), body));
    }
    resp.json().await.map_err(|e| format!("Parse: {}", e))
}

// v0.32 picker helper. Resolves a Slack permalink to a thread and
// returns the messages as a plain-text bundle.
pub(crate) async fn fetch_slack_thread_for_picker(
    permalink: &str,
) -> Result<String, String> {
    // Permalink format:
    //   https://<ws>.slack.com/archives/<channel_id>/p<ts_no_dot>
    // optionally with ?thread_ts=<root_ts> for replies.
    let trimmed = permalink.trim();
    if !trimmed.starts_with("https://") && !trimmed.starts_with("http://") {
        return Err("Slack URL must start with https://".to_string());
    }
    // Split off query string.
    let (path_part, query_part) = match trimmed.split_once('?') {
        Some((p, q)) => (p, q),
        None => (trimmed, ""),
    };
    // Drop the scheme + host, keep the path.
    let after_scheme = path_part
        .splitn(4, '/')
        .nth(3)
        .ok_or_else(|| "Slack URL too short".to_string())?;
    let segs: Vec<&str> = after_scheme.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 3 || segs[0] != "archives" {
        return Err("Slack URL must be a /archives/<channel>/p<ts> permalink".to_string());
    }
    let channel_id = segs[1].to_string();
    let p_ts = segs[2];
    if !p_ts.starts_with('p') || p_ts.len() < 11 {
        return Err("Slack permalink ts segment looks malformed".to_string());
    }
    // Convert "p1714984823123456" → "1714984823.123456"
    let raw = &p_ts[1..];
    let target_ts = format!("{}.{}", &raw[..raw.len() - 6], &raw[raw.len() - 6..]);

    // If the permalink had ?thread_ts= use that root, else target_ts is
    // likely the root itself.
    let thread_root = query_part
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "thread_ts")
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| target_ts.clone());

    // Reuse list_recent_messages to pull a 168h (7 day) window then
    // filter to the thread by matching ts.
    let mut messages = slack::list_recent_messages(&channel_id, 168)
        .await
        .map_err(|e| format!("Slack: {}", e))?;
    messages.retain(|m| m.ts == thread_root || m.ts == target_ts);
    if messages.is_empty() {
        return Err(
            "Slack thread not found in the last 7 days of channel history".to_string(),
        );
    }
    let mut out = String::new();
    out.push_str(&format!("**Channel id:** {}\n\n", channel_id));
    for m in messages {
        let user = m
            .user_name
            .as_deref()
            .or(m.user.as_deref())
            .unwrap_or("(unknown)");
        out.push_str(&format!("**{}** ({}):\n{}\n\n", user, m.ts, m.text));
    }
    Ok(out.trim_end().to_string())
}

// v0.32 picker helper. Reads a single Calendar event by ID. When the
// `source_ref` looks like a date (YYYY-MM-DD) we fall back to the first
// event on that date.
pub(crate) async fn fetch_calendar_event_for_picker(
    source_ref: &str,
) -> Result<String, String> {
    let token = oauth::ensure_fresh_token(&google::GOOGLE)
        .await
        .map_err(|e| format!("Google: {}", e))?;

    let event_id = if source_ref.len() == 10 && source_ref.chars().filter(|c| *c == '-').count() == 2 {
        // Looks like a date — pull events on that day, take the first.
        let url = format!(
            "{}/calendars/primary/events?timeMin={}T00:00:00Z&timeMax={}T23:59:59Z&singleEvents=true&maxResults=1",
            google::CALENDAR_API_BASE,
            urlencode(source_ref),
            urlencode(source_ref),
        );
        let val = http_get_json_bearer(&url, &token).await?;
        val["items"][0]["id"]
            .as_str()
            .ok_or_else(|| format!("No Calendar events found on {}", source_ref))?
            .to_string()
    } else {
        source_ref.to_string()
    };

    let url = format!(
        "{}/calendars/primary/events/{}",
        google::CALENDAR_API_BASE,
        urlencode(&event_id),
    );
    let val = http_get_json_bearer(&url, &token).await?;
    let summary = val["summary"].as_str().unwrap_or("(no title)");
    let description = val["description"].as_str().unwrap_or("(no description)");
    let location = val["location"].as_str().unwrap_or("");
    let start = val["start"]["dateTime"]
        .as_str()
        .or(val["start"]["date"].as_str())
        .unwrap_or("");
    let end = val["end"]["dateTime"]
        .as_str()
        .or(val["end"]["date"].as_str())
        .unwrap_or("");
    let mut attendees: Vec<String> = Vec::new();
    if let Some(arr) = val["attendees"].as_array() {
        for a in arr {
            if let Some(email) = a["email"].as_str() {
                attendees.push(email.to_string());
            }
        }
    }

    let mut out = format!(
        "**{}**\n\nWhen: {} → {}\n",
        summary, start, end
    );
    if !location.is_empty() {
        out.push_str(&format!("Where: {}\n", location));
    }
    if !attendees.is_empty() {
        out.push_str(&format!("Attendees: {}\n", attendees.join(", ")));
    }
    out.push_str(&format!("\n---\n\n{}", description));
    Ok(out)
}

// Tauri command exposed to JS. Routes to source_picker::fetch_workflow_context.
#[tauri::command]
async fn fetch_workflow_context(input: serde_json::Value) -> Result<String, String> {
    let parsed: source_picker::FetchInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    source_picker::fetch_workflow_context(parsed).await
}

#[tauri::command]
async fn pull_granola_transcripts(
    input: serde_json::Value,
    rate_limit: State<'_, RateLimit>,
) -> Result<String, String> {
    check_rate_limit(&rate_limit)?;

    let api_key = read_anthropic_key().ok_or("Anthropic API key not set")?;
    let parsed: PullGranolaInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;

    let code = parsed.client_code.trim().to_uppercase();
    if code.is_empty() {
        return Err("Pick a client first".to_string());
    }

    // Ensure we have a fresh Granola token. ensure_fresh_token surfaces
    // a clean error if Granola isn't connected — frontend turns that
    // into a "Go to Settings → Connect Granola" toast.
    let token = oauth::ensure_fresh_token(&granola::GRANOLA)
        .await
        .map_err(|e| format!("Granola: {}", e))?;

    // Resolve client name. Best-effort — falls back to the code.
    let client_name = match parsed.client_name {
        Some(ref n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => {
            let qs = format!(
                "filterByFormula={}&maxRecords=1&fields%5B%5D=name",
                urlencode(&format!("{{code}}='{}'", code))
            );
            let data = airtable_get("Clients", &qs)
                .await
                .unwrap_or(serde_json::Value::Null);
            data["records"][0]["fields"]["name"]
                .as_str()
                .unwrap_or(&code)
                .to_string()
        }
    };

    // Build the Granola MCP server entry. Allowlist only the two tools
    // we actually need — keeps token cost down per the v0.22 plan.
    let mcp_servers = vec![McpServerSpec {
        server_type: "url",
        name: granola::GRANOLA.name.to_string(),
        url: granola::GRANOLA_MCP_URL.to_string(),
        authorization_token: token,
        tool_configuration: Some(McpToolConfiguration {
            enabled: true,
            allowed_tools: Some(
                granola::MONTHLY_CHECKIN_TOOLS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
        }),
    }];

    // Plain-text response, not the receipt JSON shape — the workflow
    // call later wraps the user's pasted/pulled text into a receipt.
    let user_message = format!(
        "Use the Granola MCP tools to fetch meeting notes and \
         transcripts for the client \"{}\" (client code: {}) over \
         the last {} days.\n\n\
         Workflow:\n\
         1. Call `list_meetings` with a date range covering the last \
            {} days.\n\
         2. Filter to meetings whose title or attendees match the \
            client name above.\n\
         3. For each match, call `get_meeting_transcript`.\n\
         4. Return a plain-text bundle, one section per meeting:\n\n\
         ### YYYY-MM-DD — Meeting title\n\
         (1-2 sentence summary)\n\
         (raw transcript)\n\n\
         If no meetings match, reply with the single line: \
         `No Granola meetings found for {} in the last {} days.` and \
         nothing else.\n\n\
         Do not wrap the response in JSON or markdown fences. Plain \
         text only — Studio will paste this verbatim into a textarea.",
        client_name, code, parsed.since_days, parsed.since_days,
        client_name, parsed.since_days,
    );

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 8192,
        system:
            "You are a faithful retrieval assistant. Use the Granola \
             MCP tools to fetch and return meeting transcripts. Do not \
             summarise or interpret beyond what's asked. Plain text only.",
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
        mcp_servers: Some(mcp_servers),
    };

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;

    let response = http
        .post(ANTHROPIC_API)
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", ANTHROPIC_MCP_BETA)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic API {}: {}", status.as_u16(), text));
    }

    let api_response: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    let text = collect_text_blocks(&api_response.content);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(
            "Granola pull came back empty — try again, or paste manually".to_string(),
        );
    }

    // Stamp last-pull-at for the Settings indicator. Best-effort.
    let _ = oauth::write_keychain(
        KEYRING_GRANOLA_LAST_PULL,
        &chrono::Utc::now().to_rfc3339(),
    );

    Ok(trimmed.to_string())
}

// ── Google Calendar integration (v0.24, Block C) ───────────────────────
//
// Same OAuth + Keychain pattern as Granola. Calendar reads run direct
// against Google's REST API from Rust — Anthropic's MCP connector is
// reserved for v0.25+ workflows that need calendar reasoning, not for
// the dashboard's raw event list.
//
// Commands exposed to the frontend:
//
//   - get_google_status()        -> { connected, has_client_id, last_sync_at }
//   - save_google_client_id(id)  -> ()
//   - connect_google()           -> () — runs the browser PKCE flow.
//   - disconnect_google()        -> () — wipes Keychain entries.
//   - list_calendar_today()      -> JSON array of CalendarEvent
//   - list_calendar_week()       -> JSON array of CalendarEvent
//   - list_calendar_for_client(client_code) -> JSON array of CalendarEvent
//
// All three list_* commands return a JSON-stringified array so the
// frontend can `JSON.parse` it into an array of plain objects without
// needing to mirror the Rust struct shape. Empty arrays come back when
// nothing matches — render layer handles empty-state messaging.

fn google_extra() -> serde_json::Value {
    let connected = oauth::is_connected(&google::GOOGLE);
    let scopes: Vec<String> = if connected {
        match oauth::read_keychain(KEYRING_GOOGLE_GRANTED_SCOPES) {
            Some(s) if !s.is_empty() => s
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
            // No granted-scope record means the token was minted under
            // v0.24 (Calendar only). The user needs a re-authorise to
            // upgrade — Settings copy nudges them.
            _ => vec!["calendar".to_string()],
        }
    } else {
        Vec::new()
    };
    serde_json::json!({
        "has_client_id": oauth::read_keychain(google::GOOGLE.client_id_keychain_key).is_some(),
        "last_sync_at": oauth::read_keychain(KEYRING_GOOGLE_LAST_SYNC),
        "scopes": scopes,
        // Backwards-compat for today/fetch.js + client/fetch.js +
        // source-picker which still read status.connected and
        // status.scopes off the top level.
        "connected": connected,
    })
}

#[tauri::command]
fn get_google_status() -> IntegrationStatus {
    build_status_from_cache(
        oauth::is_connected(&google::GOOGLE),
        KEYRING_GOOGLE_LAST_VERIFIED_AT,
        KEYRING_GOOGLE_LAST_ERROR,
        google_extra(),
    )
}

#[tauri::command]
fn save_google_client_id(client_id: String) -> Result<(), String> {
    let trimmed = client_id.trim();
    if trimmed.is_empty() {
        return Err("Empty client ID".to_string());
    }
    oauth::write_keychain(google::GOOGLE.client_id_keychain_key, trimmed)
}

#[tauri::command]
async fn connect_google() -> Result<IntegrationStatus, String> {
    oauth::start_oauth_flow(&google::GOOGLE).await?;
    // Record which surfaces this token covers. v0.25 always asks for
    // Calendar + Gmail + Drive — if the user re-authorises we overwrite
    // the v0.24 marker that says Calendar only.
    let _ = oauth::write_keychain(
        KEYRING_GOOGLE_GRANTED_SCOPES,
        "calendar,gmail,drive",
    );
    // Verify immediately after OAuth so Settings shows a fresh stamp.
    match verify_google().await {
        Ok(()) => record_verification_success(
            KEYRING_GOOGLE_LAST_VERIFIED_AT,
            KEYRING_GOOGLE_LAST_ERROR,
        ),
        Err(reason) => record_verification_failure(
            KEYRING_GOOGLE_LAST_VERIFIED_AT,
            KEYRING_GOOGLE_LAST_ERROR,
            &reason,
        ),
    }
    Ok(get_google_status())
}

#[tauri::command]
fn disconnect_google() -> Result<(), String> {
    oauth::disconnect(&google::GOOGLE)?;
    let _ = oauth::delete_keychain(KEYRING_GOOGLE_LAST_SYNC);
    let _ = oauth::delete_keychain(KEYRING_GOOGLE_GRANTED_SCOPES);
    let _ = oauth::delete_keychain(KEYRING_GOOGLE_LAST_VERIFIED_AT);
    let _ = oauth::delete_keychain(KEYRING_GOOGLE_LAST_ERROR);
    invalidate_cached_secret(KEYRING_GOOGLE_LAST_VERIFIED_AT);
    invalidate_cached_secret(KEYRING_GOOGLE_LAST_ERROR);
    Ok(())
}

fn stamp_google_last_sync() {
    let _ = oauth::write_keychain(
        KEYRING_GOOGLE_LAST_SYNC,
        &chrono::Utc::now().to_rfc3339(),
    );
}

#[tauri::command]
async fn list_calendar_today() -> Result<String, String> {
    let events = calendar::list_events_today().await?;
    stamp_google_last_sync();
    serde_json::to_string(&events).map_err(|e| format!("Serialise: {}", e))
}

#[tauri::command]
async fn list_calendar_week() -> Result<String, String> {
    let events = calendar::list_events_week().await?;
    stamp_google_last_sync();
    serde_json::to_string(&events).map_err(|e| format!("Serialise: {}", e))
}

#[derive(Deserialize, Debug)]
struct ListCalendarForClientInput {
    client_code: String,
    #[serde(default)]
    client_name: Option<String>,
    // Optional override of the matching aliases. When None, the Rust
    // side reads `gmail_thread_filter` off the Clients table for this
    // client code.
    #[serde(default)]
    aliases: Option<Vec<String>>,
}

#[tauri::command]
async fn list_calendar_for_client(input: serde_json::Value) -> Result<String, String> {
    let parsed: ListCalendarForClientInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let code = parsed.client_code.trim().to_uppercase();
    if code.is_empty() {
        return Err("Pick a client first".to_string());
    }

    // Resolve client name + alias hints. Best-effort — if Airtable is
    // down we fall back to the code as the only needle.
    let (name, alias_hint) = match parsed.client_name {
        Some(ref n) if !n.trim().is_empty() => (n.trim().to_string(), parsed.aliases.clone()),
        _ => {
            let qs = format!(
                "filterByFormula={}&maxRecords=1&fields%5B%5D=name&fields%5B%5D=gmail_thread_filter",
                urlencode(&format!("{{code}}='{}'", code))
            );
            let data = airtable_get("Clients", &qs)
                .await
                .unwrap_or(serde_json::Value::Null);
            let n = data["records"][0]["fields"]["name"]
                .as_str()
                .unwrap_or(&code)
                .to_string();
            let alias = data["records"][0]["fields"]["gmail_thread_filter"]
                .as_str()
                .map(|s| vec![s.to_string()]);
            (n, parsed.aliases.or(alias))
        }
    };

    let aliases = alias_hint.unwrap_or_default();
    let events = calendar::list_events_for_client(&name, &aliases).await?;
    stamp_google_last_sync();
    serde_json::to_string(&events).map_err(|e| format!("Serialise: {}", e))
}

// ── Gmail integration (v0.25, Block C) ─────────────────────────────────
//
// Three frontend-facing commands wrap the Gmail v1 REST plumbing. All
// fail soft when Google isn't connected — the calling render layer hides
// the email triage section in that case rather than throwing.
//
//   - gmail_unread_count()             -> u32
//   - list_gmail_urgent()              -> JSON array of EmailThread
//   - list_gmail_for_client(code, ..)  -> JSON array of EmailThread
//
// The "urgent" heuristic (unread + starred OR sender in primary contacts)
// needs the Clients table's primary_contact_email values. We fetch them
// inside the command rather than asking the frontend to pass them — keeps
// the call surface tight and lets the dashboard refresh cheaply.

#[tauri::command]
async fn gmail_unread_count() -> Result<u32, String> {
    let count = gmail::count_unread().await?;
    stamp_google_last_sync();
    Ok(count)
}

async fn fetch_important_sender_emails() -> Vec<String> {
    // Pull active clients' primary_contact_email values. Best-effort —
    // when Airtable is down we just return an empty list and the
    // urgent-thread loop falls back to "starred only".
    let raw = match list_airtable_clients().await {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    if let Some(records) = parsed["records"].as_array() {
        for rec in records {
            if let Some(email) = rec["fields"]["primary_contact_email"].as_str() {
                let trimmed = email.trim().to_lowercase();
                if !trimmed.is_empty() {
                    out.push(trimmed);
                }
            }
        }
    }
    out
}

#[tauri::command]
async fn list_gmail_urgent() -> Result<String, String> {
    let senders = fetch_important_sender_emails().await;
    let threads = gmail::list_urgent_threads(&senders).await?;
    stamp_google_last_sync();
    serde_json::to_string(&threads).map_err(|e| format!("Serialise: {}", e))
}

#[derive(Deserialize, Debug)]
struct ListGmailForClientInput {
    client_code: String,
    #[serde(default)]
    client_name: Option<String>,
    // Optional override — when set, used verbatim as the Gmail q=
    // expression. Otherwise we read `gmail_thread_filter` off the
    // Clients table for this code and use that.
    #[serde(default)]
    filter_expr: Option<String>,
}

#[tauri::command]
async fn list_gmail_for_client(input: serde_json::Value) -> Result<String, String> {
    let parsed: ListGmailForClientInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let code = parsed.client_code.trim().to_uppercase();
    if code.is_empty() {
        return Err("Pick a client first".to_string());
    }

    // Resolve client name + filter from Airtable when not supplied.
    let (name, filter_expr) = match (parsed.client_name.clone(), parsed.filter_expr.clone()) {
        (Some(n), Some(f)) if !n.trim().is_empty() => (n.trim().to_string(), Some(f)),
        _ => {
            let qs = format!(
                "filterByFormula={}&maxRecords=1&fields%5B%5D=name&fields%5B%5D=gmail_thread_filter",
                urlencode(&format!("{{code}}='{}'", code))
            );
            let data = airtable_get("Clients", &qs)
                .await
                .unwrap_or(serde_json::Value::Null);
            let n = data["records"][0]["fields"]["name"]
                .as_str()
                .unwrap_or(&code)
                .to_string();
            let f = parsed.filter_expr.or_else(|| {
                data["records"][0]["fields"]["gmail_thread_filter"]
                    .as_str()
                    .map(String::from)
            });
            (parsed.client_name.unwrap_or(n), f)
        }
    };

    let threads = gmail::list_threads_for_client(&name, filter_expr.as_deref()).await?;
    stamp_google_last_sync();
    serde_json::to_string(&threads).map_err(|e| format!("Serialise: {}", e))
}

// ── Drive integration (v0.25, Block C) ─────────────────────────────────
//
// One frontend-facing command. Resolves the client's drive_folder_id
// from Airtable when the caller doesn't supply it, then queries Drive
// v3 for files modified in the last `days` (default 14). Empty when the
// client has no folder ID set — the per-client section hides itself.

#[derive(Deserialize, Debug)]
struct ListDriveForClientInput {
    client_code: String,
    #[serde(default)]
    folder_id: Option<String>,
    #[serde(default = "default_drive_window_days")]
    days: i64,
}

fn default_drive_window_days() -> i64 {
    14
}

#[tauri::command]
async fn list_drive_recent_for_client(input: serde_json::Value) -> Result<String, String> {
    let parsed: ListDriveForClientInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let code = parsed.client_code.trim().to_uppercase();
    if code.is_empty() {
        return Err("Pick a client first".to_string());
    }

    let folder_id = match parsed.folder_id {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            // Look up the client's drive_folder_id field.
            let qs = format!(
                "filterByFormula={}&maxRecords=1&fields%5B%5D=drive_folder_id",
                urlencode(&format!("{{code}}='{}'", code))
            );
            let data = airtable_get("Clients", &qs)
                .await
                .unwrap_or(serde_json::Value::Null);
            data["records"][0]["fields"]["drive_folder_id"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_string()
        }
    };

    if folder_id.is_empty() {
        // No folder configured — the section will hide itself.
        return Ok("[]".to_string());
    }

    let files = drive::list_recent_files_for_client(&folder_id, parsed.days).await?;
    stamp_google_last_sync();
    serde_json::to_string(&files).map_err(|e| format!("Serialise: {}", e))
}

// ── Slack read integration (v0.26, Block C) ────────────────────────────
//
// Mirrors the Granola + Google patterns. The existing Slack webhook
// (KEYRING_SLACK / save_slack_webhook / get_slack_status) is left alone
// — that's still the outbound write path the receipt-tick on_done hook
// uses. v0.26 adds a *separate* OAuth-driven read path:
//
//   - get_slack_oauth_status()       -> { connected, has_client_id, has_client_secret, last_sync_at }
//   - save_slack_oauth_credentials(client_id, client_secret) -> ()
//   - connect_slack()                -> () — runs the browser OAuth flow
//   - disconnect_slack()             -> ()
//   - list_slack_unreads()           -> JSON array of UnreadSummary
//   - list_slack_for_client(slug)    -> JSON ClientChannelActivity | null
//
// Naming: the OAuth commands are *_slack_oauth_* to keep them clearly
// separate from the existing webhook commands (get_slack_status /
// save_slack_webhook). Caitlin can have one or both configured — the
// dashboard will hide the Slack-activity section when OAuth isn't
// connected, and the receipt-tick path will skip the webhook post when
// no webhook URL is set.

fn slack_oauth_extra() -> serde_json::Value {
    serde_json::json!({
        "has_client_id": oauth::read_keychain(slack::SLACK.client_id_keychain_key).is_some(),
        "has_client_secret": slack::SLACK
            .client_secret_keychain_key
            .map(|k| oauth::read_keychain(k).is_some())
            .unwrap_or(false),
        "last_sync_at": oauth::read_keychain(KEYRING_SLACK_LAST_SYNC),
        // Backwards-compat: today/fetch.js + source-picker still read
        // status.connected directly. Mirrors the boolean here.
        "connected": oauth::is_connected(&slack::SLACK),
    })
}

#[tauri::command]
fn get_slack_oauth_status() -> IntegrationStatus {
    build_status_from_cache(
        oauth::is_connected(&slack::SLACK),
        KEYRING_SLACK_OAUTH_LAST_VERIFIED_AT,
        KEYRING_SLACK_OAUTH_LAST_ERROR,
        slack_oauth_extra(),
    )
}

#[tauri::command]
fn save_slack_oauth_credentials(
    client_id: String,
    client_secret: String,
) -> Result<(), String> {
    let id = client_id.trim();
    let secret = client_secret.trim();
    if id.is_empty() || secret.is_empty() {
        return Err("Both client ID and client secret required".to_string());
    }
    oauth::write_keychain(slack::SLACK.client_id_keychain_key, id)?;
    if let Some(secret_key) = slack::SLACK.client_secret_keychain_key {
        oauth::write_keychain(secret_key, secret)?;
    }
    Ok(())
}

#[tauri::command]
async fn connect_slack() -> Result<IntegrationStatus, String> {
    oauth::start_oauth_flow(&slack::SLACK).await?;
    // Verify immediately after the OAuth callback so Settings shows a
    // fresh stamp without waiting for the first list_slack_unreads
    // call. Failure here would mean Slack handed back a token that
    // doesn't pass auth.test, which shouldn't happen but we surface
    // it cleanly if it does.
    match verify_slack_oauth().await {
        Ok(()) => record_verification_success(
            KEYRING_SLACK_OAUTH_LAST_VERIFIED_AT,
            KEYRING_SLACK_OAUTH_LAST_ERROR,
        ),
        Err(reason) => record_verification_failure(
            KEYRING_SLACK_OAUTH_LAST_VERIFIED_AT,
            KEYRING_SLACK_OAUTH_LAST_ERROR,
            &reason,
        ),
    }
    Ok(get_slack_oauth_status())
}

#[tauri::command]
fn disconnect_slack() -> Result<(), String> {
    oauth::disconnect(&slack::SLACK)?;
    let _ = oauth::delete_keychain(KEYRING_SLACK_LAST_SYNC);
    let _ = oauth::delete_keychain(KEYRING_SLACK_OAUTH_LAST_VERIFIED_AT);
    let _ = oauth::delete_keychain(KEYRING_SLACK_OAUTH_LAST_ERROR);
    invalidate_cached_secret(KEYRING_SLACK_OAUTH_LAST_VERIFIED_AT);
    invalidate_cached_secret(KEYRING_SLACK_OAUTH_LAST_ERROR);
    Ok(())
}

fn stamp_slack_last_sync() {
    let _ = oauth::write_keychain(
        KEYRING_SLACK_LAST_SYNC,
        &chrono::Utc::now().to_rfc3339(),
    );
}

#[tauri::command]
async fn list_slack_unreads() -> Result<String, String> {
    let summaries = slack::count_unreads_per_channel().await?;
    stamp_slack_last_sync();
    serde_json::to_string(&summaries).map_err(|e| format!("Serialise: {}", e))
}

#[derive(Deserialize, Debug)]
struct ListSlackForClientInput {
    client_code: String,
    // Optional explicit slug — when None we derive from the client name
    // by lowercasing and replacing spaces with hyphens.
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    client_name: Option<String>,
}

// Derives a Slack channel slug from a client name. "Northcote Theatre"
// → "northcote-theatre". The slack module then prefixes "client-".
pub(crate) fn slugify_client_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_was_dash = false;
        } else if c.is_whitespace() || c == '-' || c == '_' || c == '/' {
            if !last_was_dash && !out.is_empty() {
                out.push('-');
                last_was_dash = true;
            }
        }
        // Anything else (punctuation, &, etc.) is dropped silently.
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[tauri::command]
async fn list_slack_for_client(input: serde_json::Value) -> Result<String, String> {
    let parsed: ListSlackForClientInput = serde_json::from_value(input)
        .map_err(|e| format!("Invalid input: {}", e))?;
    let code = parsed.client_code.trim().to_uppercase();
    if code.is_empty() {
        return Err("Pick a client first".to_string());
    }

    // Slug priority: explicit slug arg → client name lookup → code itself.
    let slug = match parsed.slug {
        Some(ref s) if !s.trim().is_empty() => s.trim().to_lowercase(),
        _ => {
            let name = match parsed.client_name {
                Some(ref n) if !n.trim().is_empty() => n.trim().to_string(),
                _ => {
                    let qs = format!(
                        "filterByFormula={}&maxRecords=1&fields%5B%5D=name",
                        urlencode(&format!("{{code}}='{}'", code))
                    );
                    let data = airtable_get("Clients", &qs)
                        .await
                        .unwrap_or(serde_json::Value::Null);
                    data["records"][0]["fields"]["name"]
                        .as_str()
                        .unwrap_or(&code)
                        .to_string()
                }
            };
            slugify_client_name(&name)
        }
    };

    let activity = slack::list_channel_activity_for_client(&slug).await?;
    stamp_slack_last_sync();
    serde_json::to_string(&activity).map_err(|e| format!("Serialise: {}", e))
}

// ── Conversations (v0.27 Block D) ──────────────────────────────────────
//
// Chat surface bound to a Workstream. Three commands cover the full
// surface: load, send, archive. Streaming (SSE) is queued for v0.27.1
// per the brief — non-streaming was the durable surface to ship first.

#[tauri::command]
async fn load_conversation(
    workstream_code: String,
) -> Result<conversations::ConversationPayload, String> {
    conversations::load(workstream_code).await
}

#[tauri::command]
async fn send_message(
    workstream_code: String,
    user_message: String,
    rate_limit: State<'_, RateLimit>,
) -> Result<conversations::ConversationPayload, String> {
    check_rate_limit(&rate_limit)?;
    conversations::send(workstream_code, user_message).await
}

#[tauri::command]
async fn archive_conversation(workstream_code: String) -> Result<(), String> {
    conversations::archive(workstream_code).await
}

// ── Project Updates feed (v0.28 Block E) ───────────────────────────────
//
// Aggregates ProjectNotes + Receipts + Conversations + Calendar + Slack +
// Gmail + Drive into one timeline scoped to a project code. Each source
// is fault-tolerant — if Slack OAuth is off, Slack is skipped and the
// rest still load. The free-text scratchpad lives in a new ProjectNotes
// table; CRUD commands sit alongside the aggregator.

#[tauri::command]
async fn list_project_updates(
    project_code: String,
) -> Result<project_feed::ProjectFeed, String> {
    project_feed::list_project_updates(&project_code).await
}

#[tauri::command]
async fn list_active_projects_for_client(
    client_code: String,
) -> Result<Vec<project_feed::ProjectSummary>, String> {
    project_feed::list_active_projects_for_client(&client_code).await
}

#[tauri::command]
async fn create_project_note(
    payload: serde_json::Value,
) -> Result<String, String> {
    let parsed: project_feed::CreateNoteInput = serde_json::from_value(payload)
        .map_err(|e| format!("Invalid input: {}", e))?;
    project_feed::create_note(parsed).await
}

#[tauri::command]
async fn update_project_note(
    payload: serde_json::Value,
) -> Result<(), String> {
    let parsed: project_feed::UpdateNoteInput = serde_json::from_value(payload)
        .map_err(|e| format!("Invalid input: {}", e))?;
    project_feed::update_note(parsed).await
}

#[tauri::command]
async fn delete_project_note(note_record_id: String) -> Result<(), String> {
    project_feed::delete_note(&note_record_id).await
}

// ── Cross-conversation search (v0.36 Block F) ──────────────────────────
//
// Single Tauri command exposed to the frontend. The header search bar
// debounces on the JS side; the 30s in-flight cache here covers repeat
// queries inside a session.

#[tauri::command]
async fn search_companion(
    query: String,
    limit: Option<usize>,
    cache: State<'_, search::SearchCache>,
) -> Result<Vec<search::SearchResult>, String> {
    search::search(&query, limit, &cache).await
}

// ── Studio CFO (v0.37 Block F) ─────────────────────────────────────────
//
// Read-only financial intelligence. Each command queries Airtable and
// aggregates client-side. Costs are computed using Caitlin's $110/h
// principal rate and Rose's $66/h subcontractor rate (TimeLogs.rate
// overrides when present).

#[tauri::command]
async fn cfo_studio_totals(year: i32, month: u32) -> Result<cfo::StudioTotals, String> {
    cfo::studio_totals(year, month).await
}

#[tauri::command]
async fn cfo_per_client(year: i32, month: u32) -> Result<Vec<cfo::ClientFinancials>, String> {
    cfo::per_client(year, month).await
}

#[tauri::command]
async fn cfo_hour_creep_alerts() -> Result<Vec<cfo::HourCreepAlert>, String> {
    cfo::hour_creep_alerts().await
}

#[tauri::command]
async fn cfo_outlook(year: i32, month: u32) -> Result<cfo::NextMonthOutlook, String> {
    cfo::outlook(year, month).await
}

// ── Tauri entry ─────────────────────────────────────────────────────────

fn toggle_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        match win.is_visible() {
            Ok(true) => {
                let _ = win.hide();
            }
            _ => {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let toggle_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyS);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcut(toggle_shortcut)
                .expect("register global shortcut")
                .with_handler(move |app, shortcut, event| {
                    if shortcut == &toggle_shortcut && event.state() == ShortcutState::Pressed {
                        toggle_main_window(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            // SQLite init.
            let dir = app.path().app_data_dir().expect("app_data_dir");
            std::fs::create_dir_all(&dir).expect("create app_data_dir");
            let path = dir.join("studio.db");
            let conn = Connection::open(&path).expect("open studio.db");
            init_db(&conn).expect("init schema");
            app.manage(DbState(Mutex::new(conn)));
            app.manage(RateLimit(Mutex::new(None)));
            app.manage(DriftCache(Mutex::new(None)));
            app.manage(search::SearchCache::new());

            // Warm secrets cache so we hit Keychain once at launch, not
            // per API call. The user clicks "Always Allow" once per
            // Keychain entry on first run; afterwards every call serves
            // from memory.
            let _ = cached_secret(KEYRING_USER);
            let _ = cached_secret(KEYRING_SLACK);
            let _ = cached_secret(KEYRING_AIRTABLE_KEY);
            let _ = cached_secret(KEYRING_AIRTABLE_BASE);

            // Tray icon with a small menu.
            let open_item = MenuItem::with_id(app, "open", "Open Companion", true, None::<&str>)?;
            let new_thinking = MenuItem::with_id(
                app,
                "new-thinking",
                "Start Strategic Thinking",
                true,
                Some("Cmd+Shift+S"),
            )?;
            let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Companion", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &new_thinking, &separator, &quit_item])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .menu_on_left_click(true)
                .tooltip("Companion")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "new-thinking" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                            let _ = win.eval(
                                "document.getElementById('strategic-thinking-btn')?.click()",
                            );
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_api_key_status,
            save_api_key,
            get_slack_status,
            save_slack_webhook,
            get_airtable_status,
            save_airtable_credentials,
            list_airtable_clients,
            list_airtable_projects,
            list_airtable_commitments,
            list_airtable_decisions,
            list_airtable_workstreams,
            list_airtable_receipts_recent,
            list_airtable_receipts_for_client,
            update_airtable_record,
            get_studio_version,
            check_url_up,
            check_github_actions,
            read_morning_briefing,
            check_drift,
            run_strategic_thinking,
            run_new_client_onboarding,
            run_monthly_checkin,
            run_new_campaign_scope,
            run_quarterly_review,
            run_subcontractor_onboarding,
            // v0.31 Block F — Skills batch 1
            run_nct_caption,
            run_in_cahoots_social_post,
            run_campaign_wrap_report,
            run_scope_of_work,
            // v0.35 Block F — Campaign Launch Checklist (deterministic L3)
            run_campaign_launch_checklist,
            // v0.32 Block F — Skills batch 2
            run_press_release,
            run_edm_writer,
            run_reels_scripting,
            run_hook_generator,
            run_client_email,
            run_humanizer,
            run_copy_editor,
            create_airtable_subcontractor,
            create_airtable_client,
            create_airtable_project,
            list_airtable_subcontractors,
            // v0.38 Block F — per-Subcontractor view (Rose's home)
            list_active_subcontractors,
            load_subcontractor_view,
            whoami_user,
            get_i_am,
            save_i_am,
            // v0.33 Block F — Lead-to-Client one-click promotion
            list_airtable_leads,
            promote_lead,
            confirm_discovery_slot,
            mark_lead_won,
            create_social_post,
            create_time_log,
            update_project,
            save_receipt,
            list_receipts,
            delete_receipt,
            tick_item,
            reprint_receipt,
            // v0.22 Block C — Granola MCP connector
            get_granola_status,
            save_granola_client_id,
            connect_granola,
            disconnect_granola,
            pull_granola_transcripts,
            // v0.24 Block C — Google Calendar
            get_google_status,
            save_google_client_id,
            connect_google,
            disconnect_google,
            list_calendar_today,
            list_calendar_week,
            list_calendar_for_client,
            // v0.25 Block C — Gmail + Drive (same Google OAuth)
            gmail_unread_count,
            list_gmail_urgent,
            list_gmail_for_client,
            list_drive_recent_for_client,
            // v0.26 Block C — Slack read OAuth (separate from the
            // existing webhook write path which stays on get_slack_status
            // / save_slack_webhook above)
            get_slack_oauth_status,
            save_slack_oauth_credentials,
            connect_slack,
            disconnect_slack,
            list_slack_unreads,
            list_slack_for_client,
            // v0.27 Block D — Conversations chat surface
            load_conversation,
            send_message,
            archive_conversation,
            // v0.28 Block E — Project Updates scratchpad
            list_project_updates,
            list_active_projects_for_client,
            create_project_note,
            update_project_note,
            delete_project_note,
            // v0.30 Block F — Forms layer
            get_form_url,
            set_form_url,
            list_form_urls,
            // v0.32 Block F — Source picker pattern
            fetch_workflow_context,
            // v0.39 Block F — Draft dossier (multi-source)
            run_draft_dossier,
            // v0.36 Block F — Cross-conversation search
            search_companion,
            // v0.37 Block F — Studio CFO (financial intelligence)
            cfo_studio_totals,
            cfo_per_client,
            cfo_hour_creep_alerts,
            cfo_outlook,
            // v0.41 — manual re-verify any integration
            verify_integration
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
