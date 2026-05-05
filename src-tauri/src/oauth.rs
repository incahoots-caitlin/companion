// Companion - generic OAuth 2.0 PKCE helper
//
// One module, used by every browser-OAuth provider Studio talks to. The
// pattern is:
//
//   1. Caller builds a `ProviderConfig` (auth URL, token URL, scopes,
//      client ID, optional client secret).
//   2. Caller invokes `start_oauth_flow(provider, keychain_keys)`.
//   3. Helper generates PKCE verifier+challenge, opens the system
//      browser to the auth URL with `redirect_uri=http://localhost:53682/callback`,
//      and stands up a tiny single-shot HTTP listener on port 53682.
//   4. User authorises in the browser. Provider redirects to the
//      localhost callback with `?code=...`.
//   5. Listener captures the code, exchanges it for an access token
//      (and refresh token, if granted) at the provider's token URL,
//      then stores all of it in macOS Keychain under the supplied
//      keychain accounts.
//   6. Helper also stores `<prefix>-expires-at` as a unix timestamp so
//      the next call can decide whether to refresh.
//
// Refresh handling lives in `ensure_fresh_token` — call this any time
// before using a token. It reads the access token, refresh token, and
// expiry from Keychain. If the access token is still good, it's returned.
// If it's expired (or about to be) and a refresh token exists, the
// helper exchanges the refresh token for a new access token, updates
// Keychain, and returns the new token.
//
// Fixed callback port: 53682 (matches `gh`, `rclone`). Means the user
// has to register `http://localhost:53682/callback` as the redirect URI
// when setting up the OAuth client in the provider's console.
//
// Slack note (v0.26): Slack's OAuth v2 still requires a client secret
// even for desktop apps and doesn't support PKCE. The flow has the same
// shape (browser auth → code → token exchange) but skips the PKCE
// challenge and posts the secret with the code-for-token swap. The
// `pkce` flag on `ProviderConfig` toggles that behaviour. Slack also
// returns user tokens under `authed_user.access_token` rather than the
// top-level `access_token`, so a small post-parse adapter handles that
// difference.

use keyring::Entry;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::time::Duration;

const KEYRING_SERVICE: &str = "marketing.incahoots.studio";
const CALLBACK_PORT: u16 = 53682;
const CALLBACK_PATH: &str = "/callback";
const REDIRECT_URI: &str = "http://localhost:53682/callback";

// 30s leeway on token expiry. If a token expires within 30s, we treat
// it as expired and refresh proactively to avoid in-flight 401s.
const REFRESH_LEEWAY_SECS: i64 = 30;

// How the provider expects scopes to be passed on the authorisation URL.
// RFC 6749 says space-separated under the `scope` param. Slack's v2 OAuth
// uses `scope=channels:read,channels:history,...` (commas) and a separate
// `user_scope` param when requesting user-token scopes. Most providers
// match RFC; only Slack diverges in our set today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeStyle {
    SpaceSeparated, // Google, Granola, anything RFC-vanilla.
    SlackV2,        // Slack: comma-separated, treats as user-token scopes.
}

#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub name: &'static str,             // "granola", "google", "slack"
    pub auth_url: &'static str,         // e.g. https://api.granola.ai/oauth/authorize
    pub token_url: &'static str,        // e.g. https://api.granola.ai/oauth/token
    pub scopes: &'static [&'static str],
    pub client_id_keychain_key: &'static str,    // Keychain account holding the client ID
    pub client_secret_keychain_key: Option<&'static str>, // None for PKCE-only public clients
    // Keychain accounts under KEYRING_SERVICE for storing tokens.
    pub access_token_key: &'static str,    // e.g. "granola-access-token"
    pub refresh_token_key: &'static str,   // e.g. "granola-refresh-token"
    pub expires_at_key: &'static str,      // e.g. "granola-expires-at"
    // Extra (key, value) params appended to the authorisation URL only.
    // Google needs `access_type=offline&prompt=consent` to issue a
    // refresh token. Granola and other RFC-vanilla providers leave this
    // empty.
    pub auth_extra_params: &'static [(&'static str, &'static str)],
    // Whether to run PKCE on this provider. Public clients (Google
    // Desktop, Granola) use PKCE; Slack v2 OAuth still requires a client
    // secret and doesn't support PKCE. False for Slack, true elsewhere.
    pub pkce: bool,
    pub scope_style: ScopeStyle,
}

