# Veloce Browser

Veloce is a new, separate Tauri 2 browser project. It does not modify Astra's source, installation, or profile. The shell is React/TypeScript, the tab and navigation coordinator is Rust, and pages render in native child WebViews owned by the same application window.

## Included in this first build

- URL and search handling in one Rust navigation path; search provider is configurable.
- Native WebView tabs, tab selection/close/create, browser history commands, page reload and popup-to-tab routing.
- Page zoom per tab through the native WebView zoom API, persisted in the session. WebView native zoom shortcuts are enabled; shell focus is also handled by the React key layer.
- Correct child-view bounds below Veloce's browser chrome. Settings and menus are drawn in the same window above the still-visible page WebView; on Windows Veloce makes its shell WebView transparent and adjusts native child-window z-order so the page can remain visible and playing behind a panel.
- A best-effort background-tab policy. When memory saving is enabled, inactive views are hidden immediately; tabs with no WebView2-reported audio and no playing video are assigned the low-memory target and suspended with `TrySuspend`. The separate idle-discard timer can still close an inactive, unpinned view after its selected timeout; restoring a discarded page reloads its last URL, so scroll position, form state and JavaScript heap state may be lost.
- A conservative list that blocks known tracker domains only when they are the top-level navigation destination.
- Per-user settings and session files in Veloce's own application data directory.
- A built-in Gaming profile that shortens the discard delay for unpinned background tabs. It does not change OS power, security, or process-priority settings.
- Seven settings areas: Allgemein, Aussehen, Tabs, Datenschutz, Leistung, Video & Medien, and Erweiterungen. Profiles are Balanced, Resource Saver, Smooth Video and Gaming; a manual change marks the profile as Custom.
- A manual WebView2 process snapshot in Settings → Leistung. It queries the WebView2 environment process list, deduplicates PIDs, adds the Veloce host process if necessary, and reports working set and private commit. It is an on-demand snapshot, not a background monitor.
- A single shared `ICoreWebView2Environment` for the Tauri shell and every page tab. Tabs therefore share the same default WebView2 profile and its disk cache, cookies and extension collection. WebView2 remains a multi-process engine; this does not collapse all tabs into one renderer process.
- An immediate best-effort pause of inactive, silent tabs. WebView2's native `IsDocumentPlayingAudio` check covers media audio/Web Audio; a small native-script query checks playing video elements. Active audio or video tabs are kept at the normal memory target and are not suspended. When a tab is selected, WebView2 is resumed before it is shown.
- GPU acceleration and the WebView2 profile's disk cache stay on by default. Veloce does not add `--disable-gpu`, cache-disabling, or experimental startup flags, and the settings UI no longer offers a GPU-off switch.
- A persistent Downloads toolbar icon and in-app list, fed by Wry/WebView2 start and finish events. Veloce keeps recent filename, local destination, timestamp and final state in its own app data folder; clearing the list does not delete downloaded files. The installed Wry download event surface does not expose reliable byte progress, so active downloads show an indeterminate spinner, never invented percentages. WebView2 chooses the normal download destination.
- A Windows default-browser registration action under Settings → Allgemein. It registers Veloce's per-user browser capabilities and opens Windows Default Apps settings; Windows still requires the user to select Veloce for HTTP/HTTPS. Any “once” versus “always” choice belongs to Windows/the app that launched the link. Veloce accepts valid HTTP/HTTPS launch arguments and opens them as a tab after session restoration.
- A separate session-restore switch, Graphite/OLED appearance options and four accent colors.
- Windows WebView2 extensions: the official uBlock Origin Lite `2026.1116.1650` and SponsorBlock `6.1.7` packages are bundled and installed into Veloce's existing WebView2 profile. Settings report the profile's actual installed/enabled state. Extension functions and Chrome APIs remain subject to the installed WebView2 Runtime.
- Extensions can be enabled/disabled through WebView2, custom unpacked folders can be checked for manifest permissions and referenced local files before adding, and custom entries can be removed where supported. The bundled extensions can be disabled but are not removable by this manager.
- A YouTube-only “fill video area” option. It uses `object-fit: cover` to crop away player letterboxing; this can cut off image edges and cannot remove black bars encoded into the video itself.
- An optional YouTube notice filter, enabled by default, that hides only the interruption toast when its action link identifies the ad-blocker notice. It does not alter requests, playback, uBlock Origin Lite, or SponsorBlock. YouTube can change the markup, and hiding the toast does not remove any underlying startup delay.
- Native webpage/video fullscreen handling on Windows. WebView2 fullscreen requests expand the child view to the window and enter native fullscreen, hiding Veloce's browser chrome and the taskbar; Escape or the page's exit control returns to the normal window.

