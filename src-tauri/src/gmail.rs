// Companion - Gmail API client (v0.25)
//
// Direct Gmail v1 REST. Calls run from the Rust backend; same pattern
// as calendar.rs — no Anthropic Messages API in the loop, just bearer
// HTTP. Three surfaces:
//
//   - count_unread()             -> total unread in the inbox.
//   - list_urgent_threads()      -> small list of "flagged urgent"
//     threads. Heuristic: unread AND (starred OR sender matches a
//     primary contact email pulled from the Clients table).
//   - list_threads_for_client()  -> last 3 threads for a client. Uses
//     the Clients table's `gmail_thread_filter` field as a free-text
//     query string passed straight to Gmail's `q=` parameter. Falls
//     back to the client name when the filter is empty.
//
// Gmail's REST surface returns thread IDs cheaply and message metadata
// on a separate call. We hydrate up to N threads with a single
// metadata fetch per thread to keep the per-render cost low.

use crate::google;
use crate::oauth;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// Cap on per-call hydration — Gmail charges quota by metadata fetch, and
// the per-client view only renders the most recent 3 anyway.
const URGENT_THREAD_CAP: usize = 5;
const PER_CLIENT_THREAD_CAP: usize = 3;

// ── Public thread shape ───────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EmailThread {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub snippet: String,
    // Internal Gmail timestamp in milliseconds since the epoch. Frontend
    // formats it; we surface the raw number so any timezone work happens
    // in the renderer.
    pub date_ms: i64,
    pub unread: bool,
    pub starred: bool,
    // Web URL to open the thread in Gmail. Built from id + accountIndex
    // 0 — Caitlin's primary Gmail. Multi-account users can edit the
    // index manually but Companion doesn't offer it as a setting.
    pub web_link: String,
}

// ── Top-level helpers ─────────────────────────────────────────────────

pub async fn count_unread() -> Result<u32, String> {
    let token = oauth::ensure_fresh_token(&google::GOOGLE)
        .await
        .map_err(|e| format!("Google: {}", e))?;
    let url = format!(
        "{}/users/me/labels/UNREAD",
        google::GMAIL_API_BASE,
    );
    let resp = http_get_json(&url, &token).await?;
    // Gmail returns threadsUnread as a string in JSON; default to 0 on
    // missing.
    let count = resp
        .get("threadsUnread")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u32>().ok())
        .or_else(|| resp.get("threadsUnread").and_then(|v| v.as_u64()).map(|n| n as u32))
        .unwrap_or(0);
    Ok(count)
}

// Heuristic: unread + (starred OR sender in the important-contacts set).
// `important_senders` is a normalised list of lowercase email addresses
// derived from Clients.primary_contact_email. The caller (lib.rs) builds
// it from a single Airtable read so the Gmail loop here is pure.
pub async fn list_urgent_threads(
    important_senders: &[String],
) -> Result<Vec<EmailThread>, String> {
    let token = oauth::ensure_fresh_token(&google::GOOGLE)
        .await
        .map_err(|e| format!("Google: {}", e))?;

    // Pull the recent unread thread list. Gmail's q= operator takes
    // search-syntax strings; "is:unread" pairs nicely with newer-than to
    // keep the result set small. 30d window is plenty for triage.
    let q = "is:unread newer_than:30d";
    let ids = list_thread_ids(&token, q, 25).await?;

    let mut out: Vec<EmailThread> = Vec::with_capacity(URGENT_THREAD_CAP);
    for id in ids {
        if out.len() >= URGENT_THREAD_CAP {
            break;
        }
        match fetch_thread_meta(&token, &id).await {
            Ok(thread) => {
                let from_lower = thread.from.to_lowercase();
                let sender_match = important_senders
                    .iter()
                    .any(|addr| !addr.is_empty() && from_lower.contains(addr));
                if thread.starred || sender_match {
                    out.push(thread);
                }
            }
            Err(e) => {
                eprintln!("[gmail] thread {} hydrate failed: {}", id, e);
                continue;
            }
        }
    }
    Ok(out)
}