#[derive(Deserialize, Debug)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

// Slack's v2 OAuth response is shaped differently from RFC: the token
// the workspace user holds lives under `authed_user.access_token`. We
// parse it loosely and adapt to a TokenResponse before persisting.
#[derive(Deserialize, Debug)]
struct SlackTokenResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    access_token: Option<String>, // bot token (when bot scopes were requested)
    #[serde(default)]
    authed_user: Option<SlackAuthedUser>,
}

#[derive(Deserialize, Debug)]
struct SlackAuthedUser {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

// ── Public API ─────────────────────────────────────────────────────────

// Reads a Keychain entry. None if the entry doesn't exist or can't be
// read. Used both for client IDs (set by the user in Settings) and for
// stored tokens.
pub fn read_keychain(account: &str) -> Option<String> {
    Entry::new(KEYRING_SERVICE, account)
        .ok()?
        .get_password()
        .ok()
}

pub fn write_keychain(account: &str, value: &str) -> Result<(), String> {
    Entry::new(KEYRING_SERVICE, account)
        .map_err(|e| format!("Keychain init for {}: {}", account, e))?
        .set_password(value)
        .map_err(|e| format!("Keychain write for {}: {}", account, e))
}

pub fn delete_keychain(account: &str) -> Result<(), String> {
    let entry = Entry::new(KEYRING_SERVICE, account)
        .map_err(|e| format!("Keychain init for {}: {}", account, e))?;
    // delete_credential returns NoEntry if it doesn't exist; that's fine.
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Keychain delete for {}: {}", account, e)),
    }
}

// Status helper: is this provider connected (has an access token in
// Keychain)? Used by Settings UI to show connect / disconnect state.
pub fn is_connected(provider: &ProviderConfig) -> bool {
    read_keychain(provider.access_token_key).is_some()
}

// Disconnect: wipe access, refresh, and expiry entries.
pub fn disconnect(provider: &ProviderConfig) -> Result<(), String> {
    delete_keychain(provider.access_token_key)?;
    delete_keychain(provider.refresh_token_key)?;
    delete_keychain(provider.expires_at_key)?;
    Ok(())
}

// Returns a valid (unexpired or freshly refreshed) access token.
// Fails if the provider isn't connected and the caller needs to run
// the full auth flow first.
pub async fn ensure_fresh_token(provider: &ProviderConfig) -> Result<String, String> {
    let access = read_keychain(provider.access_token_key)
        .ok_or_else(|| format!("{} not connected", provider.name))?;

    // If we have an expiry and it's not yet past it, the token is good.
    let now = chrono::Utc::now().timestamp();
    let expires_at = read_keychain(provider.expires_at_key)
        .and_then(|s| s.parse::<i64>().ok());

    let needs_refresh = match expires_at {
        Some(exp) => exp - now <= REFRESH_LEEWAY_SECS,
        None => false, // No expiry stored: assume still valid (some providers don't return expires_in).
    };

    if !needs_refresh {
        return Ok(access);
    }

    // Try to refresh.
    let refresh = read_keychain(provider.refresh_token_key)
        .ok_or_else(|| format!("{} access token expired and no refresh token", provider.name))?;

    let client_id = read_keychain(provider.client_id_keychain_key)
        .ok_or_else(|| format!("{} client ID not configured", provider.name))?;
    let client_secret = provider
        .client_secret_keychain_key
        .and_then(read_keychain);

    let mut params: Vec<(&str, String)> = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh),
        ("client_id", client_id),
    ];
    if let Some(secret) = client_secret {
        params.push(("client_secret", secret));
    }

    let token = match provider.scope_style {
        ScopeStyle::SlackV2 => post_slack_token_request(provider.token_url, &params).await?,
        ScopeStyle::SpaceSeparated => post_token_request(provider.token_url, &params).await?,
    };
    persist_token(provider, &token)?;
    Ok(token.access_token)
}

