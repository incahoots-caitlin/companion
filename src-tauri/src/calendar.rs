// Companion - Google Calendar API client (v0.24, extended in v0.43)
//
// Direct Google Calendar v3 REST. Calls run from the Rust backend so the
// Anthropic Messages API token spend stays zero on the dashboard slot —
// the brief reserves Option B (MCP via Messages API) for v0.25+ workflows
// that need calendar reasoning.
//
// Fetches the authenticated user's primary calendar plus any other
// calendars they own/subscribe to via singleEvents=true expansion. Time
// windows: yesterday (for transcript backfill), today, the next 7 days,
// and a per-client horizon for the per-client view.
//
// Per-client lookup (`list_events_for_client`) is a heuristic: an event
// belongs to a client if its summary, description or attendee list
// contains the client name (case-insensitive substring). The Clients
// table's `gmail_thread_filter` field is consulted as an extra alias
// hint when set — Caitlin uses this for clients whose meetings are
// usually with people whose addresses don't match the client name.
//
// v0.43 adds an attendee → client matcher (`match_event_to_client`) that
// works off email addresses against the Clients table's
// primary_contact_email + gmail_thread_filter aliases. Today's render
// layer uses this to tag each event with a client_code and a deterministic
// "no match" affordance, instead of name-substring guessing.

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

// v0.43: yesterday's events for the "Yesterday's calls" section in
// Today, used to surface receipt-draft offers when a Granola transcript
// shows up. Window is the local previous day, full day boundaries.
pub async fn list_events_yesterday() -> Result<Vec<CalendarEvent>, String> {
    let now = Local::now();
    let yesterday = now - Duration::days(1);
    let start = local_day_start(yesterday).with_timezone(&Utc);
    let end = local_day_end(yesterday).with_timezone(&Utc);
    list_events_in_window(start, end).await
}

// v0.43: per-client recent + upcoming horizon. Combines yesterday +
// today + the next 7 days into a single sorted list, filtered by the
// client matcher. Used by the per-client view's "Recent and upcoming"
// row.
pub async fn list_events_recent_and_upcoming_for_client(
    client_name: &str,
    extra_aliases: &[String],
    primary_contact_email: Option<&str>,
    match_emails: &[String],
) -> Result<Vec<CalendarEvent>, String> {
    let now = Local::now();
    let start = (local_day_start(now) - Duration::days(1)).with_timezone(&Utc);
    let end = (local_day_end(now) + Duration::days(7)).with_timezone(&Utc);
    let all = list_events_in_window(start, end).await?;
    let needles = build_needles(client_name, extra_aliases);
    let attendee_emails = build_email_needles(primary_contact_email, match_emails);
    let filtered: Vec<CalendarEvent> = all
        .into_iter()
        .filter(|ev| {
            event_matches(ev, &needles)
                || event_matches_any_attendee(ev, &attendee_emails)
        })
        .collect();
    Ok(filtered)
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

// v0.43: attendee → client match. Returns true when any of the event's
// attendee emails matches one of the supplied "client emails" (the
// client's primary contact email plus any extras in `match_emails`).
// Comparison is case-insensitive on both the local part and the domain.
//
// Two match modes layered together:
//   - Exact email match: the most reliable signal, used first.
//   - Domain match: useful when a client uses several individual
//     addresses on the same domain (Untitled, NCT, etc.). We only fall
//     back to a domain match when the domain on the supplied email is
//     not a generic provider (gmail.com, outlook.com, hotmail.com,
//     icloud.com, yahoo.com, me.com) — those generics would over-match.
fn build_email_needles(
    primary_contact_email: Option<&str>,
    match_emails: &[String],
) -> Vec<String> {
    let mut emails: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let trimmed = s.trim().to_lowercase();
        if trimmed.contains('@') {
            emails.push(trimmed);
        }
    };
    if let Some(e) = primary_contact_email {
        push(e);
    }
    for raw in match_emails {
        // Aliases come through as a flat string-or-array; we accept
        // comma/whitespace-separated lists too.
        for piece in raw.split(|c: char| c == ',' || c == ';' || c == '\n' || c.is_whitespace()) {
            push(piece);
        }
    }
    emails.sort();
    emails.dedup();
    emails
}

const GENERIC_EMAIL_DOMAINS: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "outlook.com",
    "hotmail.com",
    "live.com",
    "icloud.com",
    "me.com",
    "mac.com",
    "yahoo.com",
    "yahoo.com.au",
    "proton.me",
    "protonmail.com",
];

