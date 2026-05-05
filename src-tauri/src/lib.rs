// In Cahoots Studio — Rust backend
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
const ANTHROPIC_API: &str = "https://api.anthropic.com/v1/messages";
const AIRTABLE_API: &str = "https://api.airtable.com/v0";
const MODEL_ID: &str = "claude-opus-4-7";

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

fn read_anthropic_key() -> Option<String> {
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

fn read_airtable_creds() -> Option<(String, String)> {
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

async fn airtable_create_record(
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

async fn airtable_update_record(
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

fn urlencode(s: &str) -> String {
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

async fn airtable_get(table: &str, query: &str) -> Result<serde_json::Value, String> {
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
    let data = airtable_get(
        "Clients",
        "filterByFormula=%7Bstatus%7D%3D%27active%27&fields%5B%5D=code&fields%5B%5D=name&fields%5B%5D=status",
    )
    .await?;
    serde_json::to_string(&data).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_airtable_projects() -> Result<String, String> {
    let data = airtable_get(
        "Projects",
        "filterByFormula=NOT(%7Bstatus%7D%3D%27archive%27)&fields%5B%5D=code&fields%5B%5D=name&fields%5B%5D=status&fields%5B%5D=client",
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
}

#[derive(Deserialize, Debug)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Deserialize, Debug)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

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

    let text = parsed
        .content
        .iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

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

    let text = api_response
        .content
        .iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

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
                    let done = item["done"].as_bool() == Some(false);
                    let prefix = if is_task && done {
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

    let user_message = format!(
        "## Monthly check-in for {} ({})\n\n\
         **Status:** {}\n\
         **Today's date:** {}\n\
         **Window:** last 30 days\n\n\
         ## User flags from the studio side\n\n{}\n\n\
         ## Recent receipts\n\n{}",
        client_name,
        code,
        client_status,
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.extra_notes.as_deref().unwrap_or("(none)"),
        bundle,
    );

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: MONTHLY_CHECKIN_PROMPT,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
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

    let text = api_response
        .content
        .iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

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

    let user_message = format!(
        "## Quarterly review for {} ({})\n\n\
         **Status:** {}\n\
         **Today's date:** {}\n\
         **Window:** last 90 days\n\n\
         ## User flags from the studio side\n\n{}\n\n\
         ## Recent receipts (last quarter)\n\n{}",
        client_name,
        code,
        client_status,
        chrono::Local::now().format("%A %d %B %Y"),
        parsed.extra_notes.as_deref().unwrap_or("(none)"),
        bundle,
    );

    let body = AnthropicRequest {
        model: MODEL_ID,
        max_tokens: 4096,
        system: QUARTERLY_REVIEW_PROMPT,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: user_message,
        }],
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

    let text = api_response
        .content
        .iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

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

    let text = api_response
        .content
        .iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

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

    let text = api_response
        .content
        .iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

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

            // Warm secrets cache so we hit Keychain once at launch, not
            // per API call. The user clicks "Always Allow" once per
            // Keychain entry on first run; afterwards every call serves
            // from memory.
            let _ = cached_secret(KEYRING_USER);
            let _ = cached_secret(KEYRING_SLACK);
            let _ = cached_secret(KEYRING_AIRTABLE_KEY);
            let _ = cached_secret(KEYRING_AIRTABLE_BASE);

            // Tray icon with a small menu.
            let open_item = MenuItem::with_id(app, "open", "Open Studio", true, None::<&str>)?;
            let new_thinking = MenuItem::with_id(
                app,
                "new-thinking",
                "Start Strategic Thinking",
                true,
                Some("Cmd+Shift+S"),
            )?;
            let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Studio", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &new_thinking, &separator, &quit_item])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .menu_on_left_click(true)
                .tooltip("In Cahoots Studio")
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
            run_strategic_thinking,
            run_new_client_onboarding,
            run_monthly_checkin,
            run_new_campaign_scope,
            run_quarterly_review,
            run_subcontractor_onboarding,
            create_airtable_subcontractor,
            create_airtable_client,
            create_airtable_project,
            save_receipt,
            list_receipts,
            delete_receipt,
            tick_item,
            reprint_receipt
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