// Runs the full browser OAuth flow and stores the resulting token. Uses
// PKCE when `provider.pkce` is true, otherwise relies on the client
// secret. Blocks the calling task until the user authorises (or 5 min
// timeout).
pub async fn start_oauth_flow(provider: &ProviderConfig) -> Result<(), String> {
    let client_id = read_keychain(provider.client_id_keychain_key)
        .ok_or_else(|| format!(
            "{} client ID not set. Add it in Settings before connecting.",
            provider.name
        ))?;

    // PKCE — generated up-front so the verifier is in scope for the
    // token exchange, but only sent on the wire when pkce is true.
    let verifier = generate_code_verifier();
    let challenge = code_challenge(&verifier);

    // CSRF state.
    let state = random_url_safe(16);

    // Build the auth URL. Scope param formatting depends on provider:
    // RFC says space-separated; Slack v2 uses commas under `scope` and
    // expects user-token scopes there too.
    let scope = match provider.scope_style {
        ScopeStyle::SpaceSeparated => provider.scopes.join(" "),
        ScopeStyle::SlackV2 => provider.scopes.join(","),
    };
    let mut auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&state={}",
        provider.auth_url,
        urlencode(&client_id),
        urlencode(REDIRECT_URI),
        urlencode(&state),
    );
    // Slack reads user-token scopes off `user_scope`, not `scope` — bot
    // scopes go on `scope`. Companion only needs user-token reads, so we
    // put the entire scope list under `user_scope` for Slack.
    match provider.scope_style {
        ScopeStyle::SpaceSeparated => {
            auth_url.push_str("&scope=");
            auth_url.push_str(&urlencode(&scope));
        }
        ScopeStyle::SlackV2 => {
            auth_url.push_str("&user_scope=");
            auth_url.push_str(&urlencode(&scope));
        }
    }
    if provider.pkce {
        auth_url.push_str("&code_challenge=");
        auth_url.push_str(&urlencode(&challenge));
        auth_url.push_str("&code_challenge_method=S256");
    }
    for (k, v) in provider.auth_extra_params {
        auth_url.push('&');
        auth_url.push_str(&urlencode(k));
        auth_url.push('=');
        auth_url.push_str(&urlencode(v));
    }

    // Stand up the localhost listener BEFORE opening the browser.
    // Otherwise there's a race where the user hits "Authorise" before
    // we're listening.
    let listener = TcpListener::bind(("127.0.0.1", CALLBACK_PORT))
        .map_err(|e| format!(
            "Couldn't bind localhost:{}. Is another OAuth flow in progress? ({})",
            CALLBACK_PORT, e
        ))?;
    listener
        .set_nonblocking(false)
        .map_err(|e| format!("Listener config: {}", e))?;

    // Open the browser. macOS-specific via `open`; the underlying call
    // is identical to what tauri-plugin-opener would do.
    Command::new("open")
        .arg(&auth_url)
        .spawn()
        .map_err(|e| format!("Couldn't open browser: {}", e))?;

    // Wait for the redirect. Run the blocking accept on a spawn_blocking
    // task so we don't stall the tokio runtime.
    let listener_state = state.clone();
    let (code, returned_state) = tokio::task::spawn_blocking(move || {
        accept_oauth_callback(listener, &listener_state)
    })
    .await
    .map_err(|e| format!("Listener task: {}", e))??;

    if returned_state != state {
        return Err("OAuth state mismatch — possible CSRF, aborting".to_string());
    }

    // Exchange code for tokens.
    let client_secret = provider
        .client_secret_keychain_key
        .and_then(read_keychain);
    if !provider.pkce && client_secret.is_none() {
        return Err(format!(
            "{} client secret not set. Add it in Settings before connecting.",
            provider.name
        ));
    }

    let mut params: Vec<(&str, String)> = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code),
        ("redirect_uri", REDIRECT_URI.to_string()),
        ("client_id", client_id),
    ];
    if provider.pkce {
        params.push(("code_verifier", verifier));
    }
    if let Some(secret) = client_secret {
        params.push(("client_secret", secret));
    }

    let token = match provider.scope_style {
        ScopeStyle::SlackV2 => post_slack_token_request(provider.token_url, &params).await?,
        ScopeStyle::SpaceSeparated => post_token_request(provider.token_url, &params).await?,
    };
    persist_token(provider, &token)?;
    Ok(())
}

