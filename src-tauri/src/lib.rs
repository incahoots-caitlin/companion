// In Cahoots Studio — Rust backend
//
// Tauri commands the frontend invokes:
//   get_api_key_status()        -> bool
//   save_api_key(key)           -> Result<(), String>
//   run_strategic_thinking(input) -> Result<String, String>  (returns receipt JSON)

use keyring::Entry;
use serde::{Deserialize, Serialize};

const KEYRING_SERVICE: &str = "marketing.incahoots.studio";
const KEYRING_USER: &str = "anthropic-api-key";
const ANTHROPIC_API: &str = "https://api.anthropic.com/v1/messages";
const MODEL_ID: &str = "claude-opus-4-7";

const STRATEGIC_THINKING_PROMPT: &str =
    include_str!("../prompts/strategic-thinking-system.md");

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
async fn run_strategic_thinking(input: String) -> Result<String, String> {
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

    extract_json_block(&text)
}

// ── Tauri entry ─────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_api_key_status,
            save_api_key,
            run_strategic_thinking
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
