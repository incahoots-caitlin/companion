// Companion - Slack read API client (v0.26)
//
// v0.26 lights up Slack on the Today dashboard and per-client view via
// OAuth. The existing webhook write path in lib.rs (`post_to_slack`) is
// untouched: that's still what fires when a tick has on_done="slack:#x".
// What's new here is the read side — list channels, count unreads, pull
// recent messages, find the #client-{slug} channel for a given client.
//
// Why a fresh module rather than extending the webhook code? The two
// paths talk to entirely different Slack surfaces. The webhook needs
// nothing but the URL Caitlin pasted; reads need a workspace OAuth token,
// channel IDs, and user-name resolution. Mixing them would have meant
// half-paths everywhere.
//
// Auth flow notes (Slack diverges from Granola/Google):
//   - Authorisation URL: https://slack.com/oauth/v2/authorize
//   - Token URL:         https://slack.com/api/oauth.v2.access
//   - Scopes (user):     channels:read, channels:history, groups:read,
//                        groups:history, users:read
//   - PKCE:              not supported. Caitlin pastes a client secret
//                        into Settings; oauth.rs reads it from Keychain
//                        and posts it on the code-for-token swap.
//   - Redirect URI:      http://localhost:53682/callback (registered in
//                        the Slack app's "Redirect URLs" list)
//
// Setup (Caitlin runs once):
//   1. Open https://api.slack.com/apps → "Create New App" → from scratch.
//      App name: "Companion (Caitlin)". Workspace: In Cahoots HQ.
//   2. OAuth & Permissions → Redirect URLs → add
//      http://localhost:53682/callback. Save.
//   3. OAuth & Permissions → User Token Scopes → add the five scopes
//      above. (No bot scopes needed; Companion reads as the user.)
//   4. Basic Information → App Credentials → copy Client ID and Client
//      Secret. Paste both into Companion → Settings → "Slack
//      integration".
//   5. Click "Connect Slack". Browser opens, Caitlin authorises, control
//      returns to Companion.
//
// API surface from this module (all return owned types so the Tauri
// commands in lib.rs don't have to think about lifetimes):
//   - list_channels()                 -> Vec<Channel>
//   - count_unreads_per_channel(...)  -> Vec<UnreadSummary>
//   - list_recent_messages(id, hrs)   -> Vec<Message>
//   - find_client_channel(slug)       -> Option<Channel>
//   - list_channel_activity_for_client(slug) -> Option<ClientChannelActivity>

