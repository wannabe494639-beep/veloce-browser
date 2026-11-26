# Veloce Android port: inventory and implementation plan

Status: Android port implemented on `feature/android-port` as development beta `0.1.6-beta.1`. The frontend, shared Rust layer, ARM64 Rust target and Android ARM64 debug APK build successfully in GitHub CI. CI exposed three integration points: the removed legacy Android SDK `tools` package, the required Tauri mobile entry-point macro, and Tauri's generated APK output path (`universal/debug` rather than the assumed `arm64/debug`). The workflow now discovers the APK and verifies its ABI before artifact upload. No device/emulator runtime verification or stable release signing has been done, so this is not a public release download.

## Existing implementation

- The app is a Tauri 2.12.5 application with a React 19 / TypeScript UI and a Rust coordinator (`src-tauri/src/lib.rs`). Shared logic includes URL/search normalization, tab metadata, settings, bookmarks, download metadata, and JSON session persistence.
- The browser chrome is the Tauri `main` webview. Website tabs are separate Tauri child webviews (`Window::add_child`) positioned below it. The Windows implementation additionally injects WebView2 COM behavior, shares a WebView2 environment, handles extensions, downloads, video fullscreen, and process memory reporting.
- Several injected page scripts depend on `window.chrome.webview`; extension management and most detailed memory controls use WebView2-only APIs. The current cross-platform non-Windows branch is not a mobile browser implementation.
- The existing tracker check is for top-level navigation. It does not currently filter normal page subresources. Chrome Web Store extensions are not supported by the Linux/macOS system-webview design either.

## Engine decision

Use **Android System WebView** for the first Android port. Tauri already targets this engine on Android. It is Chromium-based, is normally updated as an Android system component, and is not bundled into every Veloce APK. That best fits the small package and low-idle-resource goals.

GeckoView is the alternative if browser-managed WebExtensions become the priority. It is designed for embedding browser content and exposes a WebExtension API, but it bundles a separate Gecko engine and makes Veloce responsible for its package size, lifecycle, and release updates. It does not make Chrome Web Store extensions compatible. On Android System WebView, Chrome extensions including uBlock Origin Lite cannot be installed as extensions; the mobile product must either expose an honest unsupported state or implement separately audited native filtering and SponsorBlock integration.

Tauri's Android WebView is the application shell. The current design's additional child WebViews are not a supported nested-WebView composition on Android. The Android port therefore needs an Android-native page-view adapter (Kotlin) hosted in the Tauri Activity, with explicit touch routing and layout bounds so the React browser chrome and overlays remain interactive. CSS alone cannot solve native view stacking or hit testing.

