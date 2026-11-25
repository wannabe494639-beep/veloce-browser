# Changelog

## 0.1.4 — 2026-11-25

- Break down YouTube click-to-first-frame timing by player request, metadata, start readiness, and first rendered frame.
- Avoid synchronous session-file writes during YouTube's single-page navigation to a video.
- Keep the shared WebView2 cache, video requests, uBlock Origin Lite, and SponsorBlock behavior unchanged.
- This release does not guarantee a fixed YouTube start time; live playback timing depends on the player, connection, cache, and media server.
