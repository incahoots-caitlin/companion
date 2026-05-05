// Companion - Google Drive API client (v0.25)
//
// Direct Drive v3 REST. One surface in v0.25:
//
//   - list_recent_files_for_client(folder_id, days)
//
// The Clients table has a `drive_folder_id` field (added 2026-05-06)
// holding the Drive folder ID for that client. When the field is empty
// we return an empty list rather than fanning out across the user's
// whole Drive — by-design, the per-client section just hides itself.
//
// "Recent" means modified in the last N days. The v3 search syntax
// supports both `'<folder_id>' in parents` and `modifiedTime > '<rfc3339>'`,
// so a single q= covers both.

use crate::google;
use crate::oauth;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

// ── Public file shape ─────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub modified_time: String, // RFC3339 — frontend formats it
    pub web_view_link: Option<String>,
    pub icon_link: Option<String>,
    pub modified_by: Option<String>,
}

// ── Top-level helper ──────────────────────────────────────────────────

pub async fn list_recent_files_for_client(
    folder_id: &str,
    days: i64,
) -> Result<Vec<DriveFile>, String> {
    if folder_id.trim().is_empty() {
        return Ok(Vec::new());
    }
    let token = oauth::ensure_fresh_token(&google::GOOGLE)
        .await
        .map_err(|e| format!("Google: {}", e))?;

    let cutoff = Utc::now() - Duration::days(days.max(1));
    // Drive's q= syntax: single-quoted ISO datetime + escaped ID.
    let safe_folder = folder_id.replace('\'', "");
    let q = format!(
        "'{}' in parents and trashed = false and modifiedTime > '{}'",
        safe_folder,
        cutoff.to_rfc3339()
    );

    // fields= keeps the response tight — we only render name, mime, and
    // modified time. corpora=allDrives + supportsAllDrives lets shared
    // drive folders work as well as My Drive.
    let url = format!(
        "{}/files?q={}&orderBy=modifiedTime%20desc&pageSize=20\
&supportsAllDrives=true&includeItemsFromAllDrives=true&corpora=allDrives\
&fields=files(id%2Cname%2CmimeType%2CmodifiedTime%2CwebViewLink%2CiconLink%2ClastModifyingUser(displayName))",
        google::DRIVE_API_BASE,
        urlencode(&q),
    );

    let resp = http_get_json(&url, &token).await?;
    let parsed: FilesResponse = serde_json::from_value(resp)
        .map_err(|e| format!("Drive files parse: {}", e))?;

    let out: Vec<DriveFile> = parsed
        .files
        .into_iter()
        .map(|f| DriveFile {
            id: f.id,
            name: f.name.unwrap_or_else(|| "(untitled)".to_string()),
            mime_type: f.mime_type.unwrap_or_default(),
            modified_time: f.modified_time.unwrap_or_default(),
            web_view_link: f.web_view_link,
            icon_link: f.icon_link,
            modified_by: f
                .last_modifying_user
                .and_then(|u| u.display_name),
        })
        .collect();
    Ok(out)
}

// ── Internals ─────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct FilesResponse {
    #[serde(default)]
    files: Vec<RawFile>,
}

#[derive(Deserialize, Debug)]
struct RawFile {
    id: String,
    name: Option<String>,
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
    #[serde(rename = "modifiedTime")]
    modified_time: Option<String>,
    #[serde(rename = "webViewLink")]
    web_view_link: Option<String>,
    #[serde(rename = "iconLink")]
    icon_link: Option<String>,
    #[serde(rename = "lastModifyingUser")]
    last_modifying_user: Option<RawUser>,
}

#[derive(Deserialize, Debug)]
struct RawUser {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

async fn http_get_json(url: &str, token: &str) -> Result<serde_json::Value, String> {
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
        return Err(format!("Drive {}: {}", status.as_u16(), body));
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