## Windows responsiveness fix

Native WebView creation and control calls run on a serialized blocking worker instead of the synchronous IPC/UI path. This avoids the documented Windows deadlock risk when `Window::add_child` waits for the UI thread while a synchronous command is running. The shell also reports navigation failures visibly and receives explicit page-load start/finish events. The fix compiles, but the packaged GUI still needs a manual interaction check on the target Windows desktop.

## Performance refinement (2026-11-23)

The app now obtains the Tauri shell's WebView2 environment and passes that exact environment to each page tab. The integrated extension API remains enabled, so uBlock Origin Lite and SponsorBlock continue to use the same WebView2 profile. Silent, non-video inactive tabs are hidden, set to WebView2's Low memory target, and passed to `TrySuspend`; selected tabs are resumed before being shown. The memory saver applies to already-open background tabs when enabled. No before/after timing or memory improvement has been measured, and YouTube requests, filters, quality, and player behavior were left unchanged.

## Important engine limits

Tauri/Wry uses the system WebView: WebView2 on Windows, WKWebView on macOS and WebKitGTK on Linux. The two bundled extensions are supported only on Windows, when the WebView2 Runtime exposes the browser-extension API (Runtime 120 or newer); Veloce confirms installation and enabled status through that API but cannot promise that every Chrome extension API is implemented by WebView2. Direct Chrome Web Store installation is not implemented. SponsorBlock depends on its upstream segment service, and uBlock Origin Lite uses its own filter lists. The conservative Rust top-level tracker list is separate and is not a full subresource blocker. There is no password vault or cross-device sync.

Video “fill” deliberately crops content to cover the player area. It does not upscale detail, change the source quality, or remove black bars that are part of the video pixels. Native fullscreen is enabled only when a page requests WebView2 fullscreen and has not been practically checked with a live YouTube session in this environment.

Extension license notices and source archives are in `src-tauri/licenses/extensions/`.

The Veloce project itself does not yet include a project-wide license. Public visibility on GitHub does not grant permission to reuse or redistribute Veloce's own code; the bundled third-party components retain the licenses in their notices.

The 300 MB idle and 700 MB ten-tab figures are evaluation targets, not enforced limits. Web content, the system WebView, GPU processes, and device drivers determine actual use. Discarding a page closes its WebView but cannot guarantee a specific amount of memory is returned immediately. Likewise, this project does not promise a sub-second cold start or fixed frame rate. See [the architecture note](docs/ARCHITECTURE.md) for the platform plan and measurement method.

The in-app measurement reports CPU process working set and private commit for processes identified by WebView2's environment API plus the Veloce host. Working-set totals can include shared pages. GPU memory is not reported because the WebView2 process API does not provide a reliable per-app GPU-memory total. Other WebView2 environments that do not belong to Veloce are excluded; processes that Windows will not allow Veloce to query are counted as inaccessible.

`TrySuspend` is best effort: WebView2 can refuse it, and an already-running network request may still finish. It pauses page timers and animations but is not a per-tab network firewall. A page that does not expose a reliable media state is left unsuspended rather than risking interruption. WebView2 does not provide a dependable mapping from each tab to an isolated renderer process; lowering the priority of a shared renderer could also slow an active tab, so Veloce deliberately leaves Windows process priorities unchanged. The low-memory target is advisory, may evict caches, and is not a hard cap. The existing idle-discard timer is more disruptive: it closes the view and reloads its last URL later.

