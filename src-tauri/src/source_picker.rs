// Companion - Source picker (v0.32 Block F)
//
// Routes a workflow's input source to one of the existing connectors and
// returns a normalised markdown context blob. Workflows that opt into
// the source-picker pattern prepend this blob to their Anthropic user
// message so the model has the original brief verbatim.
//
// Source types:
//   - "granola"  → Granola call list (existing v0.22 connector)
//   - "gmail"    → Gmail thread by ID, or first match for a search query
//   - "slack"    → Slack thread URL, or last 7 days of #client-{slug}
//   - "calendar" → Google Calendar event description + attendees
//   - "form"     → Latest Lead Intake / Discovery Pre-Brief Airtable row
//   - "manual"   → Plain paste, wrapped in the same envelope shape
//
// Each path skips silently when its OAuth/MCP isn't connected — the JS
// picker hides unsupported sources at render time, so a properly-wired
// frontend never asks for a missing source. Backend still guards in case
// of misuse.

use crate::{airtable_get, urlencode};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct FetchInput {
    #[serde(default)]
    pub client_code: Option<String>,
    pub source_type: String,
    #[serde(default)]
    pub source_ref: Option<String>,
}

// Wraps body in the standard envelope (header + body) so workflow user
// messages can prepend the blob without branching on source type.
fn envelope(source_label: &str, original_ref: &str, body: &str) -> String {
    let now = chrono::Local::now().format("%A %d %B %Y at %H:%M");
    let header = if original_ref.is_empty() {
        format!("# Source: {}\n\nRetrieved: {}", source_label, now)
    } else {
        format!(
            "# Source: {}\n\nRetrieved: {}\nReference: {}",
            source_label, now, original_ref
        )
    };
    format!("{}\n\n---\n\n{}", header, body.trim())
}

pub async fn fetch_workflow_context(input: FetchInput) -> Result<String, String> {
    let kind = input.source_type.trim().to_lowercase();
    let source_ref = input.source_ref.as_deref().unwrap_or("").trim().to_string();
    let client_code = input
        .client_code
        .as_deref()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty());

    match kind.as_str() {
        "manual" => Ok(envelope("Manual paste", "", &source_ref)),
        "granola" => fetch_granola(client_code.as_deref(), &source_ref).await,
        "gmail" => fetch_gmail(&source_ref).await,
        "slack" => fetch_slack(client_code.as_deref(), &source_ref).await,
        "calendar" => fetch_calendar(&source_ref).await,
        "form" => fetch_form(client_code.as_deref(), &source_ref).await,
        other => Err(format!("Unknown source type: {}", other)),
    }
}

// ── Granola ───────────────────────────────────────────────────────────
//
// Reuses the v0.22 pull pattern. `source_ref` is an optional substring
// that filters meeting titles. When blank we pull the last 14 days for
// the client.

async fn fetch_granola(
    client_code: Option<&str>,
    title_filter: &str,
) -> Result<String, String> {
    let code = client_code.ok_or("Granola source needs a client code")?;
    // Reuse the v0.22 pull but with a tight 14-day window for the picker
    // — workflows that already had the broader window (Monthly = 30,
    // Quarterly = 90) keep their own pull buttons.
    let body = crate::pull_granola_via_mcp(code, 14, None).await?;
    let label = if title_filter.is_empty() {
        format!("Granola — {} (last 14 days)", code)
    } else {
        format!("Granola — {} (filter: {})", code, title_filter)
    };
    Ok(envelope(&label, title_filter, &body))
}

// ── Gmail ─────────────────────────────────────────────────────────────
//
// `source_ref` is either a Gmail thread ID (~16 hex chars) or a Gmail
// search expression. We try the thread fetch first; on parse failure we
// fall back to a search and grab the first result.

async fn fetch_gmail(source_ref: &str) -> Result<String, String> {
    if source_ref.is_empty() {
        return Err("Gmail source needs a thread ID or search expression".to_string());
    }
    let blob = crate::fetch_gmail_for_picker(source_ref).await?;
    Ok(envelope("Gmail thread", source_ref, &blob))
}

// ── Slack ─────────────────────────────────────────────────────────────
//
// `source_ref` is a Slack message permalink, or blank to pull the last 7
// days of #client-{slug}. Permalink format:
//   https://<workspace>.slack.com/archives/<channel_id>/p<ts_with_dot_removed>

