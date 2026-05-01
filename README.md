# In Cahoots Studio

A mini operating system for In Cahoots. Local Mac app, powered by Claude Agent SDK, reading and writing the Dropbox templates folder, printing to the Munbyn, posting to Slack on tick.

Spec lives at [`IN CAHOOTS/STUDIO/spec.md`](https://www.dropbox.com/home/IN%20CAHOOTS/STUDIO).

## Stack

- **Shell:** Tauri 2.x (Rust + WebView), distributed as ad-hoc-signed DMG for Apple Silicon.
- **Frontend:** Vanilla HTML/CSS/JS. No framework. Mirrors Context's stack.
- **Design system:** Open Props primitives plus Context's `tokens.css` plus a thin bridge layer. Geist Sans and Geist Mono variable fonts, copied from Context.
- **AI runtime:** Claude Agent SDK, embedded (next build).
- **State:** Local SQLite (next build).
- **Distribution:** GitHub Releases plus a Vercel-hosted download page (next build).

## Running locally

```bash
# Frontend preview only (no Tauri shell):
cd src && python3 -m http.server 8766

# Full Tauri dev mode:
npm install
cargo tauri dev
```

## Building the DMG

```bash
cargo tauri build
# DMG lands at src-tauri/target/release/bundle/dmg/
```

## Status

v0 spike. Renders the receipt feed using Context's design tokens. AI workflow runner, SQLite store, MCP server, Slack hooks, Munbyn reprint, and folder-watcher all wire up in subsequent builds per the spec.
