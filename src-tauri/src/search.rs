// Companion - Cross-conversation search (v0.36 Block F).
//
// Single search bar in the Companion header. Queries five Airtable tables
// in parallel and returns a unified, recency-sorted list of results
// grouped by source. v1 uses Airtable filterByFormula scoring (case-
// insensitive FIND across the searchable fields per table). Embeddings
// are the cleaner long-term move (v0.46), but this v1 keeps shipping
// cheap and avoids new infrastructure.
//
// Tables and the fields each one searches:
//   Conversations: summary, transcript
//   Receipts:      title, summary, json
//   ProjectNotes:  body
//   Decisions:     title, decision, reasoning
//   Commitments:   title, notes
//
// Per-table query budget: 50 records, 5s timeout. Failed table queries
// skip silently so a flaky network never blanks the dropdown. Results
// are merged, ranked by `last_modified_at` (or the closest equivalent
// per source) descending, then sliced to `limit` (default 30).

use crate::{airtable_get, urlencode};
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const RECEIPTS_TABLE: &str = "Receipts";
const CONVERSATIONS_TABLE: &str = "Conversations";
const PROJECT_NOTES_TABLE: &str = "ProjectNotes";
const DECISIONS_TABLE: &str = "Decisions";
const COMMITMENTS_TABLE: &str = "Commitments";

const PER_TABLE_LIMIT: u32 = 50;
const PER_TABLE_TIMEOUT: Duration = Duration::from_secs(5);
const SNIPPET_RADIUS: usize = 60;
const DEFAULT_RESULT_LIMIT: usize = 30;

// In-flight cache. Keyed by lowercased query string. 30s TTL — covers a
// fast typist double-tapping a search and the back button without
// hammering Airtable.
const CACHE_TTL: Duration = Duration::from_secs(30);

pub struct SearchCache(pub Mutex<Vec<(String, Instant, Vec<SearchResult>)>>);

impl SearchCache {
    pub fn new() -> Self {
        SearchCache(Mutex::new(Vec::new()))
    }

    fn get(&self, key: &str) -> Option<Vec<SearchResult>> {
        let mut guard = self.0.lock().ok()?;
        let now = Instant::now();
        // Drop stale entries while we're here.
        guard.retain(|(_, ts, _)| now.duration_since(*ts) < CACHE_TTL);
        guard
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, _, v)| v.clone())
    }

    fn put(&self, key: String, value: Vec<SearchResult>) {
        if let Ok(mut guard) = self.0.lock() {
            guard.retain(|(k, _, _)| k != &key);
            guard.push((key, Instant::now(), value));
            // Cap cache size at 32 entries.
            if guard.len() > 32 {
                let drop_count = guard.len() - 32;
                guard.drain(0..drop_count);
            }
        }
    }
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Conversation,
    Receipt,
    ProjectNote,
    Decision,
    Commitment,
}

#[derive(Serialize, Debug, Clone)]
pub struct SearchResult {
    pub source: SourceKind,
    pub record_id: String,
    pub title: String,
    pub snippet: String,
    pub timestamp: String,
    // The frontend uses `jump_to` to dispatch to the right surface. Shape:
    //   { kind: "conversation", workstream_code, client_code? }
    //   { kind: "receipt", record_id, ... full receipt fields ... }
    //   { kind: "project_note", project_code?, note_record_id }
    //   { kind: "decision", record_id, fields }
    //   { kind: "commitment", record_id, fields }
    pub jump_to: serde_json::Value,
}

// ── Public entrypoint ─────────────────────────────────────────────────