async fn fetch_slack(
    client_code: Option<&str>,
    source_ref: &str,
) -> Result<String, String> {
    if !source_ref.is_empty() {
        let body = crate::fetch_slack_thread_for_picker(source_ref).await?;
        return Ok(envelope("Slack thread", source_ref, &body));
    }
    let code = client_code.ok_or("Slack source needs a client code or thread URL")?;
    // Resolve client name → slug, reuse list_channel_activity_for_client
    // with the existing 24h window (it returns Option<...>).
    let qs = format!(
        "filterByFormula={}&maxRecords=1&fields%5B%5D=name",
        urlencode(&format!("{{code}}='{}'", code))
    );
    let data = airtable_get("Clients", &qs)
        .await
        .unwrap_or(serde_json::Value::Null);
    let name = data["records"][0]["fields"]["name"]
        .as_str()
        .unwrap_or(code)
        .to_string();
    let slug = crate::slugify_client_name(&name);
    let activity = crate::slack::list_channel_activity_for_client(&slug)
        .await
        .map_err(|e| format!("Slack: {}", e))?;
    let body = match activity {
        Some(a) => format_slack_activity(&a),
        None => format!("(No #client-{} channel found in Slack.)", slug),
    };
    Ok(envelope(
        &format!("Slack #client-{} (last 24h)", slug),
        "",
        &body,
    ))
}

fn format_slack_activity(a: &crate::slack::ClientChannelActivity) -> String {
    let mut out = format!("**Channel:** #{}\n\n", a.channel.name);
    if a.messages.is_empty() {
        out.push_str("(No messages in the last 24 hours.)");
        return out;
    }
    for m in &a.messages {
        let user = m
            .user_name
            .as_deref()
            .or(m.user.as_deref())
            .unwrap_or("(unknown)");
        out.push_str(&format!("**{}** ({}):\n{}\n\n", user, m.ts, m.text));
    }
    out.trim_end().to_string()
}

// ── Calendar ──────────────────────────────────────────────────────────
//
// `source_ref` is a Google Calendar event ID. When a full date is passed
// (YYYY-MM-DD) we fall back to "first event on this date".

async fn fetch_calendar(source_ref: &str) -> Result<String, String> {
    if source_ref.is_empty() {
        return Err("Calendar source needs an event ID or date".to_string());
    }
    let body = crate::fetch_calendar_event_for_picker(source_ref).await?;
    Ok(envelope("Calendar event", source_ref, &body))
}

// ── Form submission ───────────────────────────────────────────────────
//
// Reads the latest row from the requested Airtable form table. Default
// table is `Leads` (Lead Intake form). `source_ref` can be
// `form_lead_intake` (Leads) or `form_discovery_pre_brief` (Projects)
// or a literal Airtable table name.

async fn fetch_form(
    client_code: Option<&str>,
    source_ref: &str,
) -> Result<String, String> {
    let key = if source_ref.is_empty() {
        "form_lead_intake"
    } else {
        source_ref
    };
    let (table, filter_name) = match key {
        "form_lead_intake" | "Leads" => ("Leads", None),
        "form_discovery_pre_brief" | "Projects" => ("Projects", Some("client_code")),
        "form_post_campaign_feedback" => ("Projects", Some("client_code")),
        other => (other, None),
    };

    let mut params: Vec<String> = Vec::new();
    if let (Some(field), Some(code)) = (filter_name, client_code) {
        params.push(format!(
            "filterByFormula={}",
            urlencode(&format!("{{{}}}='{}'", field, code))
        ));
    }
    params.push("maxRecords=1".to_string());
    params.push("sort%5B0%5D%5Bfield%5D=created_at".to_string());
    params.push("sort%5B0%5D%5Bdirection%5D=desc".to_string());
    let qs = params.join("&");

    let data = airtable_get(table, &qs)
        .await
        .map_err(|e| format!("Airtable form read: {}", e))?;
    let records = data["records"].as_array().cloned().unwrap_or_default();
    if records.is_empty() {
        return Err(format!(
            "No {} submission found for the requested client",
            table
        ));
    }
    let fields = &records[0]["fields"];
    let mut body = String::new();
    if let Some(obj) = fields.as_object() {
        for (k, v) in obj.iter() {
            let value_text = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => v.to_string(),
            };
            body.push_str(&format!("- **{}:** {}\n", k, value_text.trim()));
        }
    }
    Ok(envelope(
        &format!("{} submission", table),
        key,
        body.trim_end(),
    ))
}
