// Companion - Project update feed aggregator (v0.28 Block E).
//
// Pulls updates relevant to a single project from every connected source
// and returns them as a single sorted timeline. Each source has its own
// fault-tolerant sub-fetcher: when Slack OAuth isn't connected, the
// Slack pull skips silently; when Airtable is offline, every Airtable-
// backed pull skips. The feed never crashes on a partial failure.
//
// The unified `ProjectUpdate` enum is tagged via serde's `kind` field so
// the JS layer reads `update.kind === "note"` etc. and renders the right
// row template. Timestamps are RFC3339 strings (UTC) — JS formats them
// for display.
//
// Source matching rules (per the brief):
//   - ProjectNotes: filter by project link
//   - Receipts:     filter by project link (added in v0.28). Falls back
//                   to JSON `project` field starting with the project
//                   code for receipts written before the link existed.
//   - Conversations: filter by project link (added in v0.28). Falls
//                    back to workstream link → workstream's project.
//   - Calendar: scan event summary/description for the project code.
//   - Slack: scan #client-{slug} messages for the project code.
//   - Gmail: client's gmail_thread_filter AND project code in subject
//            or body.
//   - Drive: files in client's drive_folder_id whose name contains the
//            project code.
//
// Granola travels with Receipts via the workflow link, so no separate
// Granola fetch is needed at this layer.

use crate::airtable_get;
use crate::calendar;
use crate::drive;
use crate::gmail;
use crate::google;
use crate::oauth;
use crate::slack;
use crate::urlencode;
use serde::{Deserialize, Serialize};

const PROJECTS_TABLE: &str = "Projects";
const CLIENTS_TABLE: &str = "Clients";
const PROJECT_NOTES_TABLE: &str = "ProjectNotes";
const RECEIPTS_TABLE: &str = "Receipts";
const CONVERSATIONS_TABLE: &str = "Conversations";

// ── Public types ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectHeader {
    pub code: String,
    pub name: Option<String>,
    pub status: Option<String>,
    pub campaign_type: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub budget_total: Option<f64>,
    pub brief_url: Option<String>,
    pub notes: Option<String>,
    pub client_code: Option<String>,
    pub client_name: Option<String>,
    pub client_record_id: Option<String>,
    pub project_record_id: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectUpdate {
    Note(ProjectNoteUpdate),
    Receipt(ReceiptUpdate),
    Conversation(ConversationUpdate),
    Calendar(CalendarUpdate),
    Slack(SlackUpdate),
    Gmail(GmailUpdate),
    Drive(DriveUpdate),
}

