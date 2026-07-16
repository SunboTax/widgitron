# Changelog

## [0.2.5] - 2026-07-15

### Highlights
- **Sidebar sensitivity**: Reveal / hide sensitivity are configurable in Settings (defaults keep today’s feel: cautious edge open, snappy auto-hide).
- **Upgrade-safe settings**: Installed builds always store user configs in AppData; migrate leftover exe-adjacent configs once, and soft-merge JSON on schema bumps so reinstalls stop wiping preferences.
- **Mixed-DPI desktop lock**: Compensates WebView zoom when desktop-locked widgets sit on a lower-scale secondary monitor under the primary-DPI desktop host.
- **Monitor / cache polish**: Thinner sidebar resize handles (no longer block dragging); GPU/Quota cache clears finished Slurm jobs and avoids sticky “showing cached” banners.

## [0.2.4] - 2026-06-20

### Highlights
- **Quota Monitor — more agent providers**: Added or expanded quota fetchers for **Qoder CN** (local IDE cache + OpenAPI), **Pioneer AI** (API key), **Claude Code** (local `~/.claude/settings.json` or MiniMax `sk-cp-` proxy token), and **MiniMax CN** (Bearer API key with optional JSON path). Existing providers remain: **Antigravity** (language server + cloud OAuth fallback), **Codex**, **Cursor**, **VS Code Copilot**, plus **OpenAI-compatible** custom endpoints.
- **Startup performance**: Lighter widget window init, staggered backend monitors, OTA dedupe, and reduced duplicate IPC on launch.
- **Type safety & IPC**: Typed `tauriInvoke` / `tauriListen` / `tauriEmit` helpers and shared event payload types.
- **Quota UX**: Clearer Antigravity setup hints and softer offline/cached-data messages for Copilot and other providers.

## [0.2.3] - 2026-06-09

### Highlights
- **OTA Update System**: Integrated full automatic in-app software update checking, asynchronous installer download with progress tracking, and silent startup.
- **Stability Polish**: Robust network request timeouts and event-listener cleanup optimization to guarantee smooth performance.

## [0.2.2] - 2026-06-03

### Highlights
- **Quota Monitor**: Added support for multi-progress bar monitoring and subscription package display toggle.
- **Paper Deadlines**: Added CORE and CCF conference rank information and filtering.
- **Arxiv Radar**: Fixed intermittent fetch failures by implementing robust URL percent-encoding and quote-wrapping for multi-word search phrases. Added validation of HTTP response status codes.
- **UI/UX Polish**: Beautiful dashboard sidebar, customized setting list styles, and widget color unification.

## [0.2.1] - 2026-05-14

### Highlights
- **Arxiv Radar**: New research curation tool with intuitive swipe gestures (Save/Discard/Open PDF).
- **Theme Engine**: Advanced customization system for widget colors, opacity, and theme assignments.
- **UI/UX Polish**: Enhanced information density with expanded paper summaries and full-title rendering.
- **Stability Fixes**: Resolved window freezing on startup and improved cross-session position memory for all widgets.

## [0.2.0] - 2026-05-10
- **Tauri 2.0 Migration**: Major backend overhaul for improved performance and modern API support.
- **Glassmorphism UI**: Complete visual redesign for a premium desktop experience.
- **SSH Optimization**: Enhanced GPU monitoring efficiency for remote HPC clusters.