// Per-client thread list. Uses Gmail's `q=` syntax verbatim — the
// Clients.gmail_thread_filter field is meant to be edited as a Gmail
// search expression (e.g. `from:@untitledgroup.com.au OR untitled`).
// When empty, falls back to a name match so brand-new clients aren't
// blank — Caitlin can refine the filter later.
pub async fn list_threads_for_client(
    client_name: &str,
    filter_expr: Option<&str>,
) -> Result<Vec<EmailThread>, String> {
    let token = oauth::ensure_fresh_token(&google::GOOGLE)
        .await
        .map_err(|e| format!("Google: {}", e))?;

    let trimmed_filter = filter_expr.map(|s| s.trim()).unwrap_or("");
    let q_owned = if !trimmed_filter.is_empty() {
        format!("({}) newer_than:60d", trimmed_filter)
    } else {
        // No filter set — fall back to a name match. Quote the name so a
        // multi-word value (e.g. "Northcote Theatre") matches as a
        // phrase. 60d window keeps it scoped without dropping recent
        // catch-ups.
        format!("\"{}\" newer_than:60d", client_name.replace('"', ""))
    };

    let ids = list_thread_ids(&token, &q_owned, PER_CLIENT_THREAD_CAP).await?;
    let mut out: Vec<EmailThread> = Vec::with_capacity(ids.len());
    for id in ids.into_iter().take(PER_CLIENT_THREAD_CAP) {
        match fetch_thread_meta(&token, &id).await {
            Ok(thread) => out.push(thread),
            Err(e) => eprintln!("[gmail] thread {} hydrate failed: {}", id, e),
        }
    }
    Ok(out)
}

// ── Internals: list + hydrate ─────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct ThreadListResponse {
    #[serde(default)]
    threads: Vec<ThreadStub>,
}

#[derive(Deserialize, Debug)]
struct ThreadStub {
    id: String,
}

async fn list_thread_ids(
    token: &str,
    q: &str,
    max: usize,
) -> Result<Vec<String>, String> {
    let url = format!(
        "{}/users/me/threads?maxResults={}&q={}",
        google::GMAIL_API_BASE,
        max,
        urlencode(q),
    );
    let resp = http_get_json(&url, token).await?;
    let parsed: ThreadListResponse = serde_json::from_value(resp)
        .map_err(|e| format!("threads.list parse: {}", e))?;
    Ok(parsed.threads.into_iter().map(|t| t.id).collect())
}

#[derive(Deserialize, Debug)]
struct ThreadFull {
    #[serde(default)]
    messages: Vec<RawMessage>,
}

#[derive(Deserialize, Debug)]
struct RawMessage {
    #[allow(dead_code)] // captured for debug parity with Calendar's RawEvent
    id: String,
    #[serde(rename = "internalDate", default)]
    internal_date: Option<String>,
    #[serde(default)]
    snippet: Option<String>,
    #[serde(rename = "labelIds", default)]
    label_ids: Vec<String>,
    payload: Option<RawPayload>,
}

#[derive(Deserialize, Debug)]
struct RawPayload {
    #[serde(default)]
    headers: Vec<RawHeader>,
}

#[derive(Deserialize, Debug)]
struct RawHeader {
    name: String,
    value: String,
}

async fn fetch_thread_meta(token: &str, id: &str) -> Result<EmailThread, String> {
    // format=metadata + a header allowlist keeps the response tiny — we
    // just want the latest message's From, Subject, snippet, and labels.
    let url = format!(
        "{}/users/me/threads/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject",
        google::GMAIL_API_BASE,
        urlencode(id),
    );
    let resp = http_get_json(&url, token).await?;
    let parsed: ThreadFull = serde_json::from_value(resp)
        .map_err(|e| format!("thread parse: {}", e))?;

    // Use the latest message in the thread for display. Gmail returns
    // them oldest-first by default; take the last one.
    let last = parsed
        .messages
        .last()
        .ok_or_else(|| "thread has no messages".to_string())?;

    let mut from = String::new();
    let mut subject = String::new();
    if let Some(payload) = &last.payload {
        for h in &payload.headers {
            match h.name.as_str() {
                "From" => from = h.value.clone(),
                "Subject" => subject = h.value.clone(),
                _ => {}
            }
        }
    }

    let date_ms = last
        .internal_date
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    // Unread / starred state from the latest message's labels.
    let unread = last.label_ids.iter().any(|l| l == "UNREAD");
    let starred = last.label_ids.iter().any(|l| l == "STARRED");

    let web_link = format!("https://mail.google.com/mail/u/0/#all/{}", id);

    Ok(EmailThread {
        id: id.to_string(),
        subject: if subject.is_empty() { "(no subject)".to_string() } else { subject },
        from,
        snippet: last.snippet.clone().unwrap_or_default(),
        date_ms,
        unread,
        starred,
        web_link,
    })
}

// ── HTTP helper ───────────────────────────────────────────────────────

async fn http_get_json(url: &str, token: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
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
        return Err(format!("Gmail {}: {}", status.as_u16(), body));
    }
    resp.json().await.map_err(|e| format!("Parse: {}", e))
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}