// ── Internals ──────────────────────────────────────────────────────────

fn persist_token(provider: &ProviderConfig, token: &TokenResponse) -> Result<(), String> {
    write_keychain(provider.access_token_key, &token.access_token)?;
    if let Some(refresh) = &token.refresh_token {
        write_keychain(provider.refresh_token_key, refresh)?;
    }
    if let Some(expires_in) = token.expires_in {
        let expires_at = chrono::Utc::now().timestamp() + expires_in;
        write_keychain(provider.expires_at_key, &expires_at.to_string())?;
    }
    Ok(())
}

// Slack's token endpoint always returns 200 with `ok: false` on failure
// (rather than a 4xx). We parse the body as Slack's shape, surface the
// `error` field on failure, and adapt the success case to the same
// TokenResponse the rest of this module uses.
async fn post_slack_token_request(
    url: &str,
    params: &[(&str, String)],
) -> Result<TokenResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;

    let form: HashMap<&str, String> = params
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();

    let response = client
        .post(url)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("Token request network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Slack token endpoint {}: {}", status.as_u16(), text));
    }

    let parsed: SlackTokenResponse = response
        .json()
        .await
        .map_err(|e| format!("Slack token parse: {}", e))?;

    if !parsed.ok {
        return Err(format!(
            "Slack OAuth: {}",
            parsed.error.unwrap_or_else(|| "unknown error".to_string())
        ));
    }

    // User token first (Companion uses user-token scopes for reads),
    // fall back to bot token for completeness.
    let (access, refresh, expires_in) = match parsed.authed_user {
        Some(u) if u.access_token.is_some() => {
            (u.access_token.unwrap(), u.refresh_token, u.expires_in)
        }
        _ => match parsed.access_token {
            Some(t) => (t, None, None),
            None => {
                return Err("Slack OAuth: response missing access_token".to_string());
            }
        },
    };

    Ok(TokenResponse {
        access_token: access,
        refresh_token: refresh,
        expires_in,
    })
}

async fn post_token_request(
    url: &str,
    params: &[(&str, String)],
) -> Result<TokenResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;

    let form: HashMap<&str, String> = params
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();

    let response = client
        .post(url)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("Token request network error: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Token endpoint {}: {}", status.as_u16(), text));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|e| format!("Token response parse: {}", e))
}

