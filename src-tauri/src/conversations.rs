// Companion - Conversations module (v0.27 Block D)
//
// Chat surface bound to a Workstream. Each Conversations row in Airtable
// is one chat thread; transcript holds the full message history as JSON.
//
// Public surface (called from lib.rs Tauri commands):
//   - load_conversation(workstream_code) -> ConversationPayload
//   - send_message(workstream_code, user_message) -> ConversationPayload
//   - archive_conversation(conv_id) -> ()
//
// v0.27 ships non-streaming: send_message blocks while Anthropic
// returns the full response, then writes the updated transcript back to
// Airtable. Streaming (SSE) is queued for v0.27.1 — non-streaming was
// the durable surface to ship first per the brief.
//
// System prompt is rebuilt every send from:
//   - The workstream's description, status, phase, next_action, blocker,
//     last_touch_at
//   - Open commitments and decisions for the workstream's client
//   - The 10 most recent receipts for the workstream's client
//
// Persistence: every successful send writes the full transcript JSON +
// last_message_at + message_count back to the Airtable row. If no row
// exists yet for the workstream, the first send creates it.

use crate::{airtable_get, airtable_create_record, airtable_update_record, urlencode, MODEL_ID, ANTHROPIC_API};
use serde::{Deserialize, Serialize};

const CONVERSATIONS_TABLE: &str = "Conversations";
const COMPANION_CHAT_PROMPT: &str = include_str!("../prompts/companion-chat-system.md");

// ── Public payload ─────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub role: String,    // "user" or "assistant"
    pub content: String,
    #[serde(default)]
    pub ts: String,      // ISO-8601 timestamp
}

#[derive(Serialize, Debug, Clone)]
pub struct ConversationPayload {
    pub conv_id: Option<String>,        // None when no row exists yet
    pub record_id: Option<String>,      // Airtable record id, when known
    pub workstream_code: String,
    pub workstream_record_id: Option<String>,
    pub status: String,                 // "active" | "archived" | "new"
    pub started_at: Option<String>,
    pub last_message_at: Option<String>,
    pub messages: Vec<Message>,
    pub workstream_title: Option<String>,
}

// ── Helpers ────────────────────────────────────────────────────────────

fn now_iso() -> String {
    chrono::Local::now()
        .with_timezone(&chrono::Utc)
        .to_rfc3339()
}

// Format conv_YYYY-MM-DD_HH-MM-SS in Australia/Melbourne time so the id
// matches what Caitlin reads in Airtable.
fn new_conv_id() -> String {
    let now = chrono::Local::now();
    format!("conv_{}", now.format("%Y-%m-%d_%H-%M-%S"))
}

// Pull the linked workstream record by code so we can read description,
// phase, etc. Returns None if no match.
async fn fetch_workstream_by_code(code: &str) -> Result<Option<serde_json::Value>, String> {
    if code.is_empty() {
        return Ok(None);
    }
    let escaped = code.replace('\'', "");
    let formula = format!("{{code}}='{}'", escaped);
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=code&fields%5B%5D=title&fields%5B%5D=description&fields%5B%5D=status&fields%5B%5D=phase&fields%5B%5D=last_touch_at&fields%5B%5D=next_action&fields%5B%5D=blocker&fields%5B%5D=client",
        urlencode(&formula)
    );
    let data = airtable_get("Workstreams", &qs).await?;
    Ok(data["records"].get(0).cloned())
}

// Pull the existing Conversations row for this workstream (record id, by
// link). Returns None if no row exists yet — the first send creates it.
async fn fetch_conversation_for_workstream(
    workstream_record_id: &str,
) -> Result<Option<serde_json::Value>, String> {
    if workstream_record_id.is_empty() {
        return Ok(None);
    }
    let escaped = workstream_record_id.replace('\'', "");
    // {workstream} is multipleRecordLinks. ARRAYJOIN() renders the
    // linked record ids as a comma-string; FIND() returns >0 when ours
    // is in there.
    let formula = format!("FIND('{}',ARRAYJOIN({{workstream}}))", escaped);
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=id&fields%5B%5D=workstream&fields%5B%5D=client&fields%5B%5D=started_at&fields%5B%5D=last_message_at&fields%5B%5D=message_count&fields%5B%5D=status&fields%5B%5D=summary&fields%5B%5D=transcript",
        urlencode(&formula)
    );
    let data = airtable_get(CONVERSATIONS_TABLE, &qs).await?;
    Ok(data["records"].get(0).cloned())
}

