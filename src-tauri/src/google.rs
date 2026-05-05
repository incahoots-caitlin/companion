// Companion - Google provider config (v0.24)
//
// Caller-side glue that turns Google's OAuth details into a
// `ProviderConfig` for `oauth.rs` to drive. v0.24 lights up Calendar
// read-only on the Today dashboard. v0.25 will reuse the same OAuth
// session for Gmail and Drive — the trick is that Google issues one
// access token per scope set, so the scope list here is the union of
// every Google surface Companion uses.
//
// Auth flow:
//   - Authorisation URL: https://accounts.google.com/o/oauth2/v2/auth
//   - Token URL:         https://oauth2.googleapis.com/token
//   - Scopes:            calendar.readonly (more added in later versions)
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
//   2. APIs & Services > Credentials > Create credentials > OAuth client
//      ID. Application type: "Desktop app".
//   3. Set the redirect URI to http://localhost:53682/callback.
//   4. Enable the Google Calendar API on the project.
//   5. Paste the resulting client ID into Settings > "Google
//      integration" > Client ID.
//   6. Click "Connect Google" - Companion runs the browser PKCE flow.
//
// Refresh tokens: Google only returns a refresh_token when the auth
// request includes `access_type=offline` and `prompt=consent`. We add
// these via the extra-params hook below so the access token can survive
// past its 1-hour expiry without sending Caitlin back to her browser.

use crate::oauth::ProviderConfig;

pub const CALENDAR_API_BASE: &str = "https://www.googleapis.com/calendar/v3";

pub const GOOGLE: ProviderConfig = ProviderConfig {
    name: "google",
    auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    // Calendar read-only for v0.24. v0.25 will append gmail.readonly and
    // drive.readonly. Reauthorising adds scopes; the user gets one extra
    // browser round-trip when v0.25 ships.
    scopes: &["https://www.googleapis.com/auth/calendar.readonly"],
    client_id_keychain_key: "google-client-id",
    // Google "Desktop app" clients are public — PKCE only, no secret.
    client_secret_keychain_key: None,
    access_token_key: "google-access-token",
    refresh_token_key: "google-refresh-token",
    expires_at_key: "google-expires-at",
    // Google only returns a refresh_token when access_type=offline and
    // prompt=consent are present on the auth URL. include_granted_scopes
    // makes incremental authorisation work cleanly when v0.25 adds Gmail
    // and Drive scopes — Google merges them with what's already granted.
    auth_extra_params: &[
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("include_granted_scopes", "true"),
    ],
};
