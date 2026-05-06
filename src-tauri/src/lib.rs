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
mod conversations;
mod drift;
mod drive;
mod gmail;
mod google;
mod granola;
mod oauth;
mod project_feed;
mod slack;

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
const MONTHLY_CHECKIN_PROMPT: &str =
    include_str!("../prompts/monthly-checkin-system.md");
const NEW_CAMPAIGN_SCOPE_PROMPT: &str =
    include_str!("../prompts/new-campaign-scope-system.md");
const QUARTERLY_REVIEW_PROMPT: &str =
    include_str!("../prompts/quarterly-review-system.md");
const SUBCONTRACTOR_ONBOARDING_PROMPT: &str =
    include_str!("../prompts/subcontractor-onboarding-system.md");

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

#[tauri::command]
fn get_api_key_status() -> bool {
    read_anthropic_key().is_some()
}

#[tauri::command]
fn save_api_key(key: String) -> Result<(), String> {
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
    Ok(())
}

fn read_slack_webhook() -> Option<String> {
    cached_secret(KEYRING_SLACK)
}

#[tauri::command]
fn get_slack_status() -> bool {
    read_slack_webhook().is_some()
}

#[tauri::command]
fn save_slack_webhook(url: String) -> Result<(), String> {
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
    Ok(())
}

// ── Airtable ───────────────────────────────────────────────────────────

pub(crate) fn read_airtable_creds() -> Option<(String, String)> {
    let key = cached_secret(KEYRING_AIRTABLE_KEY)?;
    let base = cached_secret(KEYRING_AIRTABLE_BASE)?;
    Some((key, base))
}

#[tauri::command]
fn get_airtable_status() -> bool {
    read_airtable_creds().is_some()
}

#[tauri::command]
fn save_airtable_credentials(api_key: String, base_id: String) -> Result<(), String> {
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
    Ok(())
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
    let client_code = parsed["project"]
        .as_str()
        .and_then(|p| p.split('-').next())
        .filter(|c| c.chars().all(|ch| ch.is_ascii_uppercase()) && c.len() <= 4)
        .unwrap_or("INC");

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
    match airtable_find_client_by_code(client_code).await {
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

    if let Err(e) = airtable_create_record("Receipts", fields).await {
        eprintln!("file_receipt_to_airtable: create failed: {}", e);
    }
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

    let json = extract_json_block(&text)?;

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

    let json = extract_json_block(&text)?;

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

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: MONTHLY_CHECKIN_PROMPT,
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

    let json = extract_json_block(&text)?;

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

    let json = extract_json_block(&text)?;

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

    let json = extract_json_block(&text)?;

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

    let json = extract_json_block(&text)?;

    {
        let conn = state.0.lock().map_err(|e| format!("DB lock: {}", e))?;
        persist_receipt(&conn, &json)?;
    }

    file_receipt_to_airtable(&json).await;

    Ok(json)
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
    let receipt_json = build_pure_receipt(
        &receipt_id,
        "schedule-social-post",
        &format!("RECEIPT — SOCIAL POST · {}", title),
        &receipt_project,
        sections,
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
    let receipt_json = build_pure_receipt(
        &receipt_id,
        "log-time",
        &title,
        &receipt_project,
        sections,
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
    let receipt_json = receipt_obj.to_string();

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

#[derive(Serialize)]
struct GranolaStatus {
    connected: bool,
    has_client_id: bool,
    last_pull_at: Option<String>,
}

#[tauri::command]
fn get_granola_status() -> GranolaStatus {
    GranolaStatus {
        connected: oauth::is_connected(&granola::GRANOLA),
        has_client_id: oauth::read_keychain(granola::GRANOLA.client_id_keychain_key)
            .is_some(),
        last_pull_at: oauth::read_keychain(KEYRING_GRANOLA_LAST_PULL),
    }
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
async fn connect_granola() -> Result<(), String> {
    oauth::start_oauth_flow(&granola::GRANOLA).await
}

#[tauri::command]
fn disconnect_granola() -> Result<(), String> {
    oauth::disconnect(&granola::GRANOLA)
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

#[derive(Serialize)]
struct GoogleStatus {
    connected: bool,
    has_client_id: bool,
    last_sync_at: Option<String>,
    // v0.25: which surfaces this token covers. Settings renders this as
    // "Calendar, Gmail, Drive" so Caitlin can see at a glance whether
    // she needs to re-authorise to add a missing surface. Defaults to
    // ["calendar"] for tokens minted under v0.24 — those need a
    // re-authorise to upgrade. We assume the full set for any new
    // connect (the OAuth flow now requests all three).
    scopes: Vec<String>,
}

#[tauri::command]
fn get_google_status() -> GoogleStatus {
    let connected = oauth::is_connected(&google::GOOGLE);
    let scopes = if connected {
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
    GoogleStatus {
        connected,
        has_client_id: oauth::read_keychain(google::GOOGLE.client_id_keychain_key)
            .is_some(),
        last_sync_at: oauth::read_keychain(KEYRING_GOOGLE_LAST_SYNC),
        scopes,
    }
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
async fn connect_google() -> Result<(), String> {
    oauth::start_oauth_flow(&google::GOOGLE).await?;
    // Record which surfaces this token covers. v0.25 always asks for
    // Calendar + Gmail + Drive — if the user re-authorises we overwrite
    // the v0.24 marker that says Calendar only.
    let _ = oauth::write_keychain(
        KEYRING_GOOGLE_GRANTED_SCOPES,
        "calendar,gmail,drive",
    );
    Ok(())
}

#[tauri::command]
fn disconnect_google() -> Result<(), String> {
    oauth::disconnect(&google::GOOGLE)?;
    let _ = oauth::delete_keychain(KEYRING_GOOGLE_LAST_SYNC);
    let _ = oauth::delete_keychain(KEYRING_GOOGLE_GRANTED_SCOPES);
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

#[derive(Serialize)]
struct SlackOauthStatus {
    connected: bool,
    has_client_id: bool,
    has_client_secret: bool,
    last_sync_at: Option<String>,
}

#[tauri::command]
fn get_slack_oauth_status() -> SlackOauthStatus {
    SlackOauthStatus {
        connected: oauth::is_connected(&slack::SLACK),
        has_client_id: oauth::read_keychain(slack::SLACK.client_id_keychain_key)
            .is_some(),
        has_client_secret: slack::SLACK
            .client_secret_keychain_key
            .map(|k| oauth::read_keychain(k).is_some())
            .unwrap_or(false),
        last_sync_at: oauth::read_keychain(KEYRING_SLACK_LAST_SYNC),
    }
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
async fn connect_slack() -> Result<(), String> {
    oauth::start_oauth_flow(&slack::SLACK).await
}

#[tauri::command]
fn disconnect_slack() -> Result<(), String> {
    oauth::disconnect(&slack::SLACK)?;
    let _ = oauth::delete_keychain(KEYRING_SLACK_LAST_SYNC);
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
fn slugify_client_name(name: &str) -> String {
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
            create_airtable_subcontractor,
            create_airtable_client,
            create_airtable_project,
            list_airtable_subcontractors,
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
            delete_project_note
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
