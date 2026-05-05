// Companion - Granola provider config
//
// Caller-side glue that turns Granola's OAuth + MCP details into a
// `ProviderConfig` for `oauth.rs` to drive.
//
// Auth flow (from https://docs.granola.ai/help-center/sharing/integrations/mcp):
//
//   - Authorisation URL: https://api.granola.ai/oauth/authorize
//   - Token URL:         https://api.granola.ai/oauth/token
//   - Scopes:            "read:meetings read:transcripts"
//   - PKCE:              required (Granola supports DCR but Companion uses
//                        a manually-registered client ID for now)
//   - Redirect URI:      http://localhost:53682/callback (registered
//                        when the user creates the OAuth client in
//                        Granola's developer console)
//
// The OAuth client ID is supplied by the user in Settings — we don't
// ship it baked in. When Caitlin (or anyone else) sets up Companion for
// Granola, they:
//
//   1. Create a Granola OAuth client at the Granola developer console.
//   2. Set the redirect URI to http://localhost:53682/callback.
//   3. Paste the resulting client ID into Settings → "Granola
//      integration" → Client ID.
//   4. Click "Connect Granola" - Companion runs the browser PKCE flow.
//
// MCP server details for the Anthropic Messages API request:
//   - URL: https://mcp.granola.ai/mcp
//   - Tools we allowlist for Monthly Check-in / Quarterly Review:
//     `list_meetings`, `get_meeting_transcript`.

use crate::oauth::ProviderConfig;

pub const GRANOLA_MCP_URL: &str = "https://mcp.granola.ai/mcp";

pub const GRANOLA: ProviderConfig = ProviderConfig {
    name: "granola",
    auth_url: "https://api.granola.ai/oauth/authorize",
    token_url: "https://api.granola.ai/oauth/token",
    scopes: &["read:meetings", "read:transcripts"],
    client_id_keychain_key: "granola-client-id",
    // Granola issues public clients (PKCE only, no secret). If your
    // Granola OAuth client was created as confidential, set the secret
    // in Keychain at "granola-client-secret" and switch this to:
    //   client_secret_keychain_key: Some("granola-client-secret"),
    client_secret_keychain_key: None,
    access_token_key: "granola-access-token",
    refresh_token_key: "granola-refresh-token",
    expires_at_key: "granola-expires-at",
    auth_extra_params: &[],
};

// Convenience: tools to allowlist when running Monthly Check-in /
// Quarterly Review with Granola enabled. Keeps token cost down by
// disabling everything else on the server. Same allowlist for both
// surfaces in v0.22 — kept as a single constant.
pub const MONTHLY_CHECKIN_TOOLS: &[&str] = &["list_meetings", "get_meeting_transcript"];