// Decode the transcript JSON column into a list of Messages. Empty array
// if the column is missing or unparseable — we don't want a bad row to
// break the chat surface.
fn parse_transcript(raw: &str) -> Vec<Message> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let parsed: Result<Vec<Message>, _> = serde_json::from_str(raw);
    match parsed {
        Ok(v) => v,
        Err(_) => {
            // Fall back to a permissive read so a hand-edited row doesn't
            // wipe history.
            #[derive(Deserialize)]
            struct LooseMsg {
                role: Option<String>,
                content: Option<String>,
                ts: Option<String>,
            }
            let loose: Result<Vec<LooseMsg>, _> = serde_json::from_str(raw);
            match loose {
                Ok(items) => items
                    .into_iter()
                    .filter_map(|m| {
                        let role = m.role?;
                        let content = m.content?;
                        Some(Message {
                            role,
                            content,
                            ts: m.ts.unwrap_or_default(),
                        })
                    })
                    .collect(),
                Err(_) => Vec::new(),
            }
        }
    }
}

// ── System prompt construction ────────────────────────────────────────
//
// Rebuilt per send. Cheap relative to Claude's reply and keeps the chat
// fresh as Caitlin updates Airtable from other surfaces.

async fn build_system_prompt(
    workstream: &serde_json::Value,
    client_record_id: Option<&str>,
) -> String {
    let mut sections: Vec<String> = Vec::new();
    sections.push(COMPANION_CHAT_PROMPT.to_string());

    // ── Workstream block ──
    let f = &workstream["fields"];
    let mut ws = String::from("\n\n## This workstream\n");
    if let Some(v) = f["code"].as_str() {
        ws.push_str(&format!("- Code: {}\n", v));
    }
    if let Some(v) = f["title"].as_str() {
        ws.push_str(&format!("- Title: {}\n", v));
    }
    if let Some(v) = f["status"].as_str() {
        ws.push_str(&format!("- Status: {}\n", v));
    }
    if let Some(v) = f["phase"].as_str() {
        ws.push_str(&format!("- Phase: {}\n", v));
    }
    if let Some(v) = f["next_action"].as_str() {
        ws.push_str(&format!("- Next action: {}\n", v));
    }
    if let Some(v) = f["blocker"].as_str() {
        ws.push_str(&format!("- Blocker: {}\n", v));
    }
    if let Some(v) = f["last_touch_at"].as_str() {
        ws.push_str(&format!("- Last touch: {}\n", v));
    }
    if let Some(v) = f["description"].as_str() {
        ws.push_str(&format!("- Description: {}\n", v));
    }
    sections.push(ws);

    // ── Client-scoped context ──
    if let Some(client_id) = client_record_id {
        // Open commitments for this client.
        if let Ok(commits) = fetch_open_commitments_for_client(client_id).await {
            if !commits.is_empty() {
                let mut block = String::from("\n## Open commitments for this client\n");
                for c in commits.iter().take(20) {
                    let title = c["fields"]["title"].as_str().unwrap_or("(untitled)");
                    let due = c["fields"]["due_at"].as_str().unwrap_or("");
                    if due.is_empty() {
                        block.push_str(&format!("- {}\n", title));
                    } else {
                        block.push_str(&format!("- {} (due {})\n", title, due));
                    }
                }
                sections.push(block);
            }
        }

        // Open decisions for this client.
        if let Ok(decisions) = fetch_open_decisions_for_client(client_id).await {
            if !decisions.is_empty() {
                let mut block = String::from("\n## Open decisions for this client\n");
                for d in decisions.iter().take(20) {
                    let title = d["fields"]["title"].as_str().unwrap_or("(untitled)");
                    let due = d["fields"]["due_date"].as_str().unwrap_or("");
                    if due.is_empty() {
                        block.push_str(&format!("- {}\n", title));
                    } else {
                        block.push_str(&format!("- {} (due {})\n", title, due));
                    }
                }
                sections.push(block);
            }
        }

        // Last 10 receipts for this client.
        if let Ok(receipts) = fetch_recent_receipts_for_client(client_id, 10).await {
            if !receipts.is_empty() {
                let mut block = String::from("\n## Last 10 receipts for this client\n");
                for r in receipts.iter() {
                    let title = r["fields"]["title"].as_str().unwrap_or("(untitled)");
                    let date = r["fields"]["date"].as_str().unwrap_or("");
                    let workflow = r["fields"]["workflow"].as_str().unwrap_or("");
                    block.push_str(&format!("- {} | {} | {}\n", date, workflow, title));
                }
                sections.push(block);
            }
        }
    }

    sections.push(format!(
        "\n## Today\n- {}",
        chrono::Local::now().format("%A %d %B %Y")
    ));

    sections.join("")
}