fn split_email(addr: &str) -> Option<(String, String)> {
    let lower = addr.trim().to_lowercase();
    let mut parts = lower.splitn(2, '@');
    let local = parts.next()?.to_string();
    let domain = parts.next()?.to_string();
    if local.is_empty() || domain.is_empty() {
        return None;
    }
    Some((local, domain))
}

fn event_matches_any_attendee(ev: &CalendarEvent, client_emails: &[String]) -> bool {
    if client_emails.is_empty() {
        return false;
    }
    let attendees: Vec<(String, String)> = ev
        .attendees
        .iter()
        .filter_map(|a| split_email(a))
        .collect();
    if attendees.is_empty() {
        return false;
    }
    // Exact match wins.
    for (al, ad) in &attendees {
        let exact = format!("{}@{}", al, ad);
        if client_emails.iter().any(|c| c == &exact) {
            return true;
        }
    }
    // Domain match, but only for non-generic domains.
    for (_al, ad) in &attendees {
        if GENERIC_EMAIL_DOMAINS.contains(&ad.as_str()) {
            continue;
        }
        for ce in client_emails {
            if let Some((_, cd)) = split_email(ce) {
                if cd == *ad {
                    return true;
                }
            }
        }
    }
    false
}

// Public surface: given an event and a slice of (client_code,
// client_name, primary_contact_email, match_emails) tuples, return the
// best-matching client_code or None. v0.43 calling code passes the full
// active Clients list once per dashboard refresh and applies this to
// every event.
//
// Multi-match heuristic: if two clients both match (e.g. cross-client
// meeting), pick the one whose primary_contact_email matched first.
// Falls back to None when no client signal is found.
pub fn match_event_to_client(
    event: &CalendarEvent,
    clients: &[ClientMatchRecord],
) -> Option<String> {
    let attendees: Vec<(String, String)> = event
        .attendees
        .iter()
        .filter_map(|a| split_email(a))
        .collect();
    if attendees.is_empty() {
        // Fall back to name-substring matching against the event title.
        for c in clients {
            let needles = build_needles(&c.name, &c.aliases);
            if !needles.is_empty() && event_matches(event, &needles) {
                return Some(c.code.clone());
            }
        }
        return None;
    }
    // First pass: exact email match on primary_contact_email or
    // match_emails.
    for c in clients {
        let emails = build_email_needles(c.primary_contact_email.as_deref(), &c.match_emails);
        for (al, ad) in &attendees {
            let exact = format!("{}@{}", al, ad);
            if emails.iter().any(|e| e == &exact) {
                return Some(c.code.clone());
            }
        }
    }
    // Second pass: domain match on a non-generic domain.
    for c in clients {
        let emails = build_email_needles(c.primary_contact_email.as_deref(), &c.match_emails);
        for (_al, ad) in &attendees {
            if GENERIC_EMAIL_DOMAINS.contains(&ad.as_str()) {
                continue;
            }
            for ce in &emails {
                if let Some((_, cd)) = split_email(ce) {
                    if cd == *ad {
                        return Some(c.code.clone());
                    }
                }
            }
        }
    }
    // Third pass: name-substring fallback.
    for c in clients {
        let needles = build_needles(&c.name, &c.aliases);
        if !needles.is_empty() && event_matches(event, &needles) {
            return Some(c.code.clone());
        }
    }
    None
}

// Slim record passed to `match_event_to_client`. Each field is optional
// so the caller can populate from a partial Airtable response without
// failing.
#[derive(Clone, Debug)]
pub struct ClientMatchRecord {
    pub code: String,
    pub name: String,
    pub primary_contact_email: Option<String>,
    pub match_emails: Vec<String>,
    pub aliases: Vec<String>,
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

// v0.44 — exposed publicly so the time-per-client aggregator can request
// a true 7-day window in a single backend call rather than chaining
// list_events_yesterday + list_events_today (which only covered 2 days
// in the v0.43 partial). Other callers should still prefer the named
// today/yesterday/week helpers above.
#[allow(dead_code)]
pub async fn list_events_in_window_pub(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<CalendarEvent>, String> {
    list_events_in_window(start, end).await
}

// Convenience wrapper that takes a number of days back from now (local
// timezone) and returns the events from then to now. Used by the
// 7-day time aggregation in lib.rs.
pub async fn list_events_last_n_days(days_back: i64) -> Result<Vec<CalendarEvent>, String> {
    let now = Local::now();
    let start = (local_day_start(now) - Duration::days(days_back.max(1))).with_timezone(&Utc);
    let end = local_day_end(now).with_timezone(&Utc);
    list_events_in_window(start, end).await
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