pub async fn search(
    query: &str,
    limit: Option<usize>,
    cache: &SearchCache,
) -> Result<Vec<SearchResult>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    // Single-char queries thrash Airtable for no value. Two chars is the
    // shortest practical match.
    if q.chars().count() < 2 {
        return Ok(Vec::new());
    }

    let cache_key = q.to_lowercase();
    if let Some(hit) = cache.get(&cache_key) {
        return Ok(truncate(hit, limit));
    }

    let q_owned = q.to_string();

    // Run all five table queries in parallel. Each returns its own
    // Vec<SearchResult>; failures are absorbed into an empty vec so a
    // single bad table never blanks the dropdown.
    let (conv, rec, notes, dec, cmt) = tokio::join!(
        with_timeout(PER_TABLE_TIMEOUT, search_conversations(&q_owned)),
        with_timeout(PER_TABLE_TIMEOUT, search_receipts(&q_owned)),
        with_timeout(PER_TABLE_TIMEOUT, search_project_notes(&q_owned)),
        with_timeout(PER_TABLE_TIMEOUT, search_decisions(&q_owned)),
        with_timeout(PER_TABLE_TIMEOUT, search_commitments(&q_owned)),
    );

    let mut all: Vec<SearchResult> = Vec::new();
    push_or_log("conversations", conv, &mut all);
    push_or_log("receipts", rec, &mut all);
    push_or_log("project_notes", notes, &mut all);
    push_or_log("decisions", dec, &mut all);
    push_or_log("commitments", cmt, &mut all);

    // Sort by timestamp desc. Empty timestamps sink to the bottom.
    all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    cache.put(cache_key, all.clone());
    Ok(truncate(all, limit))
}

fn truncate(mut v: Vec<SearchResult>, limit: Option<usize>) -> Vec<SearchResult> {
    let n = limit.unwrap_or(DEFAULT_RESULT_LIMIT);
    if v.len() > n {
        v.truncate(n);
    }
    v
}

fn push_or_log(
    label: &str,
    result: Result<Result<Vec<SearchResult>, String>, &'static str>,
    out: &mut Vec<SearchResult>,
) {
    match result {
        Ok(Ok(mut v)) => out.append(&mut v),
        Ok(Err(e)) => eprintln!("[search] {} failed: {}", label, e),
        Err(_) => eprintln!("[search] {} timed out", label),
    }
}

async fn with_timeout<F, T>(d: Duration, fut: F) -> Result<T, &'static str>
where
    F: std::future::Future<Output = T>,
{
    match tokio::time::timeout(d, fut).await {
        Ok(v) => Ok(v),
        Err(_) => Err("timeout"),
    }
}

// ── Per-table searches ────────────────────────────────────────────────

// FIND(LOWER('q'), LOWER({field}))>0 OR ... — case-insensitive contains
// across N fields. Airtable's FIND returns 0 when not found, so >0 is
// the truthy form. Single quotes in the query are stripped to keep the
// formula valid; Airtable doesn't expose an escape mechanism beyond
// avoiding them.
fn build_or_find_formula(query: &str, fields: &[&str]) -> String {
    let q = query.replace('\'', "").to_lowercase();
    if fields.is_empty() {
        return "FALSE()".to_string();
    }
    let parts: Vec<String> = fields
        .iter()
        .map(|f| format!("FIND('{}',LOWER({{{}}}))", q, f))
        .collect();
    if parts.len() == 1 {
        parts.into_iter().next().unwrap()
    } else {
        format!("OR({})", parts.join(","))
    }
}