async fn fetch_open_commitments_for_client(
    client_record_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let escaped = client_record_id.replace('\'', "");
    let formula = format!(
        "AND({{status}}='open',FIND('{}',ARRAYJOIN({{client}})))",
        escaped
    );
    let qs = format!(
        "filterByFormula={}&pageSize=20&fields%5B%5D=title&fields%5B%5D=due_at&fields%5B%5D=status",
        urlencode(&formula)
    );
    let data = airtable_get("Commitments", &qs).await?;
    Ok(data["records"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

async fn fetch_open_decisions_for_client(
    client_record_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let escaped = client_record_id.replace('\'', "");
    let formula = format!(
        "AND({{status}}='open',FIND('{}',ARRAYJOIN({{client}})))",
        escaped
    );
    let qs = format!(
        "filterByFormula={}&pageSize=20&fields%5B%5D=title&fields%5B%5D=due_date&fields%5B%5D=status",
        urlencode(&formula)
    );
    let data = airtable_get("Decisions", &qs).await?;
    Ok(data["records"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

async fn fetch_recent_receipts_for_client(
    client_record_id: &str,
    limit: u32,
) -> Result<Vec<serde_json::Value>, String> {
    let n = limit.clamp(1, 50);
    let escaped = client_record_id.replace('\'', "");
    let formula = format!("FIND('{}',ARRAYJOIN({{client}}))", escaped);
    let qs = format!(
        "filterByFormula={}&pageSize={}&fields%5B%5D=title&fields%5B%5D=date&fields%5B%5D=workflow&sort%5B0%5D%5Bfield%5D=date&sort%5B0%5D%5Bdirection%5D=desc",
        urlencode(&formula),
        n
    );
    let data = airtable_get("Receipts", &qs).await?;
    Ok(data["records"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

// ── Public commands ────────────────────────────────────────────────────

pub async fn load(workstream_code: String) -> Result<ConversationPayload, String> {
    if workstream_code.trim().is_empty() {
        return Err("workstream_code required".to_string());
    }

    let workstream = fetch_workstream_by_code(&workstream_code)
        .await?
        .ok_or_else(|| format!("Workstream not found: {}", workstream_code))?;
    let workstream_id = workstream["id"]
        .as_str()
        .ok_or("Workstream missing id")?
        .to_string();
    let title = workstream["fields"]["title"].as_str().map(String::from);

    let row = fetch_conversation_for_workstream(&workstream_id).await?;
    if let Some(r) = row {
        let record_id = r["id"].as_str().map(String::from);
        let f = &r["fields"];
        let conv_id = f["id"].as_str().map(String::from);
        let started_at = f["started_at"].as_str().map(String::from);
        let last_message_at = f["last_message_at"].as_str().map(String::from);
        let status = f["status"].as_str().unwrap_or("active").to_string();
        let raw = f["transcript"].as_str().unwrap_or("");
        let messages = parse_transcript(raw);
        Ok(ConversationPayload {
            conv_id,
            record_id,
            workstream_code,
            workstream_record_id: Some(workstream_id),
            status,
            started_at,
            last_message_at,
            messages,
            workstream_title: title,
        })
    } else {
        // No row yet — return an empty placeholder. send_message will
        // create the row on first send.
        Ok(ConversationPayload {
            conv_id: None,
            record_id: None,
            workstream_code,
            workstream_record_id: Some(workstream_id),
            status: "new".to_string(),
            started_at: None,
            last_message_at: None,
            messages: Vec::new(),
            workstream_title: title,
        })
    }
}

// Anthropic non-streaming send. Streaming (SSE) is queued for v0.27.1.
pub async fn send(
    workstream_code: String,
    user_message: String,
) -> Result<ConversationPayload, String> {
    if workstream_code.trim().is_empty() {
        return Err("workstream_code required".to_string());
    }
    if user_message.trim().is_empty() {
        return Err("Empty message".to_string());
    }

    let api_key = crate::read_anthropic_key().ok_or("Anthropic API key not set")?;

    let workstream = fetch_workstream_by_code(&workstream_code)
        .await?
        .ok_or_else(|| format!("Workstream not found: {}", workstream_code))?;
    let workstream_id = workstream["id"]
        .as_str()
        .ok_or("Workstream missing id")?
        .to_string();
    let title = workstream["fields"]["title"].as_str().map(String::from);

    // Pull client link from the workstream so we can copy it on row
    // creation and use it for system-prompt scoping.
    let client_record_id: Option<String> = workstream["fields"]["client"]
        .as_array()
        .and_then(|a| a.get(0))
        .and_then(|v| v.as_str())
        .map(String::from);

    let existing = fetch_conversation_for_workstream(&workstream_id).await?;

    // Refuse to extend an archived conversation.
    if let Some(ref r) = existing {
        if r["fields"]["status"].as_str() == Some("archived") {
            return Err("Conversation is archived".to_string());
        }
    }

    // Load existing transcript or start fresh.
    let mut messages: Vec<Message> = match &existing {
        Some(r) => parse_transcript(r["fields"]["transcript"].as_str().unwrap_or("")),
        None => Vec::new(),
    };

    // Append the user turn.
    let now = now_iso();
    messages.push(Message {
        role: "user".to_string(),
        content: user_message.clone(),
        ts: now.clone(),
    });

    // Build system prompt fresh per send.
    let system_prompt = build_system_prompt(&workstream, client_record_id.as_deref()).await;

    // Build the Anthropic message array (role + content only — drop ts).
    let api_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content,
            })
        })
        .collect();

    let body = serde_json::json!({
        "model": MODEL_ID,
        "max_tokens": 4096,
        "system": system_prompt,
        "messages": api_messages,
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client init: {}", e))?;

    let response = client
        .post(ANTHROPIC_API)
        .header("x-api-key", &api_key)
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

    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    // Walk the content blocks for text. Same shape as collect_text_blocks
    // in lib.rs but local so we don't have to expose that helper.
    let assistant_text: String = parsed["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b["type"].as_str() == Some("text"))
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    if assistant_text.trim().is_empty() {
        return Err("Claude returned an empty reply".to_string());
    }

    let assistant_ts = now_iso();
    messages.push(Message {
        role: "assistant".to_string(),
        content: assistant_text,
        ts: assistant_ts.clone(),
    });

    let transcript_json = serde_json::to_string(&messages)
        .map_err(|e| format!("Serialise transcript: {}", e))?;
    let message_count = messages.len() as u64;

    // Persist to Airtable. Create on first send, patch on subsequent.
    let (conv_id, record_id, started_at) = match existing {
        Some(r) => {
            let record_id = r["id"]
                .as_str()
                .ok_or("Conversation row missing id")?
                .to_string();
            let conv_id = r["fields"]["id"].as_str().map(String::from);
            let started_at = r["fields"]["started_at"].as_str().map(String::from);
            airtable_update_record(
                CONVERSATIONS_TABLE,
                &record_id,
                serde_json::json!({
                    "transcript": transcript_json,
                    "last_message_at": assistant_ts,
                    "message_count": message_count,
                }),
            )
            .await?;
            (conv_id, Some(record_id), started_at)
        }
        None => {
            let conv_id = new_conv_id();
            let mut fields = serde_json::json!({
                "id": conv_id,
                "workstream": [workstream_id.clone()],
                "started_at": now,
                "last_message_at": assistant_ts.clone(),
                "message_count": message_count,
                "status": "active",
                "transcript": transcript_json,
            });
            if let Some(ref cid) = client_record_id {
                fields["client"] = serde_json::json!([cid]);
            }
            let record_id = airtable_create_record(CONVERSATIONS_TABLE, fields).await?;
            (Some(conv_id), Some(record_id), Some(now.clone()))
        }
    };

    Ok(ConversationPayload {
        conv_id,
        record_id,
        workstream_code,
        workstream_record_id: Some(workstream_id),
        status: "active".to_string(),
        started_at,
        last_message_at: Some(assistant_ts),
        messages,
        workstream_title: title,
    })
}

// Set the conversation row's status to "archived". Idempotent — if the
// row already says archived we still return Ok. If no row exists yet we
// also return Ok so callers (e.g. the workstream-mark-done flow) don't
// have to special-case "no chat yet".
pub async fn archive(workstream_code: String) -> Result<(), String> {
    let workstream = match fetch_workstream_by_code(&workstream_code).await? {
        Some(w) => w,
        None => return Ok(()),
    };
    let workstream_id = match workstream["id"].as_str() {
        Some(s) => s.to_string(),
        None => return Ok(()),
    };
    let row = match fetch_conversation_for_workstream(&workstream_id).await? {
        Some(r) => r,
        None => return Ok(()),
    };
    let record_id = match row["id"].as_str() {
        Some(s) => s.to_string(),
        None => return Ok(()),
    };
    if row["fields"]["status"].as_str() == Some("archived") {
        return Ok(());
    }
    airtable_update_record(
        CONVERSATIONS_TABLE,
        &record_id,
        serde_json::json!({ "status": "archived" }),
    )
    .await
}
