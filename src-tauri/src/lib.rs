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
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, State,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const KEYRING_SERVICE: &str = "marketing.incahoots.studio";
const KEYRING_USER: &str = "anthropic-api-key";
const KEYRING_SLACK: &str = "slack-webhook-url";
const ANTHROPIC_API: &str = "https://api.anthropic.com/v1/messages";
const MODEL_ID: &str = "claude-opus-4-7";

const STRATEGIC_THINKING_PROMPT: &str =
    include_str!("../prompts/strategic-thinking-system.md");

// ── DB state ───────────────────────────────────────────────────────────

pub struct DbState(pub Mutex<Connection>);

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

// ── Auth ────────────────────────────────────────────────────────────────

fn read_anthropic_key() -> Option<String> {
    // 1. Env var (dev convenience).
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    // 2. macOS Keychain.
    Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .ok()
        .and_then(|e| e.get_password().ok())
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
    Ok(())
}

fn read_slack_webhook() -> Option<String> {
    Entry::new(KEYRING_SERVICE, KEYRING_SLACK)
        .ok()
        .and_then(|e| e.get_password().ok())
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
    Ok(())
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
) -> Result<String, String> {
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

    Ok(json)
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

    // Fire on_done hook (best-effort).
    if let Some(target) = hook_target {
        if let Some(channel) = target.strip_prefix("slack:") {
            if let Some(webhook) = read_slack_webhook() {
                let msg = format!(":white_check_mark: *{}*\nposted to {}", item_text, channel);
                if let Err(e) = post_to_slack(&webhook, &msg).await {
                    eprintln!("Slack post failed: {}", e);
                }
            }
        }
    }

    Ok(new_json)
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
            run_strategic_thinking,
            save_receipt,
            list_receipts,
            delete_receipt,
            tick_item
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
