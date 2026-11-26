# Changelog

## 0.1.5-beta.1 — 2026-11-26

- Add the first ARM64 Android port using Android System WebView in a Kotlin tab adapter with the shared Tauri/Rust and React/TypeScript application.
- Add touch-oriented mobile layout, a saved top/bottom browser-bar preference, Android Back handling, system share, file selection, DownloadManager downloads, external HTTP(S) links, browser-role settings, and video custom fullscreen.
- Keep desktop Chrome extensions out of the Android package and clearly mark Android extension installation as unsupported by System WebView.
- Add an Android ARM64 debug-APK workflow artifact. This is a development beta; no stable signing key, Play Store package, website download, emulator/device verification, or performance claim is included yet.

## 0.1.4 — 2026-11-25

- Break down YouTube click-to-first-frame timing by player request, metadata, start readiness, and first rendered frame.
- Avoid synchronous session-file writes during YouTube's single-page navigation to a video.
- Keep the shared WebView2 cache, video requests, uBlock Origin Lite, and SponsorBlock behavior unchanged.
- This release does not guarantee a fixed YouTube start time; live playback timing depends on the player, connection, cache, and media server.