async fn search_conversations(query: &str) -> Result<Vec<SearchResult>, String> {
    let formula = build_or_find_formula(query, &["summary", "transcript"]);
    let qs = format!(
        "filterByFormula={}&pageSize={}\
&fields%5B%5D=id&fields%5B%5D=workstream&fields%5B%5D=client\
&fields%5B%5D=started_at&fields%5B%5D=last_message_at\
&fields%5B%5D=message_count&fields%5B%5D=status\
&fields%5B%5D=summary&fields%5B%5D=transcript",
        urlencode(&formula),
        PER_TABLE_LIMIT
    );
    let data = airtable_get(CONVERSATIONS_TABLE, &qs).await?;
    let mut out = Vec::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let record_id = r["id"].as_str().unwrap_or_default().to_string();
            let summary = f["summary"].as_str().unwrap_or_default();
            let transcript = f["transcript"].as_str().unwrap_or_default();
            // Workstream link rolls up to a code via the formula field.
            // Airtable returns rollup-as-array of strings; the chat view
            // navigates by workstream code, so we read it here.
            let workstream_code = read_first_string(&f["workstream"])
                .unwrap_or_default();
            let client_code = read_first_string(&f["client"])
                .unwrap_or_default();
            let title = if !summary.is_empty() {
                first_line(summary, 80).to_string()
            } else if !workstream_code.is_empty() {
                format!("Conversation · {}", workstream_code)
            } else {
                "Conversation".to_string()
            };
            let snippet = make_snippet(query, &[summary, transcript]);
            let ts = f["last_message_at"]
                .as_str()
                .or_else(|| f["started_at"].as_str())
                .unwrap_or_default()
                .to_string();
            out.push(SearchResult {
                source: SourceKind::Conversation,
                record_id: record_id.clone(),
                title,
                snippet,
                timestamp: ts,
                jump_to: serde_json::json!({
                    "kind": "conversation",
                    "record_id": record_id,
                    "workstream_code": workstream_code,
                    "client_code": client_code,
                }),
            });
        }
    }
    Ok(out)
}

async fn search_receipts(query: &str) -> Result<Vec<SearchResult>, String> {
    let formula = build_or_find_formula(query, &["title", "summary", "json"]);
    let qs = format!(
        "filterByFormula={}&pageSize={}\
&fields%5B%5D=id&fields%5B%5D=title&fields%5B%5D=date\
&fields%5B%5D=workflow&fields%5B%5D=client&fields%5B%5D=ticked_count\
&fields%5B%5D=json&fields%5B%5D=summary",
        urlencode(&formula),
        PER_TABLE_LIMIT
    );
    let data = airtable_get(RECEIPTS_TABLE, &qs).await?;
    let mut out = Vec::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let record_id = r["id"].as_str().unwrap_or_default().to_string();
            let title = f["title"].as_str().unwrap_or("Receipt").to_string();
            let summary = f["summary"].as_str().unwrap_or_default();
            let json_body = f["json"].as_str().unwrap_or_default();
            let workflow = f["workflow"].as_str().unwrap_or_default();
            let date = f["date"].as_str().unwrap_or_default();
            let snippet = make_snippet(query, &[summary, json_body, &title]);
            // Receipts.date is YYYY-MM-DD. Pad to RFC3339 so cross-table
            // sort stays correct.
            let ts = if date.len() == 10 {
                format!("{}T00:00:00Z", date)
            } else {
                date.to_string()
            };
            // Receipt JSON modal payload — match the shape the existing
            // showReceiptJsonModal() expects.
            let ticked = f["ticked_count"].as_i64().unwrap_or(0);
            out.push(SearchResult {
                source: SourceKind::Receipt,
                record_id: record_id.clone(),
                title,
                snippet,
                timestamp: ts,
                jump_to: serde_json::json!({
                    "kind": "receipt",
                    "record_id": record_id,
                    "title": f["title"].as_str().unwrap_or(""),
                    "workflow": workflow,
                    "date": date,
                    "ticked": ticked,
                    "total": ticked,
                    "json": json_body,
                    "summary": summary,
                }),
            });
        }
    }
    Ok(out)
}

