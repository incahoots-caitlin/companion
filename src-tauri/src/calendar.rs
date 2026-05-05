// Companion - Google Calendar API client (v0.24)
//
// Direct Google Calendar v3 REST. Calls run from the Rust backend so the
// Anthropic Messages API token spend stays zero on the dashboard slot —
// the brief reserves Option B (MCP via Messages API) for v0.25+ workflows
// that need calendar reasoning.
//
// Fetches the authenticated user's primary calendar plus any other
// calendars they own/subscribe to via singleEvents=true expansion. Two
// time windows: today (local-day boundaries) and the next 7 days.
//
// Per-client lookup (`list_events_for_client`) is a heuristic: an event
// belongs to a client if its summary, description or attendee list
// contains the client name (case-insensitive substring). The Clients
// table's `gmail_thread_filter` field is consulted as an extra alias
// hint when set — Caitlin uses this for clients whose meetings are
// usually with people whose addresses don't match the client name.

use crate::google;
use crate::oauth;
use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};

// ── Public event shape ────────────────────────────────────────────────
//
// The frontend reads these fields directly. Times come back as RFC3339
// strings (matching what render.js's fmtDateTime / fmtTime expect) plus a
// flag for all-day events.

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CalendarEvent {
    pub id: String,
    pub calendar_id: String,
    pub calendar_name: Option<String>,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    pub start: String,        // RFC3339 (or YYYY-MM-DD for all-day)
    pub end: String,
    pub all_day: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hangout_link: Option<String>, // Google Meet URL when present
    pub attendees: Vec<String>,        // email addresses, organiser excluded
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

// ── Top-level helpers ─────────────────────────────────────────────────

pub async fn list_events_today() -> Result<Vec<CalendarEvent>, String> {
    let now = Local::now();
    let start = local_day_start(now).with_timezone(&Utc);
    let end = local_day_end(now).with_timezone(&Utc);
    list_events_in_window(start, end).await
}

pub async fn list_events_week() -> Result<Vec<CalendarEvent>, String> {
    // "Next 7 days" = today + 6 more days. Tomorrow morning shows up,
    // and so does anything later this week. Caitlin opens Companion on
    // a Monday morning and sees Mon-Sun; opens it on Friday and sees
    // Fri-Thu. Consistent rolling window.
    let now = Local::now();
    let start = local_day_start(now).with_timezone(&Utc);
    let end = (local_day_end(now) + Duration::days(6)).with_timezone(&Utc);
    list_events_in_window(start, end).await
}

// Match heuristic: the event's summary, description or attendee list
// contains the client name (case-insensitive). When `extra_aliases` is
// set (sourced from the Clients table's `gmail_thread_filter` field),
// each alias is checked too. Window is the next 14 days so per-client
// view shows enough scheduled meetings to be useful without being noisy.
pub async fn list_events_for_client(
    client_name: &str,
    extra_aliases: &[String],
) -> Result<Vec<CalendarEvent>, String> {
    let now = Local::now();
    let start = local_day_start(now).with_timezone(&Utc);
    let end = (local_day_end(now) + Duration::days(14)).with_timezone(&Utc);
    let all = list_events_in_window(start, end).await?;
    let needles = build_needles(client_name, extra_aliases);
    if needles.is_empty() {
        return Ok(Vec::new());
    }
    let filtered: Vec<CalendarEvent> = all
        .into_iter()
        .filter(|ev| event_matches(ev, &needles))
        .collect();
    Ok(filtered)
}

fn build_needles(client_name: &str, extra_aliases: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push = |out: &mut Vec<String>, s: &str| {
        let trimmed = s.trim();
        if trimmed.len() >= 3 {
            out.push(trimmed.to_lowercase());
        }
    };
    push(&mut out, client_name);
    for alias in extra_aliases {
        // Aliases can come in as comma- or whitespace-separated lists
        // (the Airtable field is free text). Split aggressively so a
        // value like "untitled, abletons, abletonsfest" turns into
        // three needles.
        for piece in alias.split(|c: char| c == ',' || c == ';' || c == '\n') {
            push(&mut out, piece);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn event_matches(ev: &CalendarEvent, needles: &[String]) -> bool {
    // Build a single haystack so we only lowercase once per event.
    let mut haystack = String::new();
    haystack.push_str(&ev.summary);
    haystack.push(' ');
    if let Some(d) = &ev.description {
        haystack.push_str(d);
        haystack.push(' ');
    }
    if let Some(l) = &ev.location {
        haystack.push_str(l);
        haystack.push(' ');
    }
    for a in &ev.attendees {
        haystack.push_str(a);
        haystack.push(' ');
    }
    let lower = haystack.to_lowercase();
    needles.iter().any(|n| lower.contains(n))
}

// ── Calendar list + event listing ─────────────────────────────────────

#[derive(Deserialize, Debug)]
struct CalendarListResponse {
    items: Option<Vec<CalendarListEntry>>,
}

#[derive(Deserialize, Debug)]
struct CalendarListEntry {
    id: String,
    summary: Option<String>,
    #[serde(default)]
    selected: Option<bool>,
    #[serde(default)]
    primary: Option<bool>,
    // "owner", "writer", "reader", "freeBusyReader". freeBusyReader
    // gives us no event details so we skip them.
    #[serde(rename = "accessRole", default)]
    access_role: Option<String>,
}

async fn list_user_calendars(token: &str) -> Result<Vec<CalendarListEntry>, String> {
    let url = format!("{}/users/me/calendarList?minAccessRole=reader", google::CALENDAR_API_BASE);
    let resp = http_get_json(&url, token).await?;
    let parsed: CalendarListResponse = serde_json::from_value(resp)
        .map_err(|e| format!("calendarList parse: {}", e))?;
    let mut items = parsed.items.unwrap_or_default();
    // Keep calendars the user actually wants to see in their UI (selected),
    // plus anything marked primary even if selected is false (Google
    // sometimes omits `selected` for primary). Drop freeBusyReader-only
    // calendars.
    items.retain(|c| {
        let role_ok = c.access_role.as_deref() != Some("freeBusyReader");
        let visible = c.selected.unwrap_or(false) || c.primary.unwrap_or(false);
        role_ok && visible
    });
    Ok(items)
}

#[derive(Deserialize, Debug)]
struct EventsResponse {
    items: Option<Vec<RawEvent>>,
}

#[derive(Deserialize, Debug)]
struct RawEvent {
    id: String,
    summary: Option<String>,
    description: Option<String>,
    location: Option<String>,
    status: Option<String>,
    #[serde(rename = "htmlLink")]
    html_link: Option<String>,
    #[serde(rename = "hangoutLink")]
    hangout_link: Option<String>,
    start: Option<EventTime>,
    end: Option<EventTime>,
    attendees: Option<Vec<RawAttendee>>,
}

#[derive(Deserialize, Debug)]
struct EventTime {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    date: Option<String>,
}

#[derive(Deserialize, Debug)]
struct RawAttendee {
    email: Option<String>,
    #[serde(default)]
    organizer: Option<bool>,
    #[serde(default, rename = "responseStatus")]
    response_status: Option<String>,
}

async fn list_events_in_window(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<CalendarEvent>, String> {
    let token = oauth::ensure_fresh_token(&google::GOOGLE)
        .await
        .map_err(|e| format!("Google: {}", e))?;
    let calendars = list_user_calendars(&token).await?;
    let mut out: Vec<CalendarEvent> = Vec::new();

    for cal in calendars {
        let raw = match fetch_events_for_calendar(&token, &cal.id, &start, &end).await {
            Ok(items) => items,
            Err(e) => {
                // Single-calendar failure shouldn't take the whole
                // dashboard down. Warn into stderr and skip.
                eprintln!("[calendar] {} events failed: {}", cal.id, e);
                continue;
            }
        };
        for ev in raw {
            if let Some(mapped) = map_event(ev, &cal) {
                out.push(mapped);
            }
        }
    }

    // Sort by start ascending. All-day events sort by their date; timed
    // events by datetime. Lexicographic on the RFC3339/date strings is
    // close enough — both formats sort correctly when prefixed with
    // YYYY-MM-DD.
    out.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(out)
}

async fn fetch_events_for_calendar(
    token: &str,
    calendar_id: &str,
    start: &DateTime<Utc>,
    end: &DateTime<Utc>,
) -> Result<Vec<RawEvent>, String> {
    // Use singleEvents=true so recurring events are expanded into
    // individual instances. orderBy=startTime requires singleEvents.
    let time_min = start.to_rfc3339();
    let time_max = end.to_rfc3339();
    let url = format!(
        "{}/calendars/{}/events?singleEvents=true&orderBy=startTime&showDeleted=false&maxResults=50&timeMin={}&timeMax={}",
        google::CALENDAR_API_BASE,
        urlencode(calendar_id),
        urlencode(&time_min),
        urlencode(&time_max),
    );
    let resp = http_get_json(&url, token).await?;
    let parsed: EventsResponse = serde_json::from_value(resp)
        .map_err(|e| format!("events parse: {}", e))?;
    Ok(parsed.items.unwrap_or_default())
}

fn map_event(raw: RawEvent, cal: &CalendarListEntry) -> Option<CalendarEvent> {
    // Cancelled events come back when showDeleted=true; we ask for
    // false but defend anyway.
    if raw.status.as_deref() == Some("cancelled") {
        return None;
    }
    let start_t = raw.start.as_ref();
    let end_t = raw.end.as_ref();
    let (start_str, end_str, all_day) = match (start_t, end_t) {
        (Some(s), Some(e)) => {
            if let (Some(sd), Some(ed)) = (&s.date_time, &e.date_time) {
                (sd.clone(), ed.clone(), false)
            } else if let (Some(sd), Some(ed)) = (&s.date, &e.date) {
                (sd.clone(), ed.clone(), true)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let attendees: Vec<String> = raw
        .attendees
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !a.organizer.unwrap_or(false))
        .filter(|a| a.response_status.as_deref() != Some("declined"))
        .filter_map(|a| a.email)
        .collect();
    Some(CalendarEvent {
        id: raw.id,
        calendar_id: cal.id.clone(),
        calendar_name: cal.summary.clone(),
        summary: raw.summary.unwrap_or_else(|| "(no title)".to_string()),
        description: raw.description,
        location: raw.location,
        start: start_str,
        end: end_str,
        all_day,
        html_link: raw.html_link,
        hangout_link: raw.hangout_link,
        attendees,
        status: raw.status,
    })
}

// ── HTTP + time helpers ───────────────────────────────────────────────

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
        return Err(format!("Google {}: {}", status.as_u16(), body));
    }
    resp.json().await.map_err(|e| format!("Parse: {}", e))
}

fn local_day_start(now: DateTime<Local>) -> DateTime<Local> {
    Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .unwrap_or(now)
}

fn local_day_end(now: DateTime<Local>) -> DateTime<Local> {
    Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 23, 59, 59)
        .single()
        .unwrap_or(now)
}

// Tiny URL-encoder — matches the one used in lib.rs / oauth.rs. Local
// copy keeps this module self-contained.
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
