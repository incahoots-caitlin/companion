// Companion - Google provider config (v0.25)
//
// Caller-side glue that turns Google's OAuth details into a
// `ProviderConfig` for `oauth.rs` to drive. v0.24 lit up Calendar
// read-only on the Today dashboard. v0.25 reuses the same OAuth session
// for Gmail (read) and Drive (read), so the scope list below is the
// union of every Google surface Companion talks to. One client, three
// scopes — Caitlin re-authorises once after upgrading from v0.24 and the
// existing Calendar token is replaced with a broader-scope token.
//
// Auth flow:
//   - Authorisation URL: https://accounts.google.com/o/oauth2/v2/auth
//   - Token URL:         https://oauth2.googleapis.com/token
//   - Scopes:            calendar.readonly + gmail.readonly + drive.readonly
//   - PKCE:              required for desktop apps (Google's "installed"
//                        client type — public client, no secret needed)
//   - Redirect URI:      http://localhost:53682/callback (registered
//                        when the user creates the OAuth client in
//                        Google Cloud Console)
//
// The OAuth client ID is supplied by the user in Settings — we don't
// ship it baked in. Setup steps Caitlin runs once:
//
//   1. Open the existing Google Cloud project used by gws-* skills.
//   2. APIs & Services > Library > enable: Gmail API and
//      Google Drive API (Calendar API was already enabled in v0.24).
//   3. APIs & Services > Credentials > Create credentials > OAuth client
//      ID. Application type: "Desktop app".
//   4. Set the redirect URI to http://localhost:53682/callback.
//   5. Paste the resulting client ID into Settings > "Google
//      integration" > Client ID. (v0.24 users keep their existing ID.)
//   6. Click "Re-authorise" - Companion runs the browser PKCE flow with
//      the broader scope list. The v0.24 Calendar-only token gets
//      replaced; no data loss.
//
// Refresh tokens: Google only returns a refresh_token when the auth
// request includes `access_type=offline` and `prompt=consent`. We add
// these via the extra-params hook below so the access token can survive
// past its 1-hour expiry without sending Caitlin back to her browser.

use crate::oauth::{ProviderConfig, ScopeStyle};

pub const CALENDAR_API_BASE: &str = "https://www.googleapis.com/calendar/v3";
pub const GMAIL_API_BASE: &str = "https://gmail.googleapis.com/gmail/v1";
pub const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";

// Public scope strings — surface them as constants so the Settings UI
// can render the connected-scope list without hard-coding URLs.
pub const SCOPE_CALENDAR: &str = "https://www.googleapis.com/auth/calendar.readonly";
pub const SCOPE_GMAIL: &str = "https://www.googleapis.com/auth/gmail.readonly";
pub const SCOPE_DRIVE: &str = "https://www.googleapis.com/auth/drive.readonly";

pub const GOOGLE: ProviderConfig = ProviderConfig {
    name: "google",
    auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    // Calendar + Gmail + Drive read-only. Single OAuth covers all three
    // surfaces. v0.24 users re-authorise once after upgrading and the
    // broader-scope token replaces the Calendar-only one.
    scopes: &[SCOPE_CALENDAR, SCOPE_GMAIL, SCOPE_DRIVE],
    client_id_keychain_key: "google-client-id",
    // Google "Desktop app" clients are public — PKCE only, no secret.
    client_secret_keychain_key: None,
    access_token_key: "google-access-token",
    refresh_token_key: "google-refresh-token",
    expires_at_key: "google-expires-at",
    // Google only returns a refresh_token when access_type=offline and
    // prompt=consent are present on the auth URL. include_granted_scopes
    // makes incremental authorisation work cleanly when the user adds
    // future scopes — Google merges them with what's already granted.
    auth_extra_params: &[
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("include_granted_scopes", "true"),
    ],
    pkce: true,
    scope_style: ScopeStyle::SpaceSeparated,
};