Extension review validates the manifest format, requested string permissions, host patterns and concrete referenced files, then asks the installed WebView2 Runtime to add the folder. It is not a malware scanner or publisher-signature check. Wildcard web-accessible-resource patterns cannot be validated as individual files. Install only extension folders you trust. WebView2's extension surface is not identical to Chrome's; the Chrome Web Store's normal install button is not supported as a one-click Chrome install path here.

For YouTube, Veloce leaves requests, filters, player quality and bundled extension preferences unchanged. The optional crop style and narrowly targeted interruption-toast cosmetic filter are configurable. No live YouTube playback measurement is claimed unless it is recorded below.

## Verification status for the performance change (2026-11-23)

- `npm run build`, `cargo fmt --check` and `cargo check --locked` passed after this change.
- I did not run Rust tests, launch the desktop app, or perform interactive tests of tab switching, audio/video protection, extension behavior, or fullscreen. The event-driven media monitor and suspend/resume flow therefore still need a live Windows check.
- No new RAM/CPU measurement was taken, so no performance improvement is claimed. A previous, single-machine idle sample from an earlier build reported 358.6 MiB working set and 251.2 MiB private commit; it is not a before/after comparison for this change, and GPU memory was not included.
- The optimized Windows release executable compiled to `src-tauri/target/release/veloce-browser.exe`.
- NSIS installer packaging was attempted, but Tauri could not download its NSIS prerequisite from GitHub because DNS resolution failed (`os error 11001`). Therefore no installer containing these changes was produced; any existing NSIS installer predates them. The standalone executable is not represented as a verified, self-contained installer.

## Build

The Windows installer release workflow runs when a `v*` tag is pushed. The website can download the latest setup directly from the release asset without opening GitHub's release page; see [the website download integration](docs/website-download-integration.md). The first link works only after the first tagged release has finished.

Install Node.js/npm, Rust stable, and the native Tauri prerequisites for the operating system you are building on. The complete target-specific prerequisites are maintained by the [official Tauri prerequisites guide](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev
```

Production build on the current host:

```sh
npm run build
npm run tauri:build -- --bundles nsis
```

On Windows, the resulting NSIS installer is:

`src-tauri/target/release/bundle/nsis/Veloce Browser_0.1.0_x64-setup.exe`. This installer is unsigned. Installing it was not part of the build verification.

Tauri builds for the host OS. Windows requires Microsoft C++ Build Tools/Windows SDK and the WebView2 Runtime; macOS requires Xcode command-line tools; Linux requires the documented WebKitGTK/GTK development packages. Build each release on its target OS or set up that platform's supported cross-compilation toolchain. A Windows build does not validate the macOS or Linux backend.

## Data

Veloce uses the OS-specific Tauri application-data directory under its own identifier `com.veloce.browser`. Session/settings files are JSON; they contain tab URLs and preferences, not passwords. When password autosave is enabled, credentials are handled by WebView2's profile storage and are not written into Veloce's JSON files; storage security and behavior are provided by WebView2/Windows, with no Veloce cloud sync. Do not treat this baseline as a finished consumer browser until the missing security, network-filter and platform-specific test work is completed.

Video progress is stored locally in `playback.json` in the same application-data directory. It records the page/video key, playback position, duration, and whether playback was active; up to 100 recent checkpoints are retained and a completed video is removed. The “Video an der letzten Stelle fortsetzen” setting restores the saved position and attempts autoplay only when the video was playing before the last checkpoint. Site autoplay rules or WebView2 may require the user to press Play. Session and progress data normally remain through an app update or reinstall that leaves this application-data directory intact; manually deleting profile data or a cleanup tool can remove them. Progress is checkpointed during playback, so an abrupt power loss can lose the last few seconds.