use crate::oauth::{self, ProviderConfig, ScopeStyle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

pub const SLACK_API_BASE: &str = "https://slack.com/api";

// User-token scopes Companion asks for. Five reads — no writes; the
// webhook path in lib.rs handles outbound posts and doesn't share this
// OAuth session. `search:read` is intentionally omitted — it requires
// a Slack admin scope review and the Today dashboard works fine without
// cross-channel search.
pub const SCOPE_CHANNELS_READ: &str = "channels:read";
pub const SCOPE_CHANNELS_HISTORY: &str = "channels:history";
pub const SCOPE_GROUPS_READ: &str = "groups:read";
pub const SCOPE_GROUPS_HISTORY: &str = "groups:history";
pub const SCOPE_USERS_READ: &str = "users:read";

pub const SLACK: ProviderConfig = ProviderConfig {
    name: "slack",
    auth_url: "https://slack.com/oauth/v2/authorize",
    token_url: "https://slack.com/api/oauth.v2.access",
    scopes: &[
        SCOPE_CHANNELS_READ,
        SCOPE_CHANNELS_HISTORY,
        SCOPE_GROUPS_READ,
        SCOPE_GROUPS_HISTORY,
        SCOPE_USERS_READ,
    ],
    client_id_keychain_key: "slack-oauth-client-id",
    // Slack v2 OAuth requires a confidential client — secret is
    // mandatory. Caitlin pastes it into Settings; oauth.rs reads it
    // from Keychain at exchange time.
    client_secret_keychain_key: Some("slack-oauth-client-secret"),
    access_token_key: "slack-oauth-access-token",
    refresh_token_key: "slack-oauth-refresh-token",
    expires_at_key: "slack-oauth-expires-at",
    auth_extra_params: &[],
    pkce: false,
    scope_style: ScopeStyle::SlackV2,
};

// Tiny channel cap so the unread-summary loop doesn't fan out across
// hundreds of channels. Caitlin's workspace has ~25 active channels;
// 50 covers all of them with headroom.
const CHANNEL_FETCH_CAP: u32 = 200;
const ACTIVITY_MESSAGE_CAP: usize = 5;
const UNREADS_TOP_N: usize = 8;

// ── Public types ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub is_private: bool,
    pub is_member: bool,
    // Channel-app deeplink. Built once at fetch time so the frontend can
    // open Slack desktop directly (Slack expects `slack://channel?...`).
    pub deeplink: String,
    // Web fallback for when the desktop app isn't running.
    pub web_link: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub ts: String,           // Slack timestamp string ("1714898123.001234")
    pub user: Option<String>, // user ID if any
    pub user_name: Option<String>, // resolved display name; None if unresolved
    pub text: String,         // raw text (may contain Slack mrkdwn)
    pub channel_id: String,
    pub channel_name: String,
    pub permalink: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct UnreadSummary {
    pub channel: Channel,
    pub unread_count: u32,
    // Latest message in the channel. None when nothing's been posted in
    // the conversations.history window — keeps the dashboard UI honest
    // ("no messages" rather than "0 unread").
    pub last_message: Option<Message>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ClientChannelActivity {
    pub channel: Channel,
    pub messages: Vec<Message>,
}

// ── Slack API response shapes ─────────────────────────────────────────
//
// Slack always wraps responses in `{ ok: bool, ... }`. We parse only the
// fields we need and error out on `ok: false` with the `error` string
// the server returns.

#[derive(Deserialize, Debug)]
struct SlackEnvelope<T> {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(flatten)]
    body: T,
}

#[derive(Deserialize, Debug)]
struct ConversationsListResponse {
    #[serde(default)]
    channels: Vec<RawChannel>,
    #[serde(default)]
    response_metadata: Option<ResponseMetadata>,
}

#[derive(Deserialize, Debug)]
struct ResponseMetadata {
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Deserialize, Debug)]
struct RawChannel {
    id: String,
    name: String,
    #[serde(default)]
    is_private: bool,
    #[serde(default)]
    is_member: bool,
    #[serde(default)]
    is_archived: bool,
}

#[derive(Deserialize, Debug)]
struct ConversationsHistoryResponse {
    #[serde(default)]
    messages: Vec<RawMessage>,
}

#[derive(Deserialize, Debug)]
struct RawMessage {
    #[serde(default)]
    ts: String,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    subtype: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ConversationsInfoResponse {
    channel: RawChannelInfo,
}

#[derive(Deserialize, Debug)]
struct RawChannelInfo {
    #[serde(default)]
    unread_count: Option<u32>,
}

#[derive(Deserialize, Debug)]
struct UsersInfoResponse {
    user: RawUser,
}

#[derive(Deserialize, Debug)]
struct RawUser {
    #[serde(default)]
    profile: Option<RawUserProfile>,
    #[serde(default)]
    real_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize, Debug)]
struct RawUserProfile {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    real_name: Option<String>,
}