async fn search_project_notes(query: &str) -> Result<Vec<SearchResult>, String> {
    let formula = build_or_find_formula(query, &["body"]);
    let qs = format!(
        "filterByFormula={}&pageSize={}\
&fields%5B%5D=id&fields%5B%5D=project&fields%5B%5D=client\
&fields%5B%5D=created_at&fields%5B%5D=created_by\
&fields%5B%5D=body&fields%5B%5D=tags",
        urlencode(&formula),
        PER_TABLE_LIMIT
    );
    let data = airtable_get(PROJECT_NOTES_TABLE, &qs).await?;
    let mut out = Vec::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let record_id = r["id"].as_str().unwrap_or_default().to_string();
            let body = f["body"].as_str().unwrap_or_default();
            let project_code = read_first_string(&f["project"]).unwrap_or_default();
            let client_code = read_first_string(&f["client"]).unwrap_or_default();
            let title = if !project_code.is_empty() {
                format!("Note · {}", project_code)
            } else {
                "Project note".to_string()
            };
            let snippet = make_snippet(query, &[body]);
            let ts = f["created_at"].as_str().unwrap_or_default().to_string();
            out.push(SearchResult {
                source: SourceKind::ProjectNote,
                record_id: record_id.clone(),
                title,
                snippet,
                timestamp: ts,
                jump_to: serde_json::json!({
                    "kind": "project_note",
                    "record_id": record_id,
                    "project_code": project_code,
                    "client_code": client_code,
                }),
            });
        }
    }
    Ok(out)
}

async fn search_decisions(query: &str) -> Result<Vec<SearchResult>, String> {
    let formula = build_or_find_formula(query, &["title", "decision", "reasoning"]);
    let qs = format!(
        "filterByFormula={}&pageSize={}\
&fields%5B%5D=id&fields%5B%5D=title&fields%5B%5D=surfaced_at\
&fields%5B%5D=due_date&fields%5B%5D=status&fields%5B%5D=decision_type\
&fields%5B%5D=decision&fields%5B%5D=reasoning\
&fields%5B%5D=client&fields%5B%5D=project",
        urlencode(&formula),
        PER_TABLE_LIMIT
    );
    let data = airtable_get(DECISIONS_TABLE, &qs).await?;
    let mut out = Vec::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let record_id = r["id"].as_str().unwrap_or_default().to_string();
            let title = f["title"].as_str().unwrap_or("Decision").to_string();
            let decision_text = f["decision"].as_str().unwrap_or_default();
            let reasoning = f["reasoning"].as_str().unwrap_or_default();
            let snippet = make_snippet(query, &[decision_text, reasoning, &title]);
            let ts = f["surfaced_at"]
                .as_str()
                .or_else(|| f["due_date"].as_str())
                .unwrap_or_default()
                .to_string();
            out.push(SearchResult {
                source: SourceKind::Decision,
                record_id: record_id.clone(),
                title: title.clone(),
                snippet,
                timestamp: ts,
                jump_to: serde_json::json!({
                    "kind": "decision",
                    "record_id": record_id,
                    "id": record_id,
                    "title": title,
                    "decision": decision_text,
                    "reasoning": reasoning,
                    "due_date": f["due_date"].as_str().unwrap_or(""),
                    "decision_type": f["decision_type"].as_str().unwrap_or(""),
                    "status": f["status"].as_str().unwrap_or(""),
                }),
            });
        }
    }
    Ok(out)
}