References: [Tauri webview versions](https://v2.tauri.app/reference/webview-versions/), [Tauri Android prerequisites](https://v2.tauri.app/start/prerequisites/), [Tauri mobile plugin development](https://v2.tauri.app/develop/plugins/develop-mobile/), [Android WebView embedding](https://developer.android.com/develop/ui/views/layout/webapps/embed-web-content-in-app), [GeckoView overview](https://mozilla.github.io/geckoview/), [GeckoView WebExtensions](https://mozilla.github.io/geckoview/consumer/docs/web-extensions).

## Implementation stages

1. **Mobile build foundation:** initialize and commit the Tauri Android project, set Android package metadata and ARM64 target, and add a GitHub Actions Android build. Keep the existing Windows/Linux build and release jobs unchanged.
2. **Native page adapter:** add a Kotlin Tauri mobile plugin that owns Android `WebView` instances and lifecycle. Keep Rust as the source of truth for tab IDs, URLs, settings, and session metadata; route create/activate/navigate/back/forward/reload/close/layout through a typed adapter. Use the Android system WebView for page tabs and avoid a second bundled engine.
3. **Mobile composition and controls:** lay out the native page view under the touch browser chrome; route page touches to page content and toolbar/overlay touches to React. Add phone/tablet breakpoints, bottom address-bar preference, keyboard/window-inset handling, orientation, fullscreen video, Android back, share, uploads, permissions, downloads, external links, and default-browser declarations.
4. **Platform parity and honest capability reporting:** reuse shared Rust navigation/session/settings code. Adapt playback checkpoints and native download events to a documented bridge instead of assuming `window.chrome.webview`. Keep unsupported Chromium extensions unavailable on Android; implement only independently auditable native filters/features and label their real status.
5. **Security and data:** validate HTTP(S) navigation, keep TLS verification and Safe Browsing, restrict privileged IPC to the shell, request permissions only when used, and use Android Keystore-backed encryption for any saved secrets. Do not expose an unrestricted JavaScript interface to website origins.
6. **Validation and release:** run Rust/frontend tests, Android ARM64 build, and emulator smoke tests for browsing, navigation, tab switching, full-screen media, uploads/downloads, pause/resume, memory pressure, and session recovery. Measure package size and idle/one-page/video memory on a named emulator/device. Publish a beta only after those checks; require a private, stable Android signing key through GitHub Actions secrets before a release intended to upgrade in place. Never commit the key.

## Implemented in this branch

- A Tauri Android project and Kotlin Android System WebView tab plugin keep the web page layer native and the React controls in the Tauri shell. Only the active page WebView is shown; the shell and page view use explicit bounds so browser controls remain outside the page hit area.
- Tab creation, activation, close, navigation, history, reload, per-tab zoom, layout updates, Android Back, system share, file selection, DownloadManager downloads, external links, browser-role prompt, and video custom fullscreen have Android adapters.
- The address bar can be placed at the top or bottom under **Einstellungen → Aussehen**. The preference is stored with the shared Rust settings and applied to the native page WebView bounds.
- Android shows an explicit unsupported state for Chrome extensions; desktop extension assets are excluded from generated Android assets. The Android page-view currently performs top-level tracker-domain checks only, not full subresource filtering.
- The Android workflow builds an ARM64 debug APK and publishes it as a short-lived GitHub Actions artifact for testing. It does not create a public release or distribute an unsigned APK from the website.

## Current environment and release gates

The Android SDK, NDK, JDK and ARM64 Rust target are installed in a separate tools directory under Downloads, leaving the existing app profile and installation untouched. The ARM64 Rust library compiles with `cargo check --target aarch64-linux-android`. The full `tauri android build` command still hits a local Tauri CLI-options RPC timeout in its generated `android-studio-script` callback. After the Tauri CLI built the native library and frontend, direct Gradle assembly with the Rust callback task excluded succeeded using the same generated Android project; Gradle compiled the Kotlin plugin and linked the APK. An in-process Kotlin compiler setting is committed to avoid the local compiler-daemon connection failure.

The locally available APK is `src-tauri/gen/android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk` from beta `0.1.5-beta.1` (198.80 MiB, SHA-256 `0A62E36EE3C93A5626DF7F0456CA68018BF7EE6134BC692A1EA7684556D5966D`). `aapt` confirms package `com.veloce.browser`, `versionCode=1005` and `arm64-v8a`; `apksigner` verifies its Android Debug certificate. It predates the beta `0.1.6-beta.1` SDK-workflow fix and the packaging-only setting that asks Android Gradle Plugin to strip native debug symbols. Its debug certificate is machine-local and is not a stable update key. The APK is debuggable and must be treated as a development build, not an in-place-updatable release.

No Android emulator package/system image or connected device is installed, so tabs, touch, downloads, uploads, share sheet, fullscreen, role picker, session restore, memory and performance have not been tested at runtime. The CI APK build succeeded, but the earlier artifact-collection step failed because it assumed the wrong output path; the corrected collection step awaits a fresh run. A public Play Store or website download needs a stable private Android signing key; none is present in this workspace and keys must never be committed. Until signing and real-device checks are complete, label the build as development-only and do not replace the website's working desktop downloads.

No APK, Android feature, or performance result is marked complete until it has been built and tested. The Windows/Linux code is not to be rewritten as part of this Android adapter work.