#[derive(Deserialize, Debug)]
struct AuthTeamResponse {
    #[serde(default)]
    team_id: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────

// List public + private channels Caitlin's a member of. Sorted by name
// ascending so the dashboard order is stable.
pub async fn list_channels() -> Result<Vec<Channel>, String> {
    let token = oauth::ensure_fresh_token(&SLACK)
        .await
        .map_err(|e| format!("Slack: {}", e))?;
    let team_id = workspace_team_id(&token).await.unwrap_or_default();

    let mut out: Vec<Channel> = Vec::new();
    let mut cursor: Option<String> = None;
    // Page through up to a few hundred channels — Caitlin's workspace
    // is small but a fresh-install workspace could have more.
    loop {
        let mut url = format!(
            "{}/conversations.list?types=public_channel,private_channel&exclude_archived=true&limit={}",
            SLACK_API_BASE, CHANNEL_FETCH_CAP
        );
        if let Some(c) = &cursor {
            url.push_str("&cursor=");
            url.push_str(&urlencode(c));
        }
        let value = http_get_json(&url, &token).await?;
        let env: SlackEnvelope<ConversationsListResponse> =
            parse_envelope(value, "conversations.list")?;
        let body = env.body;
        for raw in body.channels.into_iter() {
            if raw.is_archived || !raw.is_member {
                continue;
            }
            out.push(Channel {
                id: raw.id.clone(),
                name: raw.name,
                is_private: raw.is_private,
                is_member: raw.is_member,
                deeplink: build_deeplink(&team_id, &raw.id),
                web_link: build_web_link(&team_id, &raw.id),
            });
        }
        cursor = body
            .response_metadata
            .and_then(|m| m.next_cursor)
            .filter(|s| !s.is_empty());
        if cursor.is_none() {
            break;
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

// Returns up to UNREADS_TOP_N channels sorted by unread count desc.
// Skips channels with zero unread so the Today section stays scannable.
pub async fn count_unreads_per_channel() -> Result<Vec<UnreadSummary>, String> {
    let channels = list_channels().await?;
    let token = oauth::ensure_fresh_token(&SLACK)
        .await
        .map_err(|e| format!("Slack: {}", e))?;
    let mut user_cache: HashMap<String, String> = HashMap::new();

    let mut out: Vec<UnreadSummary> = Vec::with_capacity(channels.len());
    for ch in channels.into_iter() {
        let info_url = format!(
            "{}/conversations.info?channel={}&include_num_members=false",
            SLACK_API_BASE,
            urlencode(&ch.id),
        );
        let info_val = match http_get_json(&info_url, &token).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[slack] info {} failed: {}", ch.id, e);
                continue;
            }
        };
        let info_env: SlackEnvelope<ConversationsInfoResponse> =
            match parse_envelope(info_val, "conversations.info") {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("[slack] info {} parse: {}", ch.id, e);
                    continue;
                }
            };
        let unread = info_env.body.channel.unread_count.unwrap_or(0);
        if unread == 0 {
            continue;
        }
        // Pull the most recent message for context. One-shot, cap=1.
        let last_message = match list_messages(&token, &ch, 1, &mut user_cache).await {
            Ok(mut msgs) => msgs.pop(),
            Err(_) => None,
        };
        out.push(UnreadSummary {
            channel: ch,
            unread_count: unread,
            last_message,
        });
    }
    out.sort_by(|a, b| b.unread_count.cmp(&a.unread_count));
    out.truncate(UNREADS_TOP_N);
    Ok(out)
}

// Recent messages in one channel over a sliding-hour window. Used by
// the per-client view (24h on a #client-{slug}) and any future detail
// drill-down.
pub async fn list_recent_messages(
    channel_id: &str,
    since_hours: u32,
) -> Result<Vec<Message>, String> {
    let token = oauth::ensure_fresh_token(&SLACK)
        .await
        .map_err(|e| format!("Slack: {}", e))?;
    let team_id = workspace_team_id(&token).await.unwrap_or_default();

    // Resolve the channel record so we can attach its name + web link
    // without needing a second list call from the caller.
    let info_url = format!(
        "{}/conversations.info?channel={}",
        SLACK_API_BASE,
        urlencode(channel_id),
    );
    let info_val = http_get_json(&info_url, &token).await?;
    // conversations.info also returns the full channel record — reach
    // for it via a separate parse since the existing struct only pulled
    // unread fields.
    let name = info_val["channel"]["name"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let is_private = info_val["channel"]["is_private"].as_bool().unwrap_or(false);
    let is_member = info_val["channel"]["is_member"].as_bool().unwrap_or(true);
    let channel = Channel {
        id: channel_id.to_string(),
        name,
        is_private,
        is_member,
        deeplink: build_deeplink(&team_id, channel_id),
        web_link: build_web_link(&team_id, channel_id),
    };

    let mut user_cache: HashMap<String, String> = HashMap::new();
    let mut messages = list_messages_since_hours(
        &token,
        &channel,
        since_hours,
        ACTIVITY_MESSAGE_CAP * 2,
        &mut user_cache,
    )
    .await?;
    messages.truncate(ACTIVITY_MESSAGE_CAP);
    Ok(messages)
}

// Slug-match against the channel list. Uses prefix `client-` followed by
// the slug. Falls back to a more relaxed contains() match when no exact
// hit (so "client-northcote-theatre" and "client-northcote" both work
// for slug "northcote").
pub async fn find_client_channel(slug: &str) -> Result<Option<Channel>, String> {
    let normalised = slug.trim().to_lowercase().replace('_', "-");
    if normalised.is_empty() {
        return Ok(None);
    }
    let channels = list_channels().await?;
    let exact = format!("client-{}", normalised);
    if let Some(c) = channels.iter().find(|c| c.name == exact) {
        return Ok(Some(c.clone()));
    }
    // Looser match — useful when the slug is "northcote" but the
    // channel is "client-northcote-theatre".
    let prefix = format!("client-{}", normalised);
    if let Some(c) = channels.iter().find(|c| c.name.starts_with(&prefix)) {
        return Ok(Some(c.clone()));
    }
    Ok(None)
}

// Resolves the #client-{slug} channel and pulls the last 24h of
// activity. Returns None when no channel matches — the per-client view
// hides the section in that case.
pub async fn list_channel_activity_for_client(
    slug: &str,
) -> Result<Option<ClientChannelActivity>, String> {
    let channel = match find_client_channel(slug).await? {
        Some(c) => c,
        None => return Ok(None),
    };
    let messages = list_recent_messages(&channel.id, 24).await?;
    Ok(Some(ClientChannelActivity { channel, messages }))
}

// ── Internals ─────────────────────────────────────────────────────────

async fn list_messages(
    token: &str,
    channel: &Channel,
    cap: usize,
    user_cache: &mut HashMap<String, String>,
) -> Result<Vec<Message>, String> {
    let url = format!(
        "{}/conversations.history?channel={}&limit={}",
        SLACK_API_BASE,
        urlencode(&channel.id),
        cap,
    );
    let value = http_get_json(&url, token).await?;
    let env: SlackEnvelope<ConversationsHistoryResponse> =
        parse_envelope(value, "conversations.history")?;
    let mut out: Vec<Message> = Vec::with_capacity(env.body.messages.len());
    for raw in env.body.messages.into_iter() {
        if raw.subtype.as_deref() == Some("channel_join")
            || raw.subtype.as_deref() == Some("channel_leave")
        {
            continue;
        }
        let user_name = match &raw.user {
            Some(uid) => Some(resolve_user_name(token, uid, user_cache).await),
            None => None,
        };
        out.push(Message {
            permalink: build_permalink(&channel.web_link, &raw.ts),
            channel_id: channel.id.clone(),
            channel_name: channel.name.clone(),
            ts: raw.ts,
            user: raw.user,
            user_name,
            text: raw.text,
        });
    }
    Ok(out)
}

async fn list_messages_since_hours(
    token: &str,
    channel: &Channel,
    since_hours: u32,
    cap: usize,
    user_cache: &mut HashMap<String, String>,
) -> Result<Vec<Message>, String> {
    let oldest_secs = (chrono::Utc::now()
        - chrono::Duration::hours(since_hours.max(1) as i64))
    .timestamp();
    let url = format!(
        "{}/conversations.history?channel={}&limit={}&oldest={}",
        SLACK_API_BASE,
        urlencode(&channel.id),
        cap,
        oldest_secs,
    );
    let value = http_get_json(&url, token).await?;
    let env: SlackEnvelope<ConversationsHistoryResponse> =
        parse_envelope(value, "conversations.history")?;
    let mut out: Vec<Message> = Vec::with_capacity(env.body.messages.len());
    for raw in env.body.messages.into_iter() {
        if raw.subtype.as_deref() == Some("channel_join")
            || raw.subtype.as_deref() == Some("channel_leave")
        {
            continue;
        }
        let user_name = match &raw.user {
            Some(uid) => Some(resolve_user_name(token, uid, user_cache).await),
            None => None,
        };
        out.push(Message {
            permalink: build_permalink(&channel.web_link, &raw.ts),
            channel_id: channel.id.clone(),
            channel_name: channel.name.clone(),
            ts: raw.ts,
            user: raw.user,
            user_name,
            text: raw.text,
        });
    }
    Ok(out)
}

async fn resolve_user_name(
    token: &str,
    user_id: &str,
    cache: &mut HashMap<String, String>,
) -> String {
    if let Some(cached) = cache.get(user_id) {
        return cached.clone();
    }
    let url = format!(
        "{}/users.info?user={}",
        SLACK_API_BASE,
        urlencode(user_id),
    );
    let val = match http_get_json(&url, token).await {
        Ok(v) => v,
        Err(_) => {
            cache.insert(user_id.to_string(), user_id.to_string());
            return user_id.to_string();
        }
    };
    let env: SlackEnvelope<UsersInfoResponse> = match parse_envelope(val, "users.info") {
        Ok(e) => e,
        Err(_) => {
            cache.insert(user_id.to_string(), user_id.to_string());
            return user_id.to_string();
        }
    };
    let user = env.body.user;
    let display = user
        .profile
        .as_ref()
        .and_then(|p| p.display_name.as_ref())
        .filter(|s| !s.is_empty())
        .or_else(|| user.profile.as_ref().and_then(|p| p.real_name.as_ref()))
        .or(user.real_name.as_ref())
        .or(user.name.as_ref())
        .cloned()
        .unwrap_or_else(|| user_id.to_string());
    cache.insert(user_id.to_string(), display.clone());
    display
}

async fn workspace_team_id(token: &str) -> Result<String, String> {
    // auth.test returns the team_id for the user the token belongs to.
    // Used for building deeplinks. Cached per-process via a static
    // would be nicer but it's a one-shot per dashboard render in
    // practice and the call is cheap.
    let url = format!("{}/auth.test", SLACK_API_BASE);
    let val = http_get_json(&url, token).await?;
    let env: SlackEnvelope<AuthTeamResponse> = parse_envelope(val, "auth.test")?;
    Ok(env.body.team_id.unwrap_or_default())
}

fn build_deeplink(team_id: &str, channel_id: &str) -> String {
    if team_id.is_empty() {
        format!("slack://channel?id={}", channel_id)
    } else {
        format!("slack://channel?team={}&id={}", team_id, channel_id)
    }
}

fn build_web_link(team_id: &str, channel_id: &str) -> String {
    if team_id.is_empty() {
        format!("https://app.slack.com/client/_/{}", channel_id)
    } else {
        format!("https://app.slack.com/client/{}/{}", team_id, channel_id)
    }
}

fn build_permalink(channel_web_link: &str, ts: &str) -> String {
    // ts looks like "1714898123.001234"; Slack's permalink uses
    // p1714898123001234 (no dot). Best-effort — when the ts is malformed
    // we just point at the channel.
    let cleaned: String = ts.chars().filter(|c| c.is_ascii_digit()).collect();
    if cleaned.is_empty() {
        return channel_web_link.to_string();
    }
    format!("{}/p{}", channel_web_link, cleaned)
}

fn parse_envelope<T: for<'de> Deserialize<'de>>(
    value: serde_json::Value,
    label: &str,
) -> Result<SlackEnvelope<T>, String> {
    let env: SlackEnvelope<T> = serde_json::from_value(value)
        .map_err(|e| format!("{} parse: {}", label, e))?;
    if !env.ok {
        return Err(format!(
            "{}: {}",
            label,
            env.error.clone().unwrap_or_else(|| "unknown".to_string())
        ));
    }
    Ok(env)
}

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
        return Err(format!("Slack {}: {}", status.as_u16(), body));
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