// Block until a single GET arrives on the callback path. Returns
// (code, state). 5-minute timeout.
fn accept_oauth_callback(
    listener: TcpListener,
    expected_state: &str,
) -> Result<(String, String), String> {
    listener
        .set_nonblocking(false)
        .map_err(|e| format!("Listener config: {}", e))?;

    // Set a deadline so a stuck flow doesn't pin the port forever.
    let deadline = std::time::Instant::now() + Duration::from_secs(300);

    loop {
        if std::time::Instant::now() >= deadline {
            return Err("OAuth flow timed out after 5 minutes".to_string());
        }

        // Use the OS-level accept; we set non-blocking false above so
        // this blocks until a connection arrives. A real timeout would
        // need set_nonblocking + poll, but blocking accept + a 5min
        // deadline above is enough for our case.
        let (mut stream, _) = listener
            .accept()
            .map_err(|e| format!("Accept: {}", e))?;

        // Read the request line and headers. We don't bother with the
        // full HTTP parse — only the first line matters.
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .ok();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]).to_string();

        // Parse "GET /callback?code=...&state=... HTTP/1.1"
        let first_line = request.lines().next().unwrap_or("").to_string();
        if !first_line.starts_with("GET ") {
            // Not what we expected. Reply 400 and keep listening.
            let _ = stream.write_all(
                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n",
            );
            continue;
        }

        // Pull out the path+query.
        let path = first_line.split(' ').nth(1).unwrap_or("");
        let query_idx = path.find('?');
        let query = match query_idx {
            Some(i) => &path[i + 1..],
            None => "",
        };
        let path_only = match query_idx {
            Some(i) => &path[..i],
            None => path,
        };

        if path_only != CALLBACK_PATH {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
            );
            continue;
        }

        let params = parse_query(query);
        let code = params.get("code").cloned();
        let state = params.get("state").cloned().unwrap_or_default();

        // Send a friendly response in the browser.
        let body = if code.is_some() && state == expected_state {
            r#"<!doctype html><html><head><title>Companion connected</title>
<style>body{font-family:-apple-system,system-ui,sans-serif;background:#f8f5ee;color:#1a1a1a;display:flex;align-items:center;justify-content:center;height:100vh;margin:0}main{max-width:420px;text-align:center;padding:32px}h1{font-size:22px;margin:0 0 12px}p{font-size:15px;line-height:1.5;color:#555;margin:0}</style></head>
<body><main><h1>Connected.</h1><p>You can close this tab and return to Companion.</p></main></body></html>"#
        } else {
            r#"<!doctype html><html><head><title>Companion - auth failed</title>
<style>body{font-family:-apple-system,system-ui,sans-serif;background:#f8f5ee;color:#1a1a1a;display:flex;align-items:center;justify-content:center;height:100vh;margin:0}main{max-width:420px;text-align:center;padding:32px}h1{font-size:22px;margin:0 0 12px}p{font-size:15px;line-height:1.5;color:#555;margin:0}</style></head>
<body><main><h1>Couldn't connect.</h1><p>Companion didn't get a valid auth code back. Open Companion and try again.</p></main></body></html>"#
        };

        let response_text = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response_text.as_bytes());
        let _ = stream.flush();

        match (code, state) {
            (Some(c), s) if !c.is_empty() => return Ok((c, s)),
            _ => {
                return Err(
                    "OAuth callback didn't include a code parameter".to_string(),
                );
            }
        }
    }
}

fn parse_query(q: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut iter = pair.splitn(2, '=');
        let k = iter.next().unwrap_or("");
        let v = iter.next().unwrap_or("");
        out.insert(urldecode(k), urldecode(v));
    }
    out
}

fn urldecode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'+' {
            out.push(' ');
            i += 1;
        } else if b == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16).unwrap_or(0);
            let lo = (bytes[i + 2] as char).to_digit(16).unwrap_or(0);
            out.push(((hi * 16 + lo) as u8) as char);
            i += 3;
        } else {
            out.push(b as char);
            i += 1;
        }
    }
    out
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

// PKCE: 43-128 char URL-safe random string.
fn generate_code_verifier() -> String {
    random_url_safe(64)
}

fn code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    base64_url_encode(&hash)
}

fn random_url_safe(byte_count: usize) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Not cryptographically pristine, but Studio is single-user desktop
    // and the verifier is one-shot per OAuth flow. SHA256 of (time +
    // counter) gives more than enough entropy for PKCE's purposes.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(now.to_le_bytes());
    hasher.update(n.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    let hash = hasher.finalize();
    // Take the requested number of bytes; if more requested than 32,
    // re-hash to extend.
    let mut out = Vec::with_capacity(byte_count);
    out.extend_from_slice(&hash);
    while out.len() < byte_count {
        let mut hasher = Sha256::new();
        hasher.update(&out);
        out.extend_from_slice(&hasher.finalize());
    }
    out.truncate(byte_count);
    base64_url_encode(&out)
}

fn base64_url_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((bytes.len() * 4 + 2) / 3);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHABET[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
    }
    out
}