async fn search_commitments(query: &str) -> Result<Vec<SearchResult>, String> {
    let formula = build_or_find_formula(query, &["title", "notes"]);
    let qs = format!(
        "filterByFormula={}&pageSize={}\
&fields%5B%5D=id&fields%5B%5D=title&fields%5B%5D=made_at\
&fields%5B%5D=due_at&fields%5B%5D=next_check_at&fields%5B%5D=status\
&fields%5B%5D=surface&fields%5B%5D=priority\
&fields%5B%5D=client&fields%5B%5D=project&fields%5B%5D=notes",
        urlencode(&formula),
        PER_TABLE_LIMIT
    );
    let data = airtable_get(COMMITMENTS_TABLE, &qs).await?;
    let mut out = Vec::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let record_id = r["id"].as_str().unwrap_or_default().to_string();
            let title = f["title"].as_str().unwrap_or("Commitment").to_string();
            let notes = f["notes"].as_str().unwrap_or_default();
            let snippet = make_snippet(query, &[notes, &title]);
            let ts = f["made_at"]
                .as_str()
                .or_else(|| f["due_at"].as_str())
                .unwrap_or_default()
                .to_string();
            out.push(SearchResult {
                source: SourceKind::Commitment,
                record_id: record_id.clone(),
                title: title.clone(),
                snippet,
                timestamp: ts,
                jump_to: serde_json::json!({
                    "kind": "commitment",
                    "record_id": record_id,
                    "id": record_id,
                    "title": title,
                    "notes": notes,
                    "due_at": f["due_at"].as_str().unwrap_or(""),
                    "priority": f["priority"].as_str().unwrap_or(""),
                    "surface": f["surface"].as_str().unwrap_or(""),
                }),
            });
        }
    }
    Ok(out)
}

// ── Snippet helpers ───────────────────────────────────────────────────

// Find the first occurrence of `query` in any of the source strings,
// then return ±SNIPPET_RADIUS characters of context with leading and
// trailing ellipses where the snippet was clipped. The match itself is
// preserved with its original casing — the frontend highlights it.
fn make_snippet(query: &str, sources: &[&str]) -> String {
    let q_lower = query.trim().to_lowercase();
    if q_lower.is_empty() {
        return String::new();
    }
    for src in sources {
        if src.is_empty() {
            continue;
        }
        let lower = src.to_lowercase();
        if let Some(byte_pos) = lower.find(&q_lower) {
            return clip_around(src, byte_pos, q_lower.len());
        }
    }
    // No direct hit — fall back to the first non-empty source's start so
    // the row still has a preview line.
    sources
        .iter()
        .find(|s| !s.is_empty())
        .map(|s| clip_around(s, 0, 0))
        .unwrap_or_default()
}

fn clip_around(src: &str, byte_pos: usize, match_len: usize) -> String {
    // Convert byte offsets to char offsets so we don't slice through a
    // multi-byte sequence. SNIPPET_RADIUS is in chars.
    let chars: Vec<char> = src.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    // Re-derive char index that corresponds to byte_pos. Walk once.
    let mut char_idx = 0;
    let mut byte_acc = 0;
    for (i, c) in chars.iter().enumerate() {
        if byte_acc >= byte_pos {
            char_idx = i;
            break;
        }
        byte_acc += c.len_utf8();
        char_idx = i + 1;
    }
    let start = char_idx.saturating_sub(SNIPPET_RADIUS);
    let match_chars = match_len; // close enough for ASCII queries
    let end = (char_idx + match_chars + SNIPPET_RADIUS).min(chars.len());
    let prefix = if start > 0 { "..." } else { "" };
    let suffix = if end < chars.len() { "..." } else { "" };
    let body: String = chars[start..end].iter().collect();
    let normalised = body.replace(['\n', '\r', '\t'], " ");
    format!("{}{}{}", prefix, normalised.trim(), suffix)
}

// First line, capped at `max` chars. Used for conversation titles where
// we use the auto-summary as the headline.
fn first_line(s: &str, max: usize) -> &str {
    let line = s.lines().next().unwrap_or(s);
    if line.chars().count() <= max {
        line
    } else {
        // Slice on char boundary.
        match line.char_indices().nth(max) {
            Some((idx, _)) => &line[..idx],
            None => line,
        }
    }
}

// Linked record fields render as arrays of record ids OR (when the
// linked table has a primary code field rolling up via lookup) arrays of
// strings. Either way we return the first string present.
fn read_first_string(v: &serde_json::Value) -> Option<String> {
    if let Some(arr) = v.as_array() {
        if let Some(first) = arr.first() {
            if let Some(s) = first.as_str() {
                return Some(s.to_string());
            }
        }
    }
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    None
}
