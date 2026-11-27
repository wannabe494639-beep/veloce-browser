# Changelog

## 0.1.6 — 2026-11-27

- Publish the current Windows browser build as a stable release.
- Use Windows system fonts for clearer browser controls and slightly larger text in key controls and settings.
- Reduce the blur used by floating panels to keep the UI lighter to composite.
- No measured YouTube startup-time or RAM improvement is claimed in this release.

## 0.1.6-beta.1 — 2026-11-26

- Retry the Android ARM64 beta workflow with the current Android SDK setup action and only supported SDK package names.
- Mark the shared Rust application entry point with Tauri's required mobile entry-point macro so Android's JNI runtime symbols are exported.
- Verify the ARM64 ABI in Tauri's generated APK and collect its actual output path before uploading the test artifact.
- Advance the Android beta package version so future test builds can install as a newer application version.

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