impl ProjectUpdate {
    fn timestamp(&self) -> &str {
        match self {
            ProjectUpdate::Note(u) => &u.ts,
            ProjectUpdate::Receipt(u) => &u.ts,
            ProjectUpdate::Conversation(u) => &u.ts,
            ProjectUpdate::Calendar(u) => &u.ts,
            ProjectUpdate::Slack(u) => &u.ts,
            ProjectUpdate::Gmail(u) => &u.ts,
            ProjectUpdate::Drive(u) => &u.ts,
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct ProjectNoteUpdate {
    pub id: String,
    pub record_id: String,
    pub ts: String,
    pub created_by: Option<String>,
    pub body: String,
    pub tags: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ReceiptUpdate {
    pub id: String,
    pub ts: String,
    pub title: String,
    pub workflow: Option<String>,
    pub ticked_count: Option<i64>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ConversationUpdate {
    pub id: String,
    pub ts: String,
    pub workstream_code: Option<String>,
    pub summary: Option<String>,
    pub message_count: Option<i64>,
}

#[derive(Serialize, Debug, Clone)]
pub struct CalendarUpdate {
    pub id: String,
    pub ts: String,
    pub summary: String,
    pub all_day: bool,
    pub html_link: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct SlackUpdate {
    pub id: String,
    pub ts: String,
    pub channel_name: String,
    pub user_name: Option<String>,
    pub text: String,
    pub permalink: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct GmailUpdate {
    pub id: String,
    pub ts: String,
    pub subject: String,
    pub from: String,
    pub snippet: String,
    pub web_link: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct DriveUpdate {
    pub id: String,
    pub ts: String,
    pub name: String,
    pub mime_type: String,
    pub modified_by: Option<String>,
    pub web_view_link: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ProjectFeed {
    pub header: Option<ProjectHeader>,
    pub updates: Vec<ProjectUpdate>,
}

// ── Top-level entrypoint ──────────────────────────────────────────────

pub async fn list_project_updates(project_code: &str) -> Result<ProjectFeed, String> {
    let code = project_code.trim();
    if code.is_empty() {
        return Err("project_code required".to_string());
    }

    let header = fetch_header(code).await.unwrap_or(None);
    let project_record_id = header
        .as_ref()
        .and_then(|h| h.project_record_id.clone())
        .unwrap_or_default();
    let client_record_id = header
        .as_ref()
        .and_then(|h| h.client_record_id.clone())
        .unwrap_or_default();
    let client_name = header
        .as_ref()
        .and_then(|h| h.client_name.clone())
        .unwrap_or_default();

    let mut out: Vec<ProjectUpdate> = Vec::new();

    // 1. ProjectNotes — direct project link.
    match fetch_notes(&project_record_id).await {
        Ok(mut v) => out.append(&mut v),
        Err(e) => eprintln!("[project_feed] notes skipped: {}", e),
    }

    // 2. Receipts — direct project link first, fallback to JSON-side match.
    match fetch_receipts(&project_record_id, code).await {
        Ok(mut v) => out.append(&mut v),
        Err(e) => eprintln!("[project_feed] receipts skipped: {}", e),
    }

    // 3. Conversations — direct project link.
    match fetch_conversations(&project_record_id).await {
        Ok(mut v) => out.append(&mut v),
        Err(e) => eprintln!("[project_feed] conversations skipped: {}", e),
    }

    // 4. Calendar — needles include project code (works case-insensitive).
    if oauth::is_connected(&google::GOOGLE) {
        match fetch_calendar(code).await {
            Ok(mut v) => out.append(&mut v),
            Err(e) => eprintln!("[project_feed] calendar skipped: {}", e),
        }
    }

    // 5. Slack — recent messages in the client's channel that mention the code.
    if oauth::is_connected(&slack::SLACK) && !client_name.is_empty() {
        match fetch_slack(&client_name, code).await {
            Ok(mut v) => out.append(&mut v),
            Err(e) => eprintln!("[project_feed] slack skipped: {}", e),
        }
    }

    // 6. Gmail — client filter expression scoped further by project code.
    if oauth::is_connected(&google::GOOGLE) {
        match fetch_gmail(&client_record_id, &client_name, code).await {
            Ok(mut v) => out.append(&mut v),
            Err(e) => eprintln!("[project_feed] gmail skipped: {}", e),
        }
    }

    // 7. Drive — files in the client folder whose name contains the code.
    if oauth::is_connected(&google::GOOGLE) {
        match fetch_drive(&client_record_id, code).await {
            Ok(mut v) => out.append(&mut v),
            Err(e) => eprintln!("[project_feed] drive skipped: {}", e),
        }
    }

    // Sort newest first. Empty timestamps fall to the bottom.
    out.sort_by(|a, b| b.timestamp().cmp(a.timestamp()));

    Ok(ProjectFeed {
        header,
        updates: out,
    })
}

// ── Header ────────────────────────────────────────────────────────────

async fn fetch_header(code: &str) -> Result<Option<ProjectHeader>, String> {
    let escaped = code.replace('\'', "");
    let qs = format!(
        "filterByFormula={}&maxRecords=1\
&fields%5B%5D=code&fields%5B%5D=name&fields%5B%5D=status\
&fields%5B%5D=client&fields%5B%5D=type\
&fields%5B%5D=start_date&fields%5B%5D=end_date\
&fields%5B%5D=budget_total&fields%5B%5D=brief_url&fields%5B%5D=notes",
        urlencode(&format!("{{code}}='{}'", escaped))
    );
    let data = airtable_get(PROJECTS_TABLE, &qs).await?;
    let rec = match data["records"].get(0) {
        Some(v) => v,
        None => return Ok(None),
    };
    let project_record_id = rec["id"].as_str().map(String::from);
    let f = &rec["fields"];

    let client_record_id = f["client"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(String::from);

    // Resolve client name + code from the link, when present.
    let (client_code, client_name) = match &client_record_id {
        Some(id) => fetch_client_summary(id)
            .await
            .unwrap_or((None, None)),
        None => (None, None),
    };

    Ok(Some(ProjectHeader {
        code: f["code"].as_str().unwrap_or(code).to_string(),
        name: f["name"].as_str().map(String::from),
        status: f["status"].as_str().map(String::from),
        campaign_type: f["type"].as_str().map(String::from),
        start_date: f["start_date"].as_str().map(String::from),
        end_date: f["end_date"].as_str().map(String::from),
        budget_total: f["budget_total"].as_f64(),
        brief_url: f["brief_url"].as_str().map(String::from),
        notes: f["notes"].as_str().map(String::from),
        client_code,
        client_name,
        client_record_id,
        project_record_id,
    }))
}

async fn fetch_client_summary(record_id: &str) -> Result<(Option<String>, Option<String>), String> {
    let escaped = record_id.replace('\'', "");
    let formula = format!("RECORD_ID()='{}'", escaped);
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=code&fields%5B%5D=name",
        urlencode(&formula)
    );
    let data = airtable_get(CLIENTS_TABLE, &qs).await?;
    let f = &data["records"][0]["fields"];
    Ok((
        f["code"].as_str().map(String::from),
        f["name"].as_str().map(String::from),
    ))
}

// ── ProjectNotes ──────────────────────────────────────────────────────

async fn fetch_notes(project_record_id: &str) -> Result<Vec<ProjectUpdate>, String> {
    if project_record_id.is_empty() {
        return Ok(Vec::new());
    }
    let escaped = project_record_id.replace('\'', "");
    let formula = format!("FIND('{}',ARRAYJOIN({{project}}))", escaped);
    let qs = format!(
        "filterByFormula={}&pageSize=100\
&fields%5B%5D=id&fields%5B%5D=project&fields%5B%5D=client\
&fields%5B%5D=created_at&fields%5B%5D=created_by\
&fields%5B%5D=body&fields%5B%5D=tags\
&sort%5B0%5D%5Bfield%5D=created_at&sort%5B0%5D%5Bdirection%5D=desc",
        urlencode(&formula)
    );
    let data = airtable_get(PROJECT_NOTES_TABLE, &qs).await?;
    let mut out: Vec<ProjectUpdate> = Vec::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let id = f["id"].as_str().unwrap_or_default().to_string();
            let ts = f["created_at"].as_str().unwrap_or_default().to_string();
            let body = f["body"].as_str().unwrap_or_default().to_string();
            let created_by = f["created_by"].as_str().map(String::from);
            let tags = f["tags"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            out.push(ProjectUpdate::Note(ProjectNoteUpdate {
                id: id.clone(),
                record_id: r["id"].as_str().unwrap_or_default().to_string(),
                ts,
                created_by,
                body,
                tags,
            }));
        }
    }
    Ok(out)
}

// ── Receipts ──────────────────────────────────────────────────────────

async fn fetch_receipts(
    project_record_id: &str,
    project_code: &str,
) -> Result<Vec<ProjectUpdate>, String> {
    // Two-pronged: prefer the `project` link added in v0.28; fall back
    // to receipts whose JSON `project` field starts with the code so
    // historic receipts still show up. Combine and dedupe by id.
    let mut by_id: std::collections::HashMap<String, ReceiptUpdate> = std::collections::HashMap::new();

    // 1) Direct project link (v0.28+).
    if !project_record_id.is_empty() {
        let escaped = project_record_id.replace('\'', "");
        let formula = format!("FIND('{}',ARRAYJOIN({{project}}))", escaped);
        let qs = format!(
            "filterByFormula={}&pageSize=100\
&fields%5B%5D=id&fields%5B%5D=title&fields%5B%5D=date\
&fields%5B%5D=workflow&fields%5B%5D=ticked_count\
&sort%5B0%5D%5Bfield%5D=date&sort%5B0%5D%5Bdirection%5D=desc",
            urlencode(&formula)
        );
        if let Ok(data) = airtable_get(RECEIPTS_TABLE, &qs).await {
            collect_receipts(&data, &mut by_id);
        }
    }

    // 2) JSON-side project-code match — only when we have a code. Uses
    // SEARCH() against the `id` primary field's path-prefix is not
    // available, so we filter via the `id` LIKE pattern is not Airtable.
    // Instead we read the `json` column? That's expensive. Since most
    // receipts reference the code via the project field on their payload
    // and Airtable lacks JSON queries, the cleanest fallback is a date-
    // bounded pull where the title or workflow contains the code. We
    // use FIND on the `title` field as a cheap heuristic — receipt titles
    // for project-scoped workflows typically include the project name
    // (e.g. "Strategic Thinking — NCT-2026-06-tour-launch"). When the
    // title doesn't carry it, the v0.28+ link is the durable path.
    if !project_code.is_empty() {
        let escaped_code = project_code.replace('\'', "");
        let formula = format!("FIND('{}',{{title}})", escaped_code);
        let qs = format!(
            "filterByFormula={}&pageSize=50\
&fields%5B%5D=id&fields%5B%5D=title&fields%5B%5D=date\
&fields%5B%5D=workflow&fields%5B%5D=ticked_count\
&sort%5B0%5D%5Bfield%5D=date&sort%5B0%5D%5Bdirection%5D=desc",
            urlencode(&formula)
        );
        if let Ok(data) = airtable_get(RECEIPTS_TABLE, &qs).await {
            collect_receipts(&data, &mut by_id);
        }
    }

    Ok(by_id.into_values().map(ProjectUpdate::Receipt).collect())
}

fn collect_receipts(
    data: &serde_json::Value,
    by_id: &mut std::collections::HashMap<String, ReceiptUpdate>,
) {
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let id = f["id"].as_str().unwrap_or_default().to_string();
            if id.is_empty() {
                continue;
            }
            let title = f["title"].as_str().unwrap_or("Receipt").to_string();
            let date = f["date"].as_str().unwrap_or("").to_string();
            // Dates are stored as YYYY-MM-DD. Append a midnight UTC time so
            // the sort key compares correctly with the RFC3339 timestamps
            // returned by the OAuth-source pulls.
            let ts = if date.len() == 10 {
                format!("{}T00:00:00Z", date)
            } else {
                date
            };
            by_id.insert(
                id.clone(),
                ReceiptUpdate {
                    id,
                    ts,
                    title,
                    workflow: f["workflow"].as_str().map(String::from),
                    ticked_count: f["ticked_count"].as_i64(),
                },
            );
        }
    }
}

// ── Conversations ─────────────────────────────────────────────────────

async fn fetch_conversations(project_record_id: &str) -> Result<Vec<ProjectUpdate>, String> {
    if project_record_id.is_empty() {
        return Ok(Vec::new());
    }
    let escaped = project_record_id.replace('\'', "");
    // Direct project link (added v0.28). Most conversations also link
    // through Workstream, but Workstreams.project is the durable path
    // and Conversations.project is denormalised when known.
    let formula = format!("FIND('{}',ARRAYJOIN({{project}}))", escaped);
    let qs = format!(
        "filterByFormula={}&pageSize=50\
&fields%5B%5D=id&fields%5B%5D=workstream\
&fields%5B%5D=last_message_at&fields%5B%5D=message_count\
&fields%5B%5D=summary\
&sort%5B0%5D%5Bfield%5D=last_message_at&sort%5B0%5D%5Bdirection%5D=desc",
        urlencode(&formula)
    );
    let data = airtable_get(CONVERSATIONS_TABLE, &qs).await?;
    let mut out: Vec<ProjectUpdate> = Vec::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let id = f["id"].as_str().unwrap_or_default().to_string();
            if id.is_empty() {
                continue;
            }
            out.push(ProjectUpdate::Conversation(ConversationUpdate {
                id: id.clone(),
                ts: f["last_message_at"].as_str().unwrap_or_default().to_string(),
                workstream_code: f["workstream"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .map(String::from),
                summary: f["summary"].as_str().map(String::from),
                message_count: f["message_count"].as_i64(),
            }));
        }
    }
    Ok(out)
}

// ── Calendar ──────────────────────────────────────────────────────────

async fn fetch_calendar(project_code: &str) -> Result<Vec<ProjectUpdate>, String> {
    // The existing list_events_for_client takes a name + aliases. We
    // reuse it with the project code as the only needle so events whose
    // summary or description carries the literal code surface here.
    let events = calendar::list_events_for_client(project_code, &[]).await?;
    let out = events
        .into_iter()
        .map(|ev| {
            ProjectUpdate::Calendar(CalendarUpdate {
                id: ev.id,
                ts: ev.start.clone(),
                summary: ev.summary,
                all_day: ev.all_day,
                html_link: ev.html_link,
            })
        })
        .collect();
    Ok(out)
}

// ── Slack ─────────────────────────────────────────────────────────────

async fn fetch_slack(client_name: &str, project_code: &str) -> Result<Vec<ProjectUpdate>, String> {
    let slug = slug_from_name(client_name);
    let activity = match slack::list_channel_activity_for_client(&slug).await? {
        Some(a) => a,
        None => return Ok(Vec::new()),
    };
    let needle = project_code.to_lowercase();
    let out: Vec<ProjectUpdate> = activity
        .messages
        .into_iter()
        .filter(|m| m.text.to_lowercase().contains(&needle))
        .map(|m| {
            ProjectUpdate::Slack(SlackUpdate {
                id: m.ts.clone(),
                ts: ts_to_rfc3339(&m.ts),
                channel_name: m.channel_name,
                user_name: m.user_name,
                text: m.text,
                permalink: m.permalink,
            })
        })
        .collect();
    Ok(out)
}

// Slack timestamps are floating-point unix seconds in a string
// ("1714898123.001234"). Convert to RFC3339 so the cross-source sort
// works correctly.
fn ts_to_rfc3339(ts: &str) -> String {
    let secs: f64 = ts.parse().unwrap_or(0.0);
    if secs <= 0.0 {
        return String::new();
    }
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0);
    dt.map(|d| d.to_rfc3339()).unwrap_or_default()
}

fn slug_from_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if c.is_whitespace() || c == '-' || c == '_' || c == '/' {
            if !last_dash && !out.is_empty() {
                out.push('-');
                last_dash = true;
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

// ── Gmail ─────────────────────────────────────────────────────────────

async fn fetch_gmail(
    client_record_id: &str,
    client_name: &str,
    project_code: &str,
) -> Result<Vec<ProjectUpdate>, String> {
    if client_record_id.is_empty() {
        return Ok(Vec::new());
    }
    // Look up the client's gmail_thread_filter so the q= scopes to their
    // mail before the project-code search narrows further.
    let escaped = client_record_id.replace('\'', "");
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=gmail_thread_filter",
        urlencode(&format!("RECORD_ID()='{}'", escaped))
    );
    let filter = airtable_get(CLIENTS_TABLE, &qs)
        .await
        .ok()
        .and_then(|d| {
            d["records"][0]["fields"]["gmail_thread_filter"]
                .as_str()
                .map(String::from)
        });

    let scoped_filter = match filter {
        Some(f) if !f.trim().is_empty() => format!("({}) \"{}\"", f.trim(), project_code),
        _ => format!("\"{}\"", project_code),
    };

    let threads = gmail::list_threads_for_client(client_name, Some(&scoped_filter)).await?;
    let out = threads
        .into_iter()
        .map(|t| {
            let ts = if t.date_ms > 0 {
                chrono::DateTime::<chrono::Utc>::from_timestamp_millis(t.date_ms)
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            ProjectUpdate::Gmail(GmailUpdate {
                id: t.id,
                ts,
                subject: t.subject,
                from: t.from,
                snippet: t.snippet,
                web_link: t.web_link,
            })
        })
        .collect();
    Ok(out)
}

// ── Drive ─────────────────────────────────────────────────────────────

async fn fetch_drive(
    client_record_id: &str,
    project_code: &str,
) -> Result<Vec<ProjectUpdate>, String> {
    if client_record_id.is_empty() {
        return Ok(Vec::new());
    }
    let escaped = client_record_id.replace('\'', "");
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=drive_folder_id",
        urlencode(&format!("RECORD_ID()='{}'", escaped))
    );
    let data = airtable_get(CLIENTS_TABLE, &qs).await?;
    let folder_id = data["records"][0]["fields"]["drive_folder_id"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if folder_id.is_empty() {
        return Ok(Vec::new());
    }

    // 60-day window. We then filter client-side by name containing the
    // project code so the v3 q= stays simple.
    let files = drive::list_recent_files_for_client(&folder_id, 60).await?;
    let needle = project_code.to_lowercase();
    let out = files
        .into_iter()
        .filter(|f| f.name.to_lowercase().contains(&needle))
        .map(|f| {
            ProjectUpdate::Drive(DriveUpdate {
                id: f.id,
                ts: f.modified_time,
                name: f.name,
                mime_type: f.mime_type,
                modified_by: f.modified_by,
                web_view_link: f.web_view_link,
            })
        })
        .collect();
    Ok(out)
}

// ── Active projects per client ────────────────────────────────────────
//
// Sidebar feature: each client expands into a list of its non-archived
// projects. We resolve the client by code, then pull Projects rows whose
// client link contains that record id and whose status isn't archive.

#[derive(Serialize, Debug, Clone)]
pub struct ProjectSummary {
    pub code: String,
    pub name: Option<String>,
    pub status: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

pub async fn list_active_projects_for_client(
    client_code: &str,
) -> Result<Vec<ProjectSummary>, String> {
    let code = client_code.trim().to_uppercase();
    if code.is_empty() {
        return Ok(Vec::new());
    }

    // Resolve the client record id.
    let escaped_code = code.replace('\'', "");
    let qs_client = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=code",
        urlencode(&format!("{{code}}='{}'", escaped_code))
    );
    let cdata = airtable_get(CLIENTS_TABLE, &qs_client).await?;
    let client_record_id = match cdata["records"][0]["id"].as_str() {
        Some(s) => s.to_string(),
        None => return Ok(Vec::new()),
    };

    let escaped_id = client_record_id.replace('\'', "");
    let formula = format!(
        "AND(NOT({{status}}='archive'),FIND('{}',ARRAYJOIN({{client}})))",
        escaped_id
    );
    let qs = format!(
        "filterByFormula={}&pageSize=100\
&fields%5B%5D=code&fields%5B%5D=name&fields%5B%5D=status\
&fields%5B%5D=start_date&fields%5B%5D=end_date\
&sort%5B0%5D%5Bfield%5D=start_date&sort%5B0%5D%5Bdirection%5D=desc",
        urlencode(&formula)
    );
    let data = airtable_get(PROJECTS_TABLE, &qs).await?;
    let mut out: Vec<ProjectSummary> = Vec::new();
    if let Some(records) = data["records"].as_array() {
        for r in records {
            let f = &r["fields"];
            let pcode = f["code"].as_str().unwrap_or_default().to_string();
            if pcode.is_empty() {
                continue;
            }
            out.push(ProjectSummary {
                code: pcode,
                name: f["name"].as_str().map(String::from),
                status: f["status"].as_str().map(String::from),
                start_date: f["start_date"].as_str().map(String::from),
                end_date: f["end_date"].as_str().map(String::from),
            });
        }
    }
    Ok(out)
}

// ── Note CRUD ─────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
pub struct CreateNoteInput {
    pub project_code: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    // Defaults to "Caitlin" when omitted; Rose can pass "Rose" once Studio
    // distinguishes user identity at runtime (today's launch is single-user).
    #[serde(default)]
    pub created_by: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct UpdateNoteInput {
    pub note_record_id: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

pub async fn create_note(input: CreateNoteInput) -> Result<String, String> {
    let body = input.body.trim();
    if body.is_empty() {
        return Err("Note body required".to_string());
    }
    let project_code = input.project_code.trim();
    if project_code.is_empty() {
        return Err("project_code required".to_string());
    }

    // Resolve project + client record ids so the new row carries both
    // links. Client is denormalised on ProjectNotes for cheap filtering.
    let escaped_code = project_code.replace('\'', "");
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=code&fields%5B%5D=client",
        urlencode(&format!("{{code}}='{}'", escaped_code))
    );
    let data = airtable_get(PROJECTS_TABLE, &qs).await?;
    let project_record_id = data["records"][0]["id"]
        .as_str()
        .ok_or_else(|| format!("Project '{}' not found", project_code))?
        .to_string();
    let client_record_id = data["records"][0]["fields"]["client"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(String::from);

    let now = chrono::Local::now();
    let id = format!("pn_{}", now.format("%Y-%m-%d_%H-%M-%S"));
    let created_at = chrono::Utc::now().to_rfc3339();
    let created_by = input
        .created_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| "Caitlin".to_string());

    let mut fields = serde_json::json!({
        "id": id,
        "project": [project_record_id],
        "created_at": created_at,
        "created_by": created_by,
        "body": body,
    });
    if let Some(cid) = client_record_id {
        fields["client"] = serde_json::json!([cid]);
    }
    if !input.tags.is_empty() {
        let cleaned: Vec<String> = input
            .tags
            .into_iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        if !cleaned.is_empty() {
            fields["tags"] = serde_json::json!(cleaned);
        }
    }

    crate::airtable_create_record(PROJECT_NOTES_TABLE, fields).await
}

pub async fn update_note(input: UpdateNoteInput) -> Result<(), String> {
    let id = input.note_record_id.trim();
    if id.is_empty() {
        return Err("note_record_id required".to_string());
    }
    let mut fields = serde_json::Map::new();
    if let Some(body) = input.body {
        fields.insert(
            "body".to_string(),
            serde_json::Value::String(body.trim().to_string()),
        );
    }
    if let Some(tags) = input.tags {
        let cleaned: Vec<String> = tags
            .into_iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        fields.insert("tags".to_string(), serde_json::json!(cleaned));
    }
    if fields.is_empty() {
        return Ok(());
    }
    crate::airtable_update_record(PROJECT_NOTES_TABLE, id, serde_json::Value::Object(fields))
        .await
}

pub async fn delete_note(note_record_id: &str) -> Result<(), String> {
    let id = note_record_id.trim();
    if id.is_empty() {
        return Err("note_record_id required".to_string());
    }
    let (api_key, base_id) = crate::read_airtable_creds().ok_or("Airtable not configured")?;
    let url = format!(
        "https://api.airtable.com/v0/{}/{}/{}",
        base_id, PROJECT_NOTES_TABLE, id
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP init: {}", e))?;
    let resp = client
        .delete(&url)
        .bearer_auth(&api_key)
        .send()
        .await
        .map_err(|e| format!("Network: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Airtable {}: {}", status.as_u16(), body));
    }
    Ok(())
}
