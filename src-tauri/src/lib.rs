use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{
    webview::{Color, DownloadEvent, NewWindowResponse, PageLoadEvent, Webview, WebviewBuilder},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    State, WebviewUrl, Window, WindowEvent, Wry,
};
use url::Url;

#[cfg(windows)]
use webview2_com::{
    BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler, CoTaskMemPWSTR,
    ContainsFullScreenElementChangedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2BrowserExtension, ICoreWebView2Environment, ICoreWebView2Environment8,
        ICoreWebView2Profile7, ICoreWebView2Settings4, ICoreWebView2_13, ICoreWebView2_19,
        ICoreWebView2_2, ICoreWebView2_3, ICoreWebView2_8,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    },
    ProfileAddBrowserExtensionCompletedHandler, ProfileGetBrowserExtensionsCompletedHandler,
    TrySuspendCompletedHandler, WebMessageReceivedEventHandler,
};
#[cfg(windows)]
use windows::core::{Interface, HSTRING, PWSTR};
#[cfg(windows)]
use windows::Win32::{
    Foundation::{CloseHandle, HWND},
    System::{
        ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        },
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    },
    UI::WindowsAndMessaging::{
        SetWindowPos, HWND_BOTTOM, HWND_TOP, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOOWNERZORDER, SWP_NOSIZE,
    },
};

const START_URL: &str = "veloce://start";
const MEDIA_ACTIVITY_SCRIPT: &str = r#"(() => {
  if (window.__veloceMediaActivityInstalled) return;
  window.__veloceMediaActivityInstalled = true;
  const bridge = window.chrome && window.chrome.webview;
  if (!bridge || typeof bridge.postMessage !== 'function') return;
  const frameId = (crypto && crypto.randomUUID) ? crypto.randomUUID() : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  let lastState = null;
  const report = () => {
    const playingVideo = Array.from(document.querySelectorAll('video')).some((video) => !video.paused && !video.ended);
    if (playingVideo === lastState) return;
    lastState = playingVideo;
    try { bridge.postMessage({type: 'veloce-media-state', frameId, playingVideo}); } catch (_) {}
  };
  const schedule = () => queueMicrotask(report);
  for (const name of ['play', 'playing', 'pause', 'ended', 'emptied']) document.addEventListener(name, schedule, true);
  window.addEventListener('pagehide', () => {
    if (lastState === true) {
      lastState = false;
      try { bridge.postMessage({type: 'veloce-media-state', frameId, playingVideo: false}); } catch (_) {}
    }
  }, {capture: true});
  for (const name of ['yt-navigate-finish', 'yt-page-data-updated']) window.addEventListener(name, schedule, true);
  document.addEventListener('DOMContentLoaded', schedule, {once: true});
  report();
})()"#;
const YOUTUBE_FILL_STYLE_ID: &str = "veloce-youtube-video-fill";
const YOUTUBE_FILL_CSS: &str = "#movie_player .html5-video-container{width:100%!important;height:100%!important;overflow:hidden!important}#movie_player video.html5-main-video{width:100%!important;height:100%!important;object-fit:cover!important}";
const YOUTUBE_INTERRUPTION_STYLE_ID: &str = "veloce-youtube-interruption-notice";
const YOUTUBE_PLAYBACK_DIAGNOSTICS_SCRIPT: &str = r#"(() => {
  if (!/(^|\.)youtube\.com$/i.test(location.hostname)) return;
  if (window.__veloceYoutubeDiagnosticsInstalled) return;
  window.__veloceYoutubeDiagnosticsInstalled = true;
  let sequence = 0;
  let queued = [];
  let active = null;
  const attached = new WeakSet();
  const allowedPhases = new Set(['page-start', 'dom-content-loaded', 'window-load', 'video-click', 'player-request', 'metadata', 'canplay', 'first-frame', 'buffer-wait', 'buffer-resume', 'slow-resource']);
  const post = (message) => {
    try {
      const bridge = window.chrome && window.chrome.webview;
      if (bridge && typeof bridge.postMessage === 'function') bridge.postMessage(message);
      else if (queued.length < 24) queued.push(message);
    } catch (_) {}
  };
  window.__veloceFlushYoutubeDiagnostics = () => {
    const bridge = window.chrome && window.chrome.webview;
    if (!bridge || typeof bridge.postMessage !== 'function') return;
    const pending = queued;
    queued = [];
    for (const message of pending) post(message);
  };
  const begin = (source) => {
    active = { id: `${Math.floor(performance.timeOrigin)}-${++sequence}`, source, startedAt: performance.now(), playerRequested: false, frameReported: false, stallStartedAt: null, slowResources: 0 };
    return active;
  };
  const report = (phase, details = {}) => {
    if (!allowedPhases.has(phase)) return;
    if (!active) begin('page');
    post({
      type: 'veloce-youtube-metric',
      attemptId: active.id,
      source: active.source,
      phase,
      elapsedMs: Math.max(0, Math.round(performance.now() - active.startedAt)),
      ...details,
    });
  };
  let lastReportedLocation = '';
  const reportLocation = () => {
    const current = location.href;
    if (current === lastReportedLocation) return;
    lastReportedLocation = current;
    post({ type: 'veloce-youtube-location', url: current });
  };
  begin('page');
  report('page-start');
  reportLocation();
  document.addEventListener('DOMContentLoaded', () => report('dom-content-loaded'), { once: true });
  window.addEventListener('load', () => report('window-load'), { once: true });
  document.addEventListener('click', (event) => {
    const anchor = event.target && event.target.closest ? event.target.closest('a[href]') : null;
    if (!anchor) return;
    try {
      const target = new URL(anchor.href, location.href);
      if (target.hostname !== location.hostname) return;
      const isVideo = (target.pathname === '/watch' && target.searchParams.has('v')) || target.pathname.startsWith('/shorts/');
      if (!isVideo) return;
      begin('click');
      report('video-click');
      window.setTimeout(scanVideos, 0);
    } catch (_) {}
  }, true);
  const attachVideo = (video) => {
    if (!(video instanceof HTMLVideoElement) || attached.has(video)) return;
    attached.add(video);
    video.addEventListener('loadstart', () => {
      if (!active || performance.now() - active.startedAt > 120000) begin('page');
      if (!active.playerRequested) {
        active.playerRequested = true;
        report('player-request');
      }
    });
    video.addEventListener('loadedmetadata', () => report('metadata'));
    video.addEventListener('canplay', () => report('canplay'));
    video.addEventListener('playing', () => {
      if (active.frameReported) {
        if (active.stallStartedAt !== null) {
          report('buffer-resume', { stallMs: Math.max(0, Math.round(performance.now() - active.stallStartedAt)) });
          active.stallStartedAt = null;
        }
        return;
      }
      active.frameReported = true;
      const frameAttemptId = active.id;
      if (typeof video.requestVideoFrameCallback === 'function') {
        video.requestVideoFrameCallback(() => {
          if (active && active.id === frameAttemptId) report('first-frame');
        });
      } else {
        report('first-frame');
      }
    });
    video.addEventListener('waiting', () => {
      active.stallStartedAt = performance.now();
      report('buffer-wait');
    });
  };
  const scanVideos = () => {
    document.querySelectorAll('video').forEach(attachVideo);
  };
  document.addEventListener('DOMContentLoaded', scanVideos, { once: true });
  for (const eventName of ['yt-navigate-finish', 'yt-page-data-updated']) {
    window.addEventListener(eventName, () => window.setTimeout(() => {
      scanVideos();
      reportLocation();
    }, 0));
  }
  window.addEventListener('popstate', reportLocation);
  if (typeof PerformanceObserver === 'function') {
    try {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (!active || active.slowResources >= 5 || entry.duration < 800 || entry.startTime < active.startedAt) continue;
          let host = '';
          try { host = new URL(entry.name).hostname.toLowerCase(); } catch (_) { continue; }
          const resourceHost = host.endsWith('.googlevideo.com') || host === 'googlevideo.com' ? 'googlevideo.com'
            : host === 'youtubei.googleapis.com' ? host
            : host.endsWith('.ytimg.com') || host === 'ytimg.com' ? 'ytimg.com'
            : host.endsWith('.youtube.com') || host === 'youtube.com' ? 'youtube.com' : '';
          if (!resourceHost) continue;
          active.slowResources += 1;
          const status = typeof entry.responseStatus === 'number' && entry.responseStatus > 0 ? entry.responseStatus : null;
          report('slow-resource', {
            resourceHost,
            resourceType: String(entry.initiatorType || 'resource').slice(0, 24),
            resourceDurationMs: Math.round(entry.duration),
            responseStatus: status,
            transferBytes: Math.max(0, Math.round(entry.transferSize || 0)),
          });
        }
      }).observe({ type: 'resource', buffered: true });
    } catch (_) {}
  }
})()"#;
const TRACKING_DOMAINS: &[&str] = &[
    "adservice.google.com",
    "doubleclick.net",
    "google-analytics.com",
    "googlesyndication.com",
    "googletagmanager.com",
    "scorecardresearch.com",
    "taboola.com",
    "outbrain.com",
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserTab {
    id: String,
    url: String,
    title: String,
    pinned: bool,
    sleeping: bool,
    #[serde(default)]
    suspended: bool,
    #[serde(default = "default_zoom")]
    zoom: f64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuickShortcut {
    id: String,
    label: String,
    url: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookmarkEntry {
    id: String,
    title: String,
    url: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadEntry {
    id: String,
    file_name: String,
    path: String,
    started_at_ms: u64,
    status: String,
    #[serde(skip)]
    source_url: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackCheckpoint {
    key: String,
    url: String,
    position_seconds: f64,
    duration_seconds: f64,
    was_playing: bool,
    updated_at: u64,
}

fn default_zoom() -> f64 {
    1.0
}

fn clamp_zoom(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.5, 3.0)
    } else {
        1.0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct BrowserSettings {
    search_engine: String,
    tracking_protection: bool,
    sleep_after_minutes: u8,
    gaming_mode: bool,
    reduce_motion: bool,
    performance_profile: String,
    memory_saver: bool,
    hardware_acceleration: bool,
    restore_session: bool,
    password_autosave: bool,
    resume_video_playback: bool,
    theme: String,
    accent_color: String,
    #[serde(default = "default_youtube_video_fill")]
    youtube_video_fill: bool,
    #[serde(
        default = "default_hide_youtube_interruptions",
        alias = "hideYoutubeInterruptions"
    )]
    youtube_hide_interruptions: bool,
}

fn default_youtube_video_fill() -> bool {
    true
}

fn default_hide_youtube_interruptions() -> bool {
    true
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            search_engine: "duckduckgo".into(),
            tracking_protection: true,
            sleep_after_minutes: 3,
            gaming_mode: false,
            reduce_motion: true,
            performance_profile: "balanced".into(),
            memory_saver: true,
            hardware_acceleration: true,
            restore_session: true,
            password_autosave: true,
            resume_video_playback: true,
            theme: "dark".into(),
            accent_color: "violet".into(),
            youtube_video_fill: true,
            youtube_hide_interruptions: true,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionStatus {
    key: String,
    id: String,
    name: String,
    bundled_version: String,
    installed: bool,
    enabled: bool,
    removable: bool,
    detail: String,
}

fn pending_extensions() -> Vec<ExtensionStatus> {
    vec![
        ExtensionStatus {
            key: "ublock-origin-lite".into(),
            id: String::new(),
            name: "uBlock Origin Lite".into(),
            bundled_version: "2026.1116.1650".into(),
            installed: false,
            enabled: false,
            removable: false,
            detail: "Wird beim ersten Webseiten-Tab mit WebView2 abgeglichen.".into(),
        },
        ExtensionStatus {
            key: "sponsorblock".into(),
            id: String::new(),
            name: "SponsorBlock".into(),
            bundled_version: "6.1.7".into(),
            installed: false,
            enabled: false,
            removable: false,
            detail: "Wird beim ersten Webseiten-Tab mit WebView2 abgeglichen.".into(),
        },
    ]
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedSession {
    tabs: Vec<BrowserTab>,
    active_id: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TabsSnapshot {
    tabs: Vec<BrowserTab>,
    active_id: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryProcessUsage {
    pid: u32,
    kind: String,
    working_set_bytes: u64,
    private_commit_bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemorySnapshot {
    process_count: usize,
    webview_process_count: usize,
    inaccessible_process_count: usize,
    working_set_bytes: u64,
    private_commit_bytes: u64,
    processes: Vec<MemoryProcessUsage>,
    background_memory_target_supported: bool,
    gpu_memory_included: bool,
    working_set_may_include_shared_pages: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionManifestPreview {
    path: String,
    name: String,
    version: String,
    manifest_version: u8,
    permissions: Vec<String>,
    host_permissions: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedWindowGeometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
    monitor_name: Option<String>,
    monitor_x: i32,
    monitor_y: i32,
}

#[derive(Clone)]
struct AppState {
    native_ops: Arc<Mutex<()>>,
    playback_ops: Arc<Mutex<()>>,
    tabs: Arc<Mutex<Vec<BrowserTab>>>,
    active_video_frames: Arc<Mutex<HashSet<String>>>,
    active_id: Arc<Mutex<Option<String>>>,
    width: Arc<Mutex<f64>>,
    height: Arc<Mutex<f64>>,
    chrome_height: Arc<Mutex<f64>>,
    settings: Arc<Mutex<BrowserSettings>>,
    extension_status: Arc<Mutex<Vec<ExtensionStatus>>>,
    downloads: Arc<Mutex<Vec<DownloadEntry>>>,
    extension_attempted: Arc<Mutex<bool>>,
    memory_target_supported: Arc<Mutex<Option<bool>>>,
    #[cfg(windows)]
    shared_webview2_environment: Arc<Mutex<Option<SharedWebView2Environment>>>,
    window_geometry_revision: Arc<AtomicUsize>,
    window_geometry_save_scheduled: Arc<AtomicBool>,
    window_geometry_closing: Arc<AtomicBool>,
}

#[cfg(windows)]
#[derive(Clone)]
struct SharedWebView2Environment(ICoreWebView2Environment);

// Tauri's WebviewAttributes uses the same Send/Sync boundary to hand this
// environment to its main-thread WebView creation callback. The COM object is
// only cloned and consumed by WebView2/Wry; this wrapper never calls COM from
// an arbitrary worker thread.
#[cfg(windows)]
unsafe impl Send for SharedWebView2Environment {}
#[cfg(windows)]
unsafe impl Sync for SharedWebView2Environment {}

impl Default for AppState {
    fn default() -> Self {
        Self {
            native_ops: Arc::new(Mutex::new(())),
            playback_ops: Arc::new(Mutex::new(())),
            tabs: Arc::new(Mutex::new(Vec::new())),
            active_video_frames: Arc::new(Mutex::new(HashSet::new())),
            active_id: Arc::new(Mutex::new(None)),
            width: Arc::new(Mutex::new(1360.0)),
            height: Arc::new(Mutex::new(860.0)),
            chrome_height: Arc::new(Mutex::new(96.0)),
            settings: Arc::new(Mutex::new(BrowserSettings::default())),
            extension_status: Arc::new(Mutex::new(pending_extensions())),
            downloads: Arc::new(Mutex::new(Vec::new())),
            extension_attempted: Arc::new(Mutex::new(false)),
            memory_target_supported: Arc::new(Mutex::new(None)),
            #[cfg(windows)]
            shared_webview2_environment: Arc::new(Mutex::new(None)),
            window_geometry_revision: Arc::new(AtomicUsize::new(0)),
            window_geometry_save_scheduled: Arc::new(AtomicBool::new(false)),
            window_geometry_closing: Arc::new(AtomicBool::new(false)),
        }
    }
}

async fn run_blocking<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("Browser-Aufgabe konnte nicht ausgeführt werden: {error}"))?
}

fn lock_native_ops(state: &AppState) -> Result<std::sync::MutexGuard<'_, ()>, String> {
    state
        .native_ops
        .lock()
        .map_err(|_| "Die native Browseransicht ist vorübergehend gesperrt.".to_string())
}

#[cfg(windows)]
fn set_webview_memory_target(view: &Webview<Wry>, low: bool) -> Result<bool, String> {
    let (sender, receiver) = mpsc::channel();
    view.with_webview(move |platform| {
        let result = (|| -> windows::core::Result<bool> {
            let core = unsafe { platform.controller().CoreWebView2()? };
            let core19 = match core.cast::<ICoreWebView2_19>() {
                Ok(core) => core,
                Err(_) => return Ok(false),
            };
            unsafe {
                core19.SetMemoryUsageTargetLevel(if low {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                } else {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                })?;
            }
            Ok(true)
        })()
        .map_err(|error| error.to_string());
        let _ = sender.send(result);
    })
    .map_err(|error| format!("WebView2-Speicherziel konnte nicht gesetzt werden: {error}"))?;
    receiver
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "Zeitüberschreitung beim WebView2-Speicherziel.".to_string())?
}

#[cfg(windows)]
fn shared_webview2_environment(
    app: &AppHandle<Wry>,
    state: &AppState,
) -> Result<ICoreWebView2Environment, String> {
    if let Some(environment) = state
        .shared_webview2_environment
        .lock()
        .map_err(|_| "Die gemeinsame WebView2-Umgebung ist gesperrt.".to_string())?
        .clone()
    {
        return Ok(environment.0);
    }

    // The Tauri shell and every page tab use the exact same environment and
    // default profile. This shares WebView2's cache, cookies and extension set.
    let shell = app
        .get_webview("main")
        .ok_or("Die WebView2-Oberfläche ist noch nicht bereit.")?;
    let (sender, receiver) = mpsc::channel();
    shell
        .with_webview(move |platform| {
            let _ = sender.send(SharedWebView2Environment(platform.environment()));
        })
        .map_err(|error| {
            format!("Gemeinsame WebView2-Umgebung konnte nicht gelesen werden: {error}")
        })?;
    let environment = receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "Zeitüberschreitung beim Lesen der WebView2-Umgebung.".to_string())?;
    let mut shared = state
        .shared_webview2_environment
        .lock()
        .map_err(|_| "Die gemeinsame WebView2-Umgebung ist gesperrt.".to_string())?;
    Ok(shared.get_or_insert(environment).clone().0)
}

#[cfg(windows)]
fn webview_has_active_audio(view: &Webview<Wry>) -> Result<bool, String> {
    let (sender, receiver) = mpsc::channel::<Result<bool, String>>();
    view.with_webview(move |platform| {
        let result = (|| -> windows::core::Result<bool> {
            let core = unsafe { platform.controller().CoreWebView2()? };
            // The native signal also covers Web Audio, not only media tags.
            let core8 = core.cast::<ICoreWebView2_8>()?;
            let mut playing_audio = windows::core::BOOL(0);
            unsafe { core8.IsDocumentPlayingAudio(&mut playing_audio)? };
            Ok(playing_audio.as_bool())
        })();
        let _ = sender.send(result.map_err(|error| error.to_string()));
    })
    .map_err(|error| {
        format!("Audio-Status des Hintergrundtabs konnte nicht geprüft werden: {error}")
    })?;
    receiver
        .recv_timeout(Duration::from_millis(750))
        .map_err(|_| "Zeitüberschreitung beim Prüfen des Audio-Status.".to_string())?
}

#[cfg(windows)]
fn try_suspend_webview(
    app: &AppHandle<Wry>,
    state: &AppState,
    id: &str,
    view: &Webview<Wry>,
) -> Result<bool, String> {
    let (sender, receiver) = mpsc::channel::<Result<bool, String>>();
    let callback_app = app.clone();
    let callback_state = state.clone();
    let callback_id = id.to_string();
    view.with_webview(move |platform| {
        let result = (|| -> windows::core::Result<()> {
            let core = unsafe { platform.controller().CoreWebView2()? };
            let core3 = match core.cast::<ICoreWebView2_3>() {
                Ok(core) => core,
                Err(_) => {
                    let _ = sender.send(Ok(false));
                    return Ok(());
                }
            };
            let mut is_suspended = windows::core::BOOL(0);
            unsafe { core3.IsSuspended(&mut is_suspended)? };
            if is_suspended.as_bool() {
                let _ = sender.send(Ok(true));
                return Ok(());
            }
            let handler = TrySuspendCompletedHandler::create(Box::new(move |status, success| {
                match status.map(|_| success).map_err(|error| error.to_string()) {
                    Ok(true) => {
                        let still_inactive = callback_state
                            .active_id
                            .lock()
                            .map(|active| active.as_deref() != Some(callback_id.as_str()))
                            .unwrap_or(false);
                        set_tab_suspended_state(&callback_state, &callback_id, still_inactive);
                        emit_tabs(&callback_app, &callback_state);
                    }
                    Ok(false) => {
                        set_tab_suspended_state(&callback_state, &callback_id, false);
                        let _ = callback_app.emit(
                            "veloce-native-warning",
                            json!({"id": callback_id, "message": "WebView2 hat das Pausieren dieses Hintergrundtabs abgelehnt."}),
                        );
                        emit_tabs(&callback_app, &callback_state);
                    }
                    Err(error) => {
                        set_tab_suspended_state(&callback_state, &callback_id, false);
                        let _ = callback_app.emit(
                            "veloce-native-warning",
                            json!({"id": callback_id, "message": format!("WebView2-Pause ist fehlgeschlagen: {error}")}),
                        );
                        emit_tabs(&callback_app, &callback_state);
                    }
                }
                Ok(())
            }));
            unsafe { core3.TrySuspend(&handler)? };
            let _ = sender.send(Ok(true));
            Ok(())
        })();
        if let Err(error) = result {
            let _ = sender.send(Err(error.to_string()));
        }
    })
    .map_err(|error| format!("WebView2-Tab konnte nicht pausiert werden: {error}"))?;
    receiver
        .recv_timeout(Duration::from_millis(750))
        .map_err(|_| "Zeitüberschreitung beim Starten der WebView2-Pause.".to_string())?
}

#[cfg(windows)]
fn resume_webview(view: &Webview<Wry>) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    view.with_webview(move |platform| {
        let result = (|| -> windows::core::Result<()> {
            let core = unsafe { platform.controller().CoreWebView2()? };
            let core3 = match core.cast::<ICoreWebView2_3>() {
                Ok(core) => core,
                // An older WebView2 runtime cannot suspend tabs, but it can
                // still display and navigate them normally.
                Err(_) => return Ok(()),
            };
            let mut is_suspended = windows::core::BOOL(0);
            unsafe { core3.IsSuspended(&mut is_suspended)? };
            if is_suspended.as_bool() {
                unsafe { core3.Resume()? };
            }
            Ok(())
        })()
        .map_err(|error| error.to_string());
        let _ = sender.send(result);
    })
    .map_err(|error| format!("WebView2-Tab konnte nicht fortgesetzt werden: {error}"))?;
    receiver
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "Zeitüberschreitung beim Fortsetzen des WebView2-Tabs.".to_string())?
}

#[cfg(not(windows))]
fn webview_has_active_media(_view: &Webview<Wry>) -> Result<bool, String> {
    Ok(true)
}

#[cfg(not(windows))]
fn webview_has_active_audio(_view: &Webview<Wry>) -> Result<bool, String> {
    // Without a native per-WebView media signal, fail safe and leave the tab live.
    Ok(true)
}

#[cfg(not(windows))]
fn try_suspend_webview(
    _app: &AppHandle<Wry>,
    _state: &AppState,
    _id: &str,
    _view: &Webview<Wry>,
) -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(windows))]
fn resume_webview(_view: &Webview<Wry>) -> Result<(), String> {
    Ok(())
}

fn set_tab_suspended_state(state: &AppState, id: &str, suspended: bool) {
    if let Ok(mut tabs) = state.tabs.lock() {
        if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id) {
            tab.suspended = suspended;
        }
    }
}

fn tab_has_active_video(state: &AppState, id: &str) -> bool {
    let prefix = format!("{id}:");
    state
        .active_video_frames
        .lock()
        .map(|frames| frames.iter().any(|frame| frame.starts_with(&prefix)))
        .unwrap_or(true)
}

fn clear_tab_video_activity(state: &AppState, id: &str) {
    let prefix = format!("{id}:");
    if let Ok(mut frames) = state.active_video_frames.lock() {
        frames.retain(|frame| !frame.starts_with(&prefix));
    }
}

fn apply_background_tab_policy(
    app: &AppHandle<Wry>,
    state: &AppState,
    id: &str,
    view: &Webview<Wry>,
) -> Result<bool, String> {
    let active = state
        .active_id
        .lock()
        .map_err(|_| "Aktiver Tab ist nicht verfügbar.".to_string())?
        .as_deref()
        == Some(id);
    if active {
        return Ok(false);
    }

    // WebView2 requires the controller to be invisible before TrySuspend.
    view.hide().map_err(|error| error.to_string())?;
    if tab_has_active_video(state, id) || webview_has_active_audio(view)? {
        let _ = set_webview_memory_target(view, false);
        set_tab_suspended_state(state, id, false);
        return Ok(false);
    }

    let target_supported = set_webview_memory_target(view, true)?;
    *state
        .memory_target_supported
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(target_supported);
    let suspend_requested = try_suspend_webview(app, state, id, view)?;
    if !suspend_requested {
        set_tab_suspended_state(state, id, false);
        let _ = app.emit(
            "veloce-native-warning",
            json!({"id": id, "message": "Diese WebView2-Version unterstützt das Pausieren dieses Tabs nicht; die Seite bleibt verborgen und kann weiter Ressourcen verwenden."}),
        );
    }
    Ok(suspend_requested)
}

#[cfg(not(windows))]
fn set_webview_memory_target(_view: &Webview<Wry>, _low: bool) -> Result<bool, String> {
    Ok(false)
}

#[cfg(windows)]
fn set_webview_password_autosave(view: &Webview<Wry>, enabled: bool) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    view.with_webview(move |platform| {
        let result = (|| -> windows::core::Result<()> {
            let core = unsafe { platform.controller().CoreWebView2()? };
            let settings = unsafe { core.Settings()? };
            let settings4 = settings.cast::<ICoreWebView2Settings4>()?;
            unsafe { settings4.SetIsPasswordAutosaveEnabled(enabled)? };
            Ok(())
        })()
        .map_err(|error| error.to_string());
        let _ = sender.send(result);
    })
    .map_err(|error| format!("WebView2-Passwortoption konnte nicht geöffnet werden: {error}"))?;
    receiver
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "Zeitüberschreitung beim Ändern der Passwortoption.".to_string())?
        .map_err(|error| format!("WebView2 unterstützt die Passwortoption nicht: {error}"))
}

#[cfg(not(windows))]
fn set_webview_password_autosave(_view: &Webview<Wry>, _enabled: bool) -> Result<(), String> {
    Err("WebView2-Passwortspeicherung ist nur im Windows-Build verfügbar.".into())
}

#[cfg(windows)]
fn reorder_shell_webview(
    platform: &tauri::webview::PlatformWebview,
    above_pages: bool,
) -> windows::core::Result<()> {
    let controller = platform.controller();
    let mut shell = HWND::default();
    unsafe {
        // Wry creates a WRY_WEBVIEW container HWND for each WebView and passes
        // that HWND to WebView2 as its controller parent. ParentWindow therefore
        // gives us the shell container directly; searching below it for another
        // WRY_WEBVIEW child will always fail.
        controller.ParentWindow(&mut shell)?;
    }
    if shell == HWND::default() {
        return Err(windows::core::Error::new(
            windows::core::HRESULT(0x8000_4005_u32 as i32),
            "Veloce-Oberflächenfenster wurde nicht gefunden.",
        ));
    }
    let insert_after = if above_pages { HWND_TOP } else { HWND_BOTTOM };
    unsafe {
        SetWindowPos(
            shell,
            Some(insert_after),
            0,
            0,
            0,
            0,
            SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER,
        )?;
    }
    Ok(())
}

fn set_panel_overlay_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    enabled: bool,
) -> Result<(), String> {
    let _native = lock_native_ops(&state)?;
    let shell = app
        .get_webview("main")
        .ok_or("Die Veloce-Oberfläche fehlt.")?;
    shell
        .set_background_color(Some(if enabled {
            Color(0, 0, 0, 0)
        } else {
            Color(16, 17, 20, 255)
        }))
        .map_err(|error| {
            format!("Transparenz der Browseroberfläche konnte nicht geändert werden: {error}")
        })?;

    #[cfg(windows)]
    {
        let callback_app = app.clone();
        shell
            .with_webview(move |platform| {
                if let Err(error) = reorder_shell_webview(&platform, enabled) {
                    let _ = callback_app.emit(
                        "veloce-native-error",
                        format!("Browseroberfläche konnte nicht über dem Seiteninhalt angeordnet werden: {error}"),
                    );
                }
            })
            .map_err(|error| format!("Native Browseroberfläche konnte nicht angeordnet werden: {error}"))?;
    }

    if enabled {
        shell.set_focus().map_err(|error| error.to_string())?;
    } else if let Some(id) = state
        .active_id
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
    {
        if let Some(view) = app.get_webview(&id) {
            view.show().map_err(|error| error.to_string())?;
            view.set_focus().map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn set_panel_overlay(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let state = state.inner().clone();
    run_blocking(move || set_panel_overlay_blocking(app, state, enabled)).await
}

fn app_dir(app: &AppHandle<Wry>) -> Result<PathBuf, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&path)
        .map_err(|error| format!("Veloce-Datenordner konnte nicht angelegt werden: {error}"))?;
    Ok(path)
}

fn external_launch_url() -> Option<String> {
    std::env::args_os().skip(1).find_map(|argument| {
        let argument = argument.into_string().ok()?;
        let url = Url::parse(&argument).ok()?;
        (matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none())
        .then(|| url.to_string())
    })
}

#[cfg(windows)]
fn registry_set_string(key: &str, value_name: Option<&str>, value: &str) -> Result<(), String> {
    let full_key = format!(r"HKCU\{key}");
    let mut command = std::process::Command::new("reg.exe");
    command.arg("add").arg(&full_key);
    if let Some(value_name) = value_name {
        command.args(["/v", value_name]);
    } else {
        command.arg("/ve");
    }
    let output = command
        .args(["/t", "REG_SZ", "/d", value, "/f"])
        .output()
        .map_err(|error| {
            format!("Windows-Browserregistrierung konnte nicht gestartet werden: {error}")
        })?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if detail.is_empty() {
            format!("Windows lehnte den Registrierungseintrag {full_key} ab.")
        } else {
            format!("Windows lehnte den Registrierungseintrag {full_key} ab: {detail}")
        })
    }
}

#[cfg(windows)]
fn register_default_browser_blocking() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Veloce-Programmdatei konnte nicht gefunden werden: {error}"))?;
    let executable = executable.to_string_lossy().to_string();
    let executable_name = PathBuf::from(&executable)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Name der Veloce-Programmdatei ist ungültig.")?
        .to_string();
    let http_progid = "VeloceURL.HTTP";
    let https_progid = "VeloceURL.HTTPS";
    let html_progid = "VeloceHTML";
    let capabilities = r"Software\Clients\StartMenuInternet\VeloceBrowser\Capabilities";
    let command = format!("\"{executable}\" \"%1\"");
    let icon = format!("\"{executable}\",0");
    let root = r"Software\Clients\StartMenuInternet\VeloceBrowser";
    let entries = [
        (
            format!(r"Software\Classes\{http_progid}"),
            None,
            "Veloce HTTP-Adresse".to_string(),
        ),
        (
            format!(r"Software\Classes\{http_progid}"),
            Some("URL Protocol".to_string()),
            String::new(),
        ),
        (
            format!(r"Software\Classes\{http_progid}\DefaultIcon"),
            None,
            icon.clone(),
        ),
        (
            format!(r"Software\Classes\{http_progid}\shell\open\command"),
            None,
            command.clone(),
        ),
        (
            format!(r"Software\Classes\{https_progid}"),
            None,
            "Veloce HTTPS-Adresse".to_string(),
        ),
        (
            format!(r"Software\Classes\{https_progid}"),
            Some("URL Protocol".to_string()),
            String::new(),
        ),
        (
            format!(r"Software\Classes\{https_progid}\DefaultIcon"),
            None,
            icon.clone(),
        ),
        (
            format!(r"Software\Classes\{https_progid}\shell\open\command"),
            None,
            command.clone(),
        ),
        (
            format!(r"Software\Classes\{html_progid}"),
            None,
            "Veloce HTML-Dokument".to_string(),
        ),
        (
            format!(r"Software\Classes\{html_progid}\DefaultIcon"),
            None,
            icon.clone(),
        ),
        (
            format!(r"Software\Classes\{html_progid}\shell\open\command"),
            None,
            command.clone(),
        ),
        (
            format!(r"Software\Classes\http\OpenWithProgids"),
            Some(http_progid.to_string()),
            String::new(),
        ),
        (
            format!(r"Software\Classes\https\OpenWithProgids"),
            Some(https_progid.to_string()),
            String::new(),
        ),
        (
            format!(r"Software\Classes\.htm\OpenWithProgids"),
            Some(html_progid.to_string()),
            String::new(),
        ),
        (
            format!(r"Software\Classes\.html\OpenWithProgids"),
            Some(html_progid.to_string()),
            String::new(),
        ),
        (
            r"Software\Classes\Applications\veloce-browser.exe".to_string(),
            Some("FriendlyAppName".to_string()),
            "Veloce Browser".to_string(),
        ),
        (
            r"Software\Classes\Applications\veloce-browser.exe\shell\open\command".to_string(),
            None,
            command,
        ),
        (
            r"Software\Classes\Applications\veloce-browser.exe\SupportedTypes".to_string(),
            Some(".htm".to_string()),
            String::new(),
        ),
        (
            r"Software\Classes\Applications\veloce-browser.exe\SupportedTypes".to_string(),
            Some(".html".to_string()),
            String::new(),
        ),
        (root.to_string(), None, "Veloce Browser".to_string()),
        (format!(r"{root}\DefaultIcon"), None, icon),
        (
            format!(r"{root}\shell\open\command"),
            None,
            format!("\"{executable}\""),
        ),
        (
            format!(r"{capabilities}\StartMenu\StartMenuInternet"),
            None,
            executable_name,
        ),
        (
            capabilities.to_string(),
            Some("ApplicationName".to_string()),
            "Veloce Browser".to_string(),
        ),
        (
            capabilities.to_string(),
            Some("ApplicationDescription".to_string()),
            "Veloce Browser für HTTP- und HTTPS-Webseiten.".to_string(),
        ),
        (
            format!(r"{capabilities}\URLAssociations"),
            Some("http".to_string()),
            http_progid.to_string(),
        ),
        (
            format!(r"{capabilities}\URLAssociations"),
            Some("https".to_string()),
            https_progid.to_string(),
        ),
        (
            format!(r"{capabilities}\FileAssociations"),
            Some(".htm".to_string()),
            html_progid.to_string(),
        ),
        (
            format!(r"{capabilities}\FileAssociations"),
            Some(".html".to_string()),
            html_progid.to_string(),
        ),
        (
            r"Software\RegisteredApplications".to_string(),
            Some("Veloce Browser".to_string()),
            capabilities.to_string(),
        ),
    ];
    for (key, name, value) in entries {
        registry_set_string(&key, name.as_deref(), &value)?;
    }

    std::process::Command::new("explorer.exe")
        .arg("ms-settings:defaultapps?registeredAppUser=Veloce%20Browser")
        .spawn()
        .map_err(|error| {
            format!("Windows-Standard-App-Einstellungen konnten nicht geöffnet werden: {error}")
        })?;
    Ok(())
}

#[tauri::command]
async fn register_default_browser() -> Result<(), String> {
    #[cfg(windows)]
    {
        return run_blocking(register_default_browser_blocking).await;
    }
    #[cfg(not(windows))]
    {
        Err(
            "Die Standardbrowser-Registrierung ist in diesem Build nur für Windows eingerichtet."
                .into(),
        )
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &PathBuf) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_json<T: Serialize>(path: &PathBuf, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.pending");
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn capture_window_geometry(
    window: &Window<Wry>,
    maximized: bool,
) -> Result<SavedWindowGeometry, String> {
    let position = window
        .outer_position()
        .map_err(|error| format!("Fensterposition konnte nicht gelesen werden: {error}"))?;
    let size = window
        .outer_size()
        .map_err(|error| format!("Fenstergröße konnte nicht gelesen werden: {error}"))?;
    let monitor = window.current_monitor().ok().flatten();
    let (monitor_name, monitor_x, monitor_y) = monitor
        .map(|monitor| {
            (
                monitor.name().cloned(),
                monitor.position().x,
                monitor.position().y,
            )
        })
        .unwrap_or((None, position.x, position.y));
    Ok(SavedWindowGeometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized,
        monitor_name,
        monitor_x,
        monitor_y,
    })
}

fn window_rect_intersects_monitor(bounds: &SavedWindowGeometry, monitor: &tauri::Monitor) -> bool {
    let left = bounds.x as i64;
    let top = bounds.y as i64;
    let right = left + bounds.width as i64;
    let bottom = top + bounds.height as i64;
    let monitor_left = monitor.position().x as i64;
    let monitor_top = monitor.position().y as i64;
    let monitor_right = monitor_left + monitor.size().width as i64;
    let monitor_bottom = monitor_top + monitor.size().height as i64;
    (right.min(monitor_right) - left.max(monitor_left)) >= 120
        && (bottom.min(monitor_bottom) - top.max(monitor_top)) >= 120
}

fn restore_window_geometry(app: &AppHandle<Wry>) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or("Das Veloce-Hauptfenster fehlt.")?;
    let Some(saved) = read_json::<SavedWindowGeometry>(&app_dir(app)?.join("window.json")) else {
        return Ok(());
    };
    let monitors = window
        .available_monitors()
        .map_err(|error| format!("Bildschirme konnten nicht gelesen werden: {error}"))?;
    if monitors.is_empty() {
        return Ok(());
    }

    let named_monitor = saved
        .monitor_name
        .as_ref()
        .and_then(|name| monitors.iter().find(|monitor| monitor.name() == Some(name)));
    let overlapping_monitor = monitors
        .iter()
        .find(|monitor| window_rect_intersects_monitor(&saved, monitor));
    let monitor = named_monitor
        .or(overlapping_monitor)
        .unwrap_or(&monitors[0]);
    let work_area = monitor.work_area();
    let work_x = work_area.position.x;
    let work_y = work_area.position.y;
    let work_width = work_area.size.width.max(820);
    let work_height = work_area.size.height.max(560);
    let width = saved.width.clamp(820, work_width);
    let height = saved.height.clamp(560, work_height);

    let (mut x, mut y) = if named_monitor.is_some() {
        (
            monitor
                .position()
                .x
                .saturating_add(saved.x.saturating_sub(saved.monitor_x)),
            monitor
                .position()
                .y
                .saturating_add(saved.y.saturating_sub(saved.monitor_y)),
        )
    } else if overlapping_monitor.is_some() {
        (saved.x, saved.y)
    } else {
        (
            work_x.saturating_add((work_width.saturating_sub(width) / 2) as i32),
            work_y.saturating_add((work_height.saturating_sub(height) / 2) as i32),
        )
    };
    let max_x = work_x.saturating_add(work_width.saturating_sub(width) as i32);
    let max_y = work_y.saturating_add(work_height.saturating_sub(height) as i32);
    x = x.clamp(work_x, max_x);
    y = y.clamp(work_y, max_y);

    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|error| format!("Fenstergröße konnte nicht wiederhergestellt werden: {error}"))?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| {
            format!("Fensterposition konnte nicht wiederhergestellt werden: {error}")
        })?;
    if saved.maximized {
        window.maximize().map_err(|error| {
            format!("Fenster konnte nicht maximiert wiederhergestellt werden: {error}")
        })?;
    }
    Ok(())
}

fn save_window_geometry(app: &AppHandle<Wry>) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or("Das Veloce-Hauptfenster fehlt.")?;
    if window.is_minimized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return Ok(());
    }
    let maximized = window.is_maximized().unwrap_or(false);
    let path = app_dir(app)?.join("window.json");
    let mut geometry = if maximized {
        read_json::<SavedWindowGeometry>(&path).unwrap_or(capture_window_geometry(&window, true)?)
    } else {
        capture_window_geometry(&window, false)?
    };
    if maximized {
        if let Some(monitor) = window.current_monitor().ok().flatten() {
            let relative_x = geometry.x.saturating_sub(geometry.monitor_x);
            let relative_y = geometry.y.saturating_sub(geometry.monitor_y);
            geometry.x = monitor.position().x.saturating_add(relative_x);
            geometry.y = monitor.position().y.saturating_add(relative_y);
            geometry.monitor_name = monitor.name().cloned();
            geometry.monitor_x = monitor.position().x;
            geometry.monitor_y = monitor.position().y;
        }
        geometry.maximized = true;
    }
    write_json(&path, &geometry)
        .map_err(|error| format!("Fensterposition konnte nicht gespeichert werden: {error}"))
}

fn schedule_window_geometry_save(app: AppHandle<Wry>, state: AppState) {
    if state.window_geometry_closing.load(Ordering::Acquire) {
        return;
    }
    state
        .window_geometry_revision
        .fetch_add(1, Ordering::AcqRel);
    if state
        .window_geometry_save_scheduled
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let worker_state = state.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        loop {
            if worker_state.window_geometry_closing.load(Ordering::Acquire) {
                break;
            }
            let revision = worker_state
                .window_geometry_revision
                .load(Ordering::Acquire);
            std::thread::sleep(Duration::from_millis(350));
            if worker_state.window_geometry_closing.load(Ordering::Acquire) {
                break;
            }
            if worker_state
                .window_geometry_revision
                .load(Ordering::Acquire)
                != revision
            {
                continue;
            }
            if let Err(error) = save_window_geometry(&app) {
                eprintln!("{error}");
            }
            worker_state
                .window_geometry_save_scheduled
                .store(false, Ordering::Release);
            if worker_state.window_geometry_closing.load(Ordering::Acquire)
                || worker_state
                    .window_geometry_revision
                    .load(Ordering::Acquire)
                    == revision
            {
                break;
            }
            if worker_state
                .window_geometry_save_scheduled
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                break;
            }
        }
        worker_state
            .window_geometry_save_scheduled
            .store(false, Ordering::Release);
    });
}

fn persist_session(app: &AppHandle<Wry>, state: &AppState) {
    let tabs = state
        .tabs
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let active_id = state
        .active_id
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    if let Ok(dir) = app_dir(app) {
        if let Err(error) = write_json(&dir.join("session.json"), &SavedSession { tabs, active_id })
        {
            eprintln!("Sitzung konnte nicht gespeichert werden: {error}");
        }
    }
}

fn persist_downloads(app: &AppHandle<Wry>, state: &AppState) {
    let downloads = state
        .downloads
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    if let Ok(dir) = app_dir(app) {
        if let Err(error) = write_json(&dir.join("downloads.json"), &downloads) {
            eprintln!("Downloadliste konnte nicht gespeichert werden: {error}");
        }
    }
}

fn download_file_name(url: &Url, path: &PathBuf) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            url.path_segments()
                .and_then(|segments| segments.filter(|segment| !segment.is_empty()).next_back())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "Download".into())
}

fn record_download_requested(app: &AppHandle<Wry>, url: &Url, destination: &PathBuf) {
    let state = app.state::<AppState>();
    let entry = DownloadEntry {
        id: format!("download-{}", uuid::Uuid::new_v4().simple()),
        file_name: download_file_name(url, destination),
        path: destination.to_string_lossy().into_owned(),
        started_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64,
        status: "downloading".into(),
        source_url: url.as_str().to_string(),
    };
    {
        let mut downloads = state
            .downloads
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        downloads.push(entry);
        if downloads.len() > 100 {
            let excess = downloads.len() - 100;
            downloads.drain(0..excess);
        }
    }
    persist_downloads(app, &state);
    emit_downloads(app, &state);
}

fn record_download_finished(
    app: &AppHandle<Wry>,
    url: &Url,
    path: Option<&PathBuf>,
    success: bool,
) {
    let state = app.state::<AppState>();
    let changed = {
        let mut downloads = state
            .downloads
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = downloads
            .iter_mut()
            .rev()
            .find(|entry| entry.status == "downloading" && entry.source_url == url.as_str())
        {
            if let Some(path) = path {
                entry.path = path.to_string_lossy().into_owned();
                entry.file_name = download_file_name(url, path);
            }
            entry.status = if success { "completed" } else { "failed" }.into();
            true
        } else {
            false
        }
    };
    if changed {
        persist_downloads(app, &state);
        emit_downloads(app, &state);
    }
}

fn emit_downloads(app: &AppHandle<Wry>, state: &AppState) {
    let downloads = state
        .downloads
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let _ = app.emit("veloce-downloads-changed", downloads);
}

#[tauri::command]
fn get_downloads(state: State<'_, AppState>) -> Vec<DownloadEntry> {
    state
        .downloads
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

#[tauri::command]
async fn open_downloaded_file(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let path = {
        let downloads = state
            .downloads
            .lock()
            .map_err(|error| format!("Downloadliste ist gesperrt: {error}"))?;
        let entry = downloads
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| "Dieser Download ist nicht mehr in der Liste.".to_string())?;
        if entry.status != "completed" {
            return Err(
                "Die Datei kann erst nach abgeschlossenem Download geöffnet werden.".into(),
            );
        }
        PathBuf::from(&entry.path)
    };

    run_blocking(move || open_downloaded_path(path)).await
}

fn open_downloaded_path(path: PathBuf) -> Result<(), String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("Die heruntergeladene Datei wurde nicht gefunden: {error}"))?;
    if !path.is_file() {
        return Err("Der Downloadpfad verweist nicht auf eine vorhandene Datei.".into());
    }

    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");

    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    return Err(
        "Das Öffnen heruntergeladener Dateien wird auf diesem System nicht unterstützt.".into(),
    );

    #[cfg(any(target_os = "windows", target_os = "macos", unix))]
    {
        command.arg(&path).spawn().map(|_| ()).map_err(|error| {
            format!("Datei konnte nicht mit der System-App geöffnet werden: {error}")
        })
    }
}

#[tauri::command]
fn clear_download_history(app: AppHandle<Wry>, state: State<'_, AppState>) -> Result<(), String> {
    let state = state.inner().clone();
    state
        .downloads
        .lock()
        .map_err(|error| format!("Downloadliste ist gesperrt: {error}"))?
        .clear();
    persist_downloads(&app, &state);
    emit_downloads(&app, &state);
    Ok(())
}

fn new_tab(url: String) -> BrowserTab {
    let title = if url == START_URL {
        "Neuer Tab".to_string()
    } else {
        Url::parse(&url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_string))
            .unwrap_or_else(|| "Lädt …".into())
    };
    BrowserTab {
        id: format!("tab-{}", uuid::Uuid::new_v4().simple()),
        url,
        title,
        pinned: false,
        sleeping: false,
        suspended: false,
        zoom: 1.0,
    }
}

fn normalize_input(input: &str, engine: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() || input == START_URL {
        return Ok(START_URL.into());
    }
    if let Ok(url) = Url::parse(input) {
        if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
            return Ok(url.to_string());
        }
    }
    if !input.contains(char::is_whitespace) && input.contains('.') {
        if let Ok(url) = Url::parse(&format!("https://{input}")) {
            if url.host_str().is_some() {
                return Ok(url.to_string());
            }
        }
    }
    let base = match engine {
        "google" => "https://www.google.com/search?q=",
        "bing" => "https://www.bing.com/search?q=",
        "brave" => "https://search.brave.com/search?q=",
        _ => "https://duckduckgo.com/?q=",
    };
    let query = url::form_urlencoded::byte_serialize(input.as_bytes()).collect::<String>();
    Ok(format!("{base}{query}"))
}

fn default_shortcuts() -> Vec<QuickShortcut> {
    vec![
        QuickShortcut {
            id: "youtube".into(),
            label: "YouTube".into(),
            url: "https://www.youtube.com/".into(),
        },
        QuickShortcut {
            id: "brave-search".into(),
            label: "Brave Search".into(),
            url: "https://search.brave.com/".into(),
        },
        QuickShortcut {
            id: "wikipedia".into(),
            label: "Wikipedia".into(),
            url: "https://www.wikipedia.org/".into(),
        },
        QuickShortcut {
            id: "github".into(),
            label: "GitHub".into(),
            url: "https://github.com/".into(),
        },
    ]
}

fn validate_web_url(raw: &str, what: &str) -> Result<String, String> {
    if raw.len() > 2048 {
        return Err(format!("Die Adresse für {what} ist zu lang."));
    }
    let parsed = Url::parse(raw)
        .map_err(|_| format!("Bitte eine gültige HTTPS- oder HTTP-Adresse für {what} eingeben."))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(format!(
            "{what} benötigt eine HTTP- oder HTTPS-Adresse ohne eingebettete Zugangsdaten."
        ));
    }
    Ok(parsed.to_string())
}

fn validate_shortcuts(shortcuts: Vec<QuickShortcut>) -> Result<Vec<QuickShortcut>, String> {
    if shortcuts.len() > 16 {
        return Err("Es können höchstens 16 Schnellzugriffe gespeichert werden.".into());
    }
    let mut ids = HashSet::new();
    let mut result = Vec::with_capacity(shortcuts.len());
    for mut shortcut in shortcuts {
        shortcut.id = shortcut.id.trim().to_string();
        shortcut.label = shortcut.label.trim().to_string();
        if shortcut.id.is_empty() || shortcut.id.len() > 64 || !ids.insert(shortcut.id.clone()) {
            return Err(
                "Die Schnellzugriffe enthalten eine ungültige oder doppelte Kennung.".into(),
            );
        }
        if shortcut.label.is_empty() || shortcut.label.chars().count() > 30 {
            return Err("Namen für Schnellzugriffe müssen 1 bis 30 Zeichen lang sein.".into());
        }
        shortcut.url = validate_web_url(&shortcut.url, "den Schnellzugriff")?;
        result.push(shortcut);
    }
    Ok(result)
}

fn playback_key(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let is_youtube = host == "youtube.com" || host.ends_with(".youtube.com");
    if is_youtube {
        if url.path() == "/watch" {
            if let Some(video_id) = url.query_pairs().find_map(|(key, value)| {
                (key == "v" && !value.is_empty()).then(|| value.into_owned())
            }) {
                return format!("youtube:{video_id}");
            }
        }
        if let Some(video_id) = url
            .path_segments()
            .and_then(|mut parts| (parts.next() == Some("shorts")).then(|| parts.next()))
            .flatten()
            .filter(|id| !id.is_empty())
        {
            return format!("youtube:{video_id}");
        }
    }
    let mut stable = url.clone();
    stable.set_fragment(None);
    stable.to_string()
}

fn playback_checkpoints(app: &AppHandle<Wry>) -> Result<Vec<PlaybackCheckpoint>, String> {
    let path = app_dir(app)?.join("playback.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_json::<Vec<PlaybackCheckpoint>>(&path)
        .ok_or_else(|| "Der lokale Videofortschritt konnte nicht gelesen werden.".into())
}

fn get_playback_checkpoint(
    app: &AppHandle<Wry>,
    url: &Url,
) -> Result<Option<PlaybackCheckpoint>, String> {
    let key = playback_key(url);
    Ok(playback_checkpoints(app)?
        .into_iter()
        .find(|checkpoint| checkpoint.key == key))
}

fn save_playback_checkpoint(
    app: &AppHandle<Wry>,
    url: &Url,
    position_seconds: f64,
    duration_seconds: f64,
    was_playing: bool,
) -> Result<(), String> {
    if !position_seconds.is_finite()
        || !duration_seconds.is_finite()
        || position_seconds < 0.0
        || duration_seconds < 1.0
        || duration_seconds > 7.0 * 24.0 * 60.0 * 60.0
    {
        return Ok(());
    }
    let key = playback_key(url);
    let path = app_dir(app)?.join("playback.json");
    let mut checkpoints = if path.exists() {
        read_json::<Vec<PlaybackCheckpoint>>(&path)
            .ok_or_else(|| "Der lokale Videofortschritt konnte nicht gelesen werden.".to_string())?
    } else {
        Vec::new()
    };
    if position_seconds >= duration_seconds - 1.0 {
        checkpoints.retain(|item| item.key != key);
    } else {
        checkpoints.retain(|item| item.key != key);
        checkpoints.push(PlaybackCheckpoint {
            key,
            url: url.to_string(),
            position_seconds: position_seconds.min(duration_seconds),
            duration_seconds,
            was_playing,
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
        checkpoints.sort_by_key(|item| item.updated_at);
        if checkpoints.len() > 100 {
            checkpoints.drain(..checkpoints.len() - 100);
        }
    }
    write_json(&path, &checkpoints)
}

fn clear_playback_checkpoint(app: &AppHandle<Wry>, url: &Url) -> Result<(), String> {
    let path = app_dir(app)?.join("playback.json");
    if !path.exists() {
        return Ok(());
    }
    let mut checkpoints = read_json::<Vec<PlaybackCheckpoint>>(&path)
        .ok_or_else(|| "Der lokale Videofortschritt konnte nicht gelesen werden.".to_string())?;
    let original_count = checkpoints.len();
    let key = playback_key(url);
    checkpoints.retain(|item| item.key != key);
    if checkpoints.len() != original_count {
        write_json(&path, &checkpoints)?;
    }
    Ok(())
}

fn is_tracking_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    TRACKING_DOMAINS
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

fn tabs_snapshot(state: &AppState) -> TabsSnapshot {
    TabsSnapshot {
        tabs: state
            .tabs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone(),
        active_id: state
            .active_id
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone(),
    }
}

fn emit_tabs(app: &AppHandle<Wry>, state: &AppState) {
    let _ = app.emit("veloce-tabs-changed", tabs_snapshot(state));
}

fn get_tab(state: &AppState, id: &str) -> Result<BrowserTab, String> {
    state
        .tabs
        .lock()
        .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?
        .iter()
        .find(|tab| tab.id == id)
        .cloned()
        .ok_or_else(|| "Der Tab existiert nicht mehr.".into())
}

fn extension_root(app: &AppHandle<Wry>) -> Result<PathBuf, String> {
    let is_complete = |root: &PathBuf| {
        ["uBlockOriginLite", "SponsorBlock"]
            .iter()
            .all(|name| root.join(name).join("manifest.json").is_file())
    };
    if let Ok(resources) = app.path().resource_dir() {
        let packaged = resources.join("extensions");
        if is_complete(&packaged) {
            return Ok(packaged);
        }
    }
    let development = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("extensions");
    if is_complete(&development) {
        return Ok(development);
    }
    Err(
        "Die mitgelieferten Erweiterungsdateien fehlen oder enthalten kein gültiges Manifest."
            .into(),
    )
}

fn publish_extension_status(
    app: &AppHandle<Wry>,
    state: &AppState,
    statuses: Vec<ExtensionStatus>,
) {
    *state
        .extension_status
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = statuses.clone();
    let _ = app.emit("veloce-extension-status", statuses);
}

fn finish_extension_check(
    index: usize,
    status: ExtensionStatus,
    statuses: &Arc<Mutex<Vec<ExtensionStatus>>>,
    completed: &Arc<AtomicUsize>,
    sender: &mpsc::Sender<Result<Vec<ExtensionStatus>, String>>,
) {
    if let Ok(mut current) = statuses.lock() {
        current[index] = status;
    }
    if completed.fetch_add(1, Ordering::AcqRel) + 1 == 2 {
        let snapshot = statuses
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|error| error.into_inner().clone());
        let _ = sender.send(Ok(snapshot));
    }
}

#[cfg(windows)]
fn read_extension_name_and_enabled(
    extension: &ICoreWebView2BrowserExtension,
) -> Result<(String, String, bool), String> {
    let mut raw_id = PWSTR::null();
    unsafe { extension.Id(&mut raw_id) }
        .map_err(|error| format!("Erweiterungs-ID konnte nicht gelesen werden: {error}"))?;
    let id = CoTaskMemPWSTR::from(raw_id).to_string();
    let mut raw_name = PWSTR::null();
    unsafe { extension.Name(&mut raw_name) }
        .map_err(|error| format!("Erweiterungsname konnte nicht gelesen werden: {error}"))?;
    let name = CoTaskMemPWSTR::from(raw_name).to_string();
    let mut enabled = windows::core::BOOL(0);
    unsafe { extension.IsEnabled(&mut enabled) }
        .map_err(|error| format!("Erweiterungsstatus konnte nicht gelesen werden: {error}"))?;
    Ok((id, name, enabled.as_bool()))
}

#[cfg(windows)]
fn extension_index(name: &str) -> Option<usize> {
    let lowered = name.to_ascii_lowercase();
    if lowered.starts_with("ublock origin lite") || lowered.starts_with("ublock lite") {
        Some(0)
    } else if lowered.starts_with("sponsorblock") {
        Some(1)
    } else {
        None
    }
}

#[cfg(windows)]
fn extension_status_from_native(
    extension: &ICoreWebView2BrowserExtension,
) -> Result<ExtensionStatus, String> {
    let (id, name, enabled) = read_extension_name_and_enabled(extension)?;
    if let Some(index) = extension_index(&name) {
        return Ok(ExtensionStatus {
            id,
            installed: true,
            enabled,
            detail: if enabled {
                "Von WebView2 gefunden und aktiviert.".into()
            } else {
                "Installiert, aber in WebView2 deaktiviert.".into()
            },
            ..pending_extensions()[index].clone()
        });
    }
    Ok(ExtensionStatus {
        key: format!("custom:{id}"),
        id,
        name,
        bundled_version: String::new(),
        installed: true,
        enabled,
        removable: true,
        detail: "Von WebView2 im aktuellen Profil installiert.".into(),
    })
}

fn string_array_field(manifest: &serde_json::Value, field: &str) -> Vec<String> {
    manifest
        .get(field)
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(serde_json::Value::as_str)
                .take(64)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn append_manifest_path(paths: &mut Vec<String>, value: Option<&serde_json::Value>) {
    if let Some(value) = value.and_then(serde_json::Value::as_str) {
        paths.push(value.to_owned());
    }
}

fn manifest_resource_paths(manifest: &serde_json::Value) -> Vec<String> {
    let mut paths = Vec::new();
    if let Some(background) = manifest.get("background") {
        append_manifest_path(&mut paths, background.get("service_worker"));
        if let Some(scripts) = background
            .get("scripts")
            .and_then(serde_json::Value::as_array)
        {
            paths.extend(
                scripts
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned),
            );
        }
    }
    if let Some(scripts) = manifest
        .get("content_scripts")
        .and_then(serde_json::Value::as_array)
    {
        for script in scripts {
            paths.extend(string_array_field(script, "js"));
            paths.extend(string_array_field(script, "css"));
        }
    }
    for key in ["options_page", "devtools_page"] {
        append_manifest_path(&mut paths, manifest.get(key));
    }
    for key in [
        "action",
        "browser_action",
        "page_action",
        "options_ui",
        "side_panel",
    ] {
        if let Some(section) = manifest.get(key) {
            append_manifest_path(&mut paths, section.get("default_popup"));
            append_manifest_path(&mut paths, section.get("page"));
            append_manifest_path(&mut paths, section.get("default_path"));
            if let Some(icons) = section
                .get("default_icon")
                .and_then(serde_json::Value::as_object)
            {
                paths.extend(
                    icons
                        .values()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned),
                );
            }
        }
    }
    if let Some(icons) = manifest.get("icons").and_then(serde_json::Value::as_object) {
        paths.extend(
            icons
                .values()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned),
        );
    }
    if let Some(overrides) = manifest
        .get("chrome_url_overrides")
        .and_then(serde_json::Value::as_object)
    {
        paths.extend(
            overrides
                .values()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned),
        );
    }
    if let Some(rules) = manifest
        .get("declarative_net_request")
        .and_then(|value| value.get("rule_resources"))
        .and_then(serde_json::Value::as_array)
    {
        for resource in rules {
            append_manifest_path(&mut paths, resource.get("path"));
        }
    }
    if let Some(resources) = manifest
        .get("web_accessible_resources")
        .and_then(serde_json::Value::as_array)
    {
        for resource in resources {
            if let Some(entries) = resource.as_array() {
                paths.extend(
                    entries
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned),
                );
            } else {
                paths.extend(string_array_field(resource, "resources"));
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn validate_manifest_resources(
    root: &std::path::Path,
    manifest: &serde_json::Value,
) -> Result<(), String> {
    for relative in manifest_resource_paths(manifest) {
        // Wildcards in web_accessible_resources describe patterns and cannot be
        // checked as a single file. All concrete referenced resources are checked.
        if relative.contains('*') {
            continue;
        }
        let relative_path = PathBuf::from(&relative);
        if relative_path.is_absolute()
            || relative_path.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(format!(
                "Die Erweiterung referenziert einen ungültigen Dateipfad: {relative}"
            ));
        }
        let resolved = root
            .join(&relative_path)
            .canonicalize()
            .map_err(|_| format!("Eine von manifest.json referenzierte Datei fehlt: {relative}"))?;
        if !resolved.starts_with(root) || !resolved.is_file() {
            return Err(format!(
                "Eine von manifest.json referenzierte Datei ist ungültig: {relative}"
            ));
        }
    }
    Ok(())
}

fn inspect_extension_folder(path: &str) -> Result<ExtensionManifestPreview, String> {
    let folder = PathBuf::from(path.trim())
        .canonicalize()
        .map_err(|error| format!("Erweiterungsordner ist nicht erreichbar: {error}"))?;
    if !folder.is_dir() {
        return Err("Bitte den entpackten Erweiterungsordner auswählen.".into());
    }
    let manifest_path = folder.join("manifest.json");
    let metadata = fs::metadata(&manifest_path)
        .map_err(|_| "Im ausgewählten Ordner fehlt manifest.json.".to_string())?;
    if metadata.len() > 1024 * 1024 {
        return Err("Das Erweiterungsmanifest ist ungewöhnlich groß und wurde abgelehnt.".into());
    }
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("manifest.json konnte nicht gelesen werden: {error}"))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .map_err(|error| format!("manifest.json ist ungültig: {error}"))?;
    validate_manifest_resources(&folder, &manifest)?;
    let manifest_version = manifest
        .get("manifest_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or("manifest_version fehlt oder ist ungültig.")?;
    if ![2, 3].contains(&manifest_version) {
        return Err(
            "WebView2 unterstützt dieses Manifestformat nicht (nur Version 2 oder 3).".into(),
        );
    }
    let version = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("Die Erweiterung hat keine gültige Versionsangabe.")?;
    let name = manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.starts_with("__MSG_") && !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            folder
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("Unbenannte Erweiterung")
                .to_owned()
        });
    let mut permissions = string_array_field(&manifest, "permissions");
    permissions.extend(string_array_field(&manifest, "optional_permissions"));
    permissions.sort();
    permissions.dedup();
    let mut host_permissions = string_array_field(&manifest, "host_permissions");
    if let Some(scripts) = manifest
        .get("content_scripts")
        .and_then(serde_json::Value::as_array)
    {
        for script in scripts.iter().take(64) {
            host_permissions.extend(string_array_field(script, "matches"));
        }
    }
    host_permissions.sort();
    host_permissions.dedup();
    Ok(ExtensionManifestPreview {
        path: folder.to_string_lossy().into_owned(),
        name,
        version: version.to_owned(),
        manifest_version: manifest_version as u8,
        permissions,
        host_permissions,
    })
}

#[tauri::command]
fn inspect_extension_manifest(path: String) -> Result<ExtensionManifestPreview, String> {
    inspect_extension_folder(&path)
}

#[cfg(windows)]
fn add_missing_extensions_to_profile(
    profile: ICoreWebView2Profile7,
    root: PathBuf,
    sender: mpsc::Sender<Result<Vec<ExtensionStatus>, String>>,
) -> windows::core::Result<()> {
    let (statuses, completed) = (
        Arc::new(Mutex::new(pending_extensions())),
        Arc::new(AtomicUsize::new(0)),
    );
    let mut found = [false, false];
    let get_tx = sender.clone();
    let callback_profile = profile.clone();
    let list_handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
        move |result, list| {
            if let Err(error) = result {
                let _ = get_tx.send(Err(format!(
                    "WebView2 konnte Erweiterungen nicht abfragen: {error}"
                )));
                return Ok(());
            }
            let Some(list) = list else {
                let _ = get_tx.send(Err("WebView2 lieferte keine Erweiterungsliste.".into()));
                return Ok(());
            };
            let mut count = 0;
            if let Err(error) = unsafe { list.Count(&mut count) } {
                let _ = get_tx.send(Err(format!(
                    "WebView2-Erweiterungsliste konnte nicht gelesen werden: {error}"
                )));
                return Ok(());
            }
            for item in 0..count {
                let extension = match unsafe { list.GetValueAtIndex(item) } {
                    Ok(extension) => extension,
                    Err(_) => continue,
                };
                let Ok((id, name, enabled)) = read_extension_name_and_enabled(&extension) else {
                    continue;
                };
                if let Some(index) = extension_index(&name) {
                    found[index] = true;
                    finish_extension_check(
                        index,
                        ExtensionStatus {
                            id,
                            installed: true,
                            enabled,
                            removable: false,
                            detail: if enabled {
                                "Von WebView2 gefunden und aktiviert.".into()
                            } else {
                                "Installiert, aber in WebView2 deaktiviert.".into()
                            },
                            ..pending_extensions()[index].clone()
                        },
                        &statuses,
                        &completed,
                        &get_tx,
                    );
                }
            }

            for index in 0..2 {
                if found[index] {
                    continue;
                }
                let folder = if index == 0 {
                    "uBlockOriginLite"
                } else {
                    "SponsorBlock"
                };
                let extension_path = root.join(folder);
                let path_string = HSTRING::from(extension_path.as_path());
                let callback_statuses = statuses.clone();
                let callback_completed = completed.clone();
                let callback_sender = get_tx.clone();
                let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
                    move |result, extension| {
                        let status = match result {
                            Err(error) => ExtensionStatus {
                                detail: format!("WebView2 hat die Installation abgelehnt: {error}"),
                                ..pending_extensions()[index].clone()
                            },
                            Ok(()) => match extension {
                                Some(extension) => {
                                    match read_extension_name_and_enabled(&extension) {
                                        Ok((id, name, enabled)) => {
                                            ExtensionStatus {
                                                id,
                                                installed: true,
                                                enabled,
                                                removable: false,
                                                detail: if enabled {
                                                    format!("Installation von WebView2 bestätigt ({name}).")
                                                } else {
                                                    format!("Installiert, aber WebView2 meldet deaktiviert ({name}).")
                                                },
                                                ..pending_extensions()[index].clone()
                                            }
                                        }
                                        Err(error) => ExtensionStatus {
                                            detail: error,
                                            ..pending_extensions()[index].clone()
                                        },
                                    }
                                }
                                None => ExtensionStatus {
                                    detail: "WebView2 hat die Installation nicht bestätigt.".into(),
                                    ..pending_extensions()[index].clone()
                                },
                            },
                        };
                        finish_extension_check(
                            index,
                            status,
                            &callback_statuses,
                            &callback_completed,
                            &callback_sender,
                        );
                        Ok(())
                    },
                ));
                if let Err(error) =
                    unsafe { callback_profile.AddBrowserExtension(&path_string, &handler) }
                {
                    finish_extension_check(
                        index,
                        ExtensionStatus {
                            detail: format!("Erweiterung konnte nicht hinzugefügt werden: {error}"),
                            ..pending_extensions()[index].clone()
                        },
                        &statuses,
                        &completed,
                        &get_tx,
                    );
                }
            }
            Ok(())
        },
    ));
    unsafe { profile.GetBrowserExtensions(&list_handler) }
}

#[cfg(windows)]
fn install_bundled_extensions(app: &AppHandle<Wry>) -> Result<Vec<ExtensionStatus>, String> {
    let root = extension_root(app)?;
    let shell = app
        .get_webview("main")
        .ok_or("Die WebView2-Oberfläche ist noch nicht bereit.")?;
    let (sender, receiver) = mpsc::channel::<Result<Vec<ExtensionStatus>, String>>();
    let callback_sender = sender.clone();
    shell
        .with_webview(move |platform| {
            let result = (|| -> windows::core::Result<()> {
                let controller = platform.controller();
                let core = unsafe { controller.CoreWebView2()? };
                let core13 = core.cast::<ICoreWebView2_13>()?;
                let profile = unsafe { core13.Profile()? }.cast::<ICoreWebView2Profile7>()?;
                add_missing_extensions_to_profile(profile, root, callback_sender.clone())
            })();
            if let Err(error) = result {
                let _ = callback_sender.send(Err(format!(
                    "Browsererweiterungen werden von diesem WebView2-Runtime nicht unterstützt oder konnten nicht gestartet werden: {error}"
                )));
            }
        })
        .map_err(|error| format!("WebView2-Erweiterungsschnittstelle konnte nicht geöffnet werden: {error}"))?;
    receiver
        .recv_timeout(Duration::from_secs(20))
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => {
                "WebView2 hat die Erweiterungsinstallation nicht rechtzeitig bestätigt.".to_string()
            }
            mpsc::RecvTimeoutError::Disconnected => {
                "Die Erweiterungsprüfung wurde unerwartet beendet.".to_string()
            }
        })?
}

#[cfg(windows)]
fn get_extensions_blocking(app: AppHandle<Wry>) -> Result<Vec<ExtensionStatus>, String> {
    let shell = app
        .get_webview("main")
        .ok_or("Die WebView2-Oberfläche ist noch nicht bereit.")?;
    let (sender, receiver) = mpsc::channel::<Result<Vec<ExtensionStatus>, String>>();
    shell
        .with_webview(move |platform| {
            let setup = (|| -> windows::core::Result<()> {
                let core = unsafe { platform.controller().CoreWebView2()? };
                let core13 = core.cast::<ICoreWebView2_13>()?;
                let profile = unsafe { core13.Profile()? }.cast::<ICoreWebView2Profile7>()?;
                let callback_sender = sender.clone();
                let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
                    move |result, list| {
                        let result = (|| -> Result<Vec<ExtensionStatus>, String> {
                            result.map_err(|error| {
                                format!("Erweiterungsliste fehlgeschlagen: {error}")
                            })?;
                            let list = list.ok_or("WebView2 lieferte keine Erweiterungsliste.")?;
                            let mut count = 0;
                            unsafe { list.Count(&mut count) }.map_err(|error| {
                                format!("Erweiterungsanzahl konnte nicht gelesen werden: {error}")
                            })?;
                            let mut entries = Vec::with_capacity(count as usize);
                            for index in 0..count {
                                let extension =
                                    unsafe { list.GetValueAtIndex(index) }.map_err(|error| {
                                        format!("Erweiterung konnte nicht gelesen werden: {error}")
                                    })?;
                                entries.push(extension_status_from_native(&extension)?);
                            }
                            for bundled in pending_extensions() {
                                if !entries.iter().any(|entry| entry.key == bundled.key) {
                                    entries.push(ExtensionStatus {
                                        detail: "Im aktuellen WebView2-Profil nicht installiert."
                                            .into(),
                                        ..bundled
                                    });
                                }
                            }
                            entries.sort_by(|left, right| {
                                left.name.to_lowercase().cmp(&right.name.to_lowercase())
                            });
                            Ok(entries)
                        })();
                        let _ = callback_sender.send(result);
                        Ok(())
                    },
                ));
                unsafe { profile.GetBrowserExtensions(&handler) }
            })();
            if let Err(error) = setup {
                let _ = sender.send(Err(format!(
                    "WebView2-Erweiterungen konnten nicht abgefragt werden: {error}"
                )));
            }
        })
        .map_err(|error| {
            format!("WebView2-Erweiterungsansicht konnte nicht geöffnet werden: {error}")
        })?;
    receiver
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "Zeitüberschreitung beim Abfragen installierter Erweiterungen.".to_string())?
}

#[cfg(windows)]
fn set_extension_enabled_blocking(
    app: AppHandle<Wry>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let shell = app
        .get_webview("main")
        .ok_or("Die WebView2-Oberfläche ist noch nicht bereit.")?;
    let (sender, receiver) = mpsc::channel::<Result<(), String>>();
    shell
        .with_webview(move |platform| {
            let setup = (|| -> windows::core::Result<()> {
                let core = unsafe { platform.controller().CoreWebView2()? };
                let core13 = core.cast::<ICoreWebView2_13>()?;
                let profile = unsafe { core13.Profile()? }.cast::<ICoreWebView2Profile7>()?;
                let callback_sender = sender.clone();
                let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
                    move |result, list| {
                        let setup = (|| -> windows::core::Result<()> {
                            result.map_err(|error| error)?;
                            let list = list.ok_or_else(|| windows::core::Error::from_hresult(windows::core::HRESULT(0x80004003_u32 as i32)))?;
                            let mut count = 0;
                            unsafe { list.Count(&mut count)? };
                            for index in 0..count {
                                let extension = unsafe { list.GetValueAtIndex(index)? };
                                let mut raw_id = PWSTR::null();
                                unsafe { extension.Id(&mut raw_id)? };
                                let extension_id = CoTaskMemPWSTR::from(raw_id).to_string();
                                if extension_id != id { continue; }
                                let op_sender = callback_sender.clone();
                                let op = BrowserExtensionEnableCompletedHandler::create(Box::new(move |result| {
                                    let result = result.map_err(|error| format!("Erweiterung konnte nicht umgeschaltet werden: {error}"));
                                    let _ = op_sender.send(result);
                                    Ok(())
                                }));
                                unsafe { extension.Enable(enabled, &op)? };
                                return Ok(());
                            }
                            let _ = callback_sender.send(Err("Diese Erweiterung ist im Veloce-Profil nicht installiert.".into()));
                            Ok(())
                        })();
                        if let Err(error) = setup {
                            let _ = callback_sender.send(Err(format!("Erweiterungsstatus konnte nicht geändert werden: {error}")));
                        }
                        Ok(())
                    },
                ));
                unsafe { profile.GetBrowserExtensions(&handler) }
            })();
            if let Err(error) = setup {
                let _ = sender.send(Err(format!("Erweiterungsstatus konnte nicht geändert werden: {error}")));
            }
        })
        .map_err(|error| format!("WebView2-Erweiterungsansicht konnte nicht geöffnet werden: {error}"))?;
    receiver
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "Zeitüberschreitung beim Ändern der Erweiterung.".to_string())?
}

#[cfg(windows)]
fn remove_extension_blocking(app: AppHandle<Wry>, id: String) -> Result<(), String> {
    let shell = app
        .get_webview("main")
        .ok_or("Die WebView2-Oberfläche ist noch nicht bereit.")?;
    let (sender, receiver) = mpsc::channel::<Result<(), String>>();
    shell
        .with_webview(move |platform| {
            let setup = (|| -> windows::core::Result<()> {
                let core = unsafe { platform.controller().CoreWebView2()? };
                let core13 = core.cast::<ICoreWebView2_13>()?;
                let profile = unsafe { core13.Profile()? }.cast::<ICoreWebView2Profile7>()?;
                let callback_sender = sender.clone();
                let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
                    move |result, list| {
                        let setup = (|| -> windows::core::Result<()> {
                            result.map_err(|error| error)?;
                            let list = list.ok_or_else(|| windows::core::Error::from_hresult(windows::core::HRESULT(0x80004003_u32 as i32)))?;
                            let mut count = 0;
                            unsafe { list.Count(&mut count)? };
                            for index in 0..count {
                                let extension = unsafe { list.GetValueAtIndex(index)? };
                                let mut raw_id = PWSTR::null();
                                unsafe { extension.Id(&mut raw_id)? };
                                let extension_id = CoTaskMemPWSTR::from(raw_id).to_string();
                                if extension_id != id { continue; }
                                let mut raw_name = PWSTR::null();
                                unsafe { extension.Name(&mut raw_name)? };
                                let extension_name = CoTaskMemPWSTR::from(raw_name).to_string();
                                if extension_index(&extension_name).is_some() {
                                    let _ = callback_sender.send(Err("Mitgelieferte Erweiterungen können deaktiviert, aber nicht entfernt werden.".into()));
                                    return Ok(());
                                }
                                let op_sender = callback_sender.clone();
                                let op = BrowserExtensionRemoveCompletedHandler::create(Box::new(move |result| {
                                    let result = result.map_err(|error| format!("Erweiterung konnte nicht entfernt werden: {error}"));
                                    let _ = op_sender.send(result);
                                    Ok(())
                                }));
                                unsafe { extension.Remove(&op)? };
                                return Ok(());
                            }
                            let _ = callback_sender.send(Err("Diese Erweiterung ist im Veloce-Profil nicht installiert.".into()));
                            Ok(())
                        })();
                        if let Err(error) = setup {
                            let _ = callback_sender.send(Err(format!("Erweiterung konnte nicht entfernt werden: {error}")));
                        }
                        Ok(())
                    },
                ));
                unsafe { profile.GetBrowserExtensions(&handler) }
            })();
            if let Err(error) = setup {
                let _ = sender.send(Err(format!("Erweiterung konnte nicht entfernt werden: {error}")));
            }
        })
        .map_err(|error| format!("WebView2-Erweiterungsansicht konnte nicht geöffnet werden: {error}"))?;
    receiver
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "Zeitüberschreitung beim Entfernen der Erweiterung.".to_string())?
}

#[cfg(windows)]
fn add_custom_extension_blocking(
    app: AppHandle<Wry>,
    path: String,
    expected_name: String,
    expected_version: String,
) -> Result<ExtensionStatus, String> {
    let preview = inspect_extension_folder(&path)?;
    if preview.name != expected_name || preview.version != expected_version {
        return Err(
            "Das Erweiterungsmanifest hat sich seit der Prüfung geändert. Bitte erneut prüfen."
                .into(),
        );
    }
    let shell = app
        .get_webview("main")
        .ok_or("Die WebView2-Oberfläche ist noch nicht bereit.")?;
    let path = PathBuf::from(preview.path);
    let (sender, receiver) = mpsc::channel::<Result<ExtensionStatus, String>>();
    shell
        .with_webview(move |platform| {
            let setup = (|| -> windows::core::Result<()> {
                let core = unsafe { platform.controller().CoreWebView2()? };
                let core13 = core.cast::<ICoreWebView2_13>()?;
                let profile = unsafe { core13.Profile()? }.cast::<ICoreWebView2Profile7>()?;
                let path_string = HSTRING::from(path.as_path());
                let callback_sender = sender.clone();
                let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
                    move |result, extension| {
                        let status = match result {
                            Err(error) => {
                                Err(format!("WebView2 hat die Erweiterung abgelehnt: {error}"))
                            }
                            Ok(()) => match extension {
                                Some(extension) => extension_status_from_native(&extension),
                                None => {
                                    Err("WebView2 hat die Installation nicht bestätigt.".into())
                                }
                            },
                        };
                        let _ = callback_sender.send(status);
                        Ok(())
                    },
                ));
                unsafe { profile.AddBrowserExtension(&path_string, &handler) }
            })();
            if let Err(error) = setup {
                let _ = sender.send(Err(format!(
                    "Erweiterung konnte nicht hinzugefügt werden: {error}"
                )));
            }
        })
        .map_err(|error| {
            format!("WebView2-Erweiterungsansicht konnte nicht geöffnet werden: {error}")
        })?;
    receiver
        .recv_timeout(Duration::from_secs(20))
        .map_err(|_| {
            "WebView2 hat die Erweiterungsinstallation nicht rechtzeitig bestätigt.".to_string()
        })?
}

#[cfg(not(windows))]
fn get_extensions_blocking(_app: AppHandle<Wry>) -> Result<Vec<ExtensionStatus>, String> {
    Err("WebView2-Erweiterungen werden derzeit nur im Windows-Build unterstützt.".into())
}

#[cfg(not(windows))]
fn set_extension_enabled_blocking(
    _app: AppHandle<Wry>,
    _id: String,
    _enabled: bool,
) -> Result<(), String> {
    Err("WebView2-Erweiterungen werden derzeit nur im Windows-Build unterstützt.".into())
}

#[cfg(not(windows))]
fn remove_extension_blocking(_app: AppHandle<Wry>, _id: String) -> Result<(), String> {
    Err("WebView2-Erweiterungen werden derzeit nur im Windows-Build unterstützt.".into())
}

#[cfg(not(windows))]
fn add_custom_extension_blocking(
    _app: AppHandle<Wry>,
    _path: String,
    _expected_name: String,
    _expected_version: String,
) -> Result<ExtensionStatus, String> {
    Err("WebView2-Erweiterungen werden derzeit nur im Windows-Build unterstützt.".into())
}

#[cfg(not(windows))]
fn install_bundled_extensions(_app: &AppHandle<Wry>) -> Result<Vec<ExtensionStatus>, String> {
    Err("uBlock Origin Lite und SponsorBlock werden in diesem Build nur über die Windows-WebView2-Erweiterungs-API unterstützt.".into())
}

fn ensure_extensions(app: &AppHandle<Wry>, state: &AppState) {
    let already_attempted = {
        let mut attempted = state
            .extension_attempted
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *attempted {
            true
        } else {
            *attempted = true;
            false
        }
    };
    if already_attempted {
        return;
    }
    match install_bundled_extensions(app) {
        Ok(statuses) => publish_extension_status(app, state, statuses),
        Err(error) => {
            let statuses = pending_extensions()
                .into_iter()
                .map(|mut status| {
                    status.detail = error.clone();
                    status
                })
                .collect();
            publish_extension_status(app, state, statuses);
        }
    }
}

fn youtube_fill_script(enabled: bool) -> String {
    if enabled {
        format!(
            "(() => {{ if (!/(^|\\.)youtube\\.com$/i.test(location.hostname)) return; const apply = () => {{ const root = document.documentElement; if (!root) return; let style = document.getElementById('{YOUTUBE_FILL_STYLE_ID}'); if (!style) {{ style = document.createElement('style'); style.id = '{YOUTUBE_FILL_STYLE_ID}'; (document.head || root).appendChild(style); }} style.textContent = `{YOUTUBE_FILL_CSS}`; }}; if (document.documentElement) apply(); else document.addEventListener('DOMContentLoaded', apply, {{ once: true }}); }})()"
        )
    } else {
        format!("document.getElementById('{YOUTUBE_FILL_STYLE_ID}')?.remove();")
    }
}

fn youtube_interruption_notice_script(enabled: bool) -> String {
    let enabled = if enabled { "true" } else { "false" };
    let script = r#"(() => {
      if (!/(^|\.)youtube\.com$/i.test(location.hostname)) return;
      const styleId = '__STYLE_ID__';
      const hiddenClass = 'veloce-hidden-adblock-warning';
      let controller = window.__veloceInterruptionNoticeController;
      if (!controller) {
        let popupObserver = null;
        let discoveryObserver = null;
        let enabledNow = false;
        let waitingForDom = false;
        const warningText = (toast) => String(toast && (toast.innerText || toast.textContent) || '').toLocaleLowerCase();
        const isWarning = (toast) => {
          const text = warningText(toast);
          return text.includes('kommt es zu unterbrechungen')
            || text.includes('experiencing interruptions')
            || text.includes('ad blockers')
            || text.includes('werbeblocker');
        };
        const updateToast = (toast) => {
          if (!(toast instanceof Element)) return;
          toast.classList.toggle(hiddenClass, isWarning(toast));
        };
        const collectToast = (node, result) => {
          if (!(node instanceof Element)) return;
          const own = node.matches('tp-yt-paper-toast#toast') ? node : node.closest('tp-yt-paper-toast#toast');
          if (own) result.add(own);
          node.querySelectorAll('tp-yt-paper-toast#toast').forEach((toast) => result.add(toast));
        };
        const processRecords = (records) => {
          const candidates = new Set();
          for (const record of records) {
            collectToast(record.target.nodeType === Node.ELEMENT_NODE ? record.target : record.target.parentElement, candidates);
            for (const added of record.addedNodes) collectToast(added, candidates);
          }
          candidates.forEach(updateToast);
        };
        const watchPopup = () => {
          const popup = document.querySelector('ytd-popup-container');
          if (!popup) return false;
          if (discoveryObserver) { discoveryObserver.disconnect(); discoveryObserver = null; }
          if (!popupObserver) {
            popupObserver = new MutationObserver(processRecords);
            popupObserver.observe(popup, { childList: true, subtree: true, characterData: true });
          }
          popup.querySelectorAll('tp-yt-paper-toast#toast').forEach(updateToast);
          return true;
        };
        const start = () => {
          if (!enabledNow) return;
          const root = document.documentElement;
          if (!root) {
            if (!waitingForDom) {
              waitingForDom = true;
              document.addEventListener('DOMContentLoaded', () => {
                waitingForDom = false;
                start();
              }, { once: true });
            }
            return;
          }
          let style = document.getElementById(styleId);
          if (!style) {
            style = document.createElement('style');
            style.id = styleId;
            (document.head || root).appendChild(style);
          }
          style.textContent = `tp-yt-paper-toast#toast.${hiddenClass}{display:none!important}`;
          if (!watchPopup() && !discoveryObserver) {
            discoveryObserver = new MutationObserver(() => watchPopup());
            discoveryObserver.observe(root, { childList: true, subtree: true });
          }
          root.querySelectorAll('tp-yt-paper-toast#toast').forEach(updateToast);
        };
        controller = (enabled) => {
          if (enabled) {
            enabledNow = true;
            start();
            return;
          }
          enabledNow = false;
          if (popupObserver) { popupObserver.disconnect(); popupObserver = null; }
          if (discoveryObserver) { discoveryObserver.disconnect(); discoveryObserver = null; }
          document.querySelectorAll(`tp-yt-paper-toast#toast.${hiddenClass}`).forEach((toast) => toast.classList.remove(hiddenClass));
          document.getElementById(styleId)?.remove();
        };
        window.__veloceInterruptionNoticeController = controller;
      }
      window.__veloceInterruptionNoticeController(__ENABLED__);
    })()"#;
    script
        .replace("__STYLE_ID__", YOUTUBE_INTERRUPTION_STYLE_ID)
        .replace("__ENABLED__", enabled)
}

const VIDEO_RESUME_SCRIPT: &str = r#"(() => {
  // This runs in the page's WebView2 context. The checkpoint contains only the
  // current page URL and playback position and is stored by the native app.
  window.__veloceResumePlaybackEnabled = __RESUME_ENABLED__;
  if (window.__velocePlaybackCheckpointInstalled) {
    window.__velocePlaybackSettingsChanged?.(window.__veloceResumePlaybackEnabled);
    return;
  }
  window.__velocePlaybackCheckpointInstalled = true;
  let activeVideo = null;
  let activeKey = '';
  let lastWriteAt = 0;
  let lastPlayingSample = false;
  const resumed = new Set();
  const isYouTube = () => /(^|\.)youtube\.com$/i.test(location.hostname);
  const videoKey = () => {
    if (isYouTube()) {
      const match = location.pathname.match(/^\/shorts\/([^/?#]+)/);
      const id = new URL(location.href).searchParams.get('v') || (match && match[1]);
      if (id) return `youtube:${id}`;
    }
    const url = new URL(location.href);
    url.hash = '';
    return url.href;
  };
  const post = (message) => {
    try {
      const bridge = window.chrome && window.chrome.webview;
      if (bridge && typeof bridge.postMessage === 'function') bridge.postMessage(message);
    } catch (_) {}
  };
  const selectVideo = () => {
    if (isYouTube()) {
      if (document.querySelector('#movie_player.ad-showing')) return null;
      const player = document.querySelector('#movie_player video');
      if (player && player.isConnected) return player;
    }
    const candidates = Array.from(document.querySelectorAll('video')).filter((video) => {
      const rect = video.getBoundingClientRect();
      return video.isConnected && rect.width >= 160 && rect.height >= 90;
    });
    return candidates.sort((a, b) =>
      (b.clientWidth * b.clientHeight) - (a.clientWidth * a.clientHeight)
    )[0] || null;
  };
  const sendPosition = (video, force = false, playingOverride) => {
    if (!window.__veloceResumePlaybackEnabled) return;
    if (!video || !Number.isFinite(video.duration) || video.duration < 1 || !Number.isFinite(video.currentTime)) return;
    const now = Date.now();
    if (!force && now - lastWriteAt < 5000) return;
    const key = videoKey();
    if (!key) return;
    lastWriteAt = now;
    const isPlaying = playingOverride ?? (!video.paused && !video.ended);
    lastPlayingSample = isPlaying;
    post({type:'veloce-playback-position', key, url:location.href, positionSeconds:video.currentTime, durationSeconds:video.duration, wasPlaying:isPlaying});
  };
  const requestResume = (video) => {
    if (!window.__veloceResumePlaybackEnabled) return;
    const key = videoKey();
    if (!key || resumed.has(key)) return;
    resumed.add(key);
    activeKey = key;
    post({type:'veloce-playback-query', url:location.href, key});
  };
  const refresh = () => {
    const video = selectVideo();
    const key = videoKey();
    if (!video || !key) {
      activeVideo = null;
      activeKey = '';
      return;
    }
    if (video !== activeVideo || key !== activeKey) {
      activeVideo = video;
      activeKey = key;
      lastWriteAt = 0;
      lastPlayingSample = !video.paused && !video.ended;
      requestResume(video);
    }
  };
  window.__velocePlaybackRefresh = refresh;
  window.__velocePlaybackSettingsChanged = (enabled) => {
    if (enabled) {
      resumed.clear();
      if (activeVideo) requestResume(activeVideo);
      else refresh();
    }
  };
  const onVideoEvent = (event) => {
    const video = event.target;
    if (!(video instanceof HTMLVideoElement)) return;
    refresh();
    if (video !== activeVideo) return;
    if (event.type === 'play' || event.type === 'playing') {
      lastPlayingSample = true;
      sendPosition(video, true, true);
    } else if (event.type === 'pause') {
      lastPlayingSample = false;
      sendPosition(video, true, false);
    } else if (event.type === 'timeupdate' || event.type === 'seeked') {
      lastPlayingSample = !video.paused && !video.ended;
      sendPosition(video, event.type === 'seeked');
    } else if (event.type === 'ended') {
      post({type:'veloce-playback-clear', url:location.href});
    } else if (event.type === 'loadedmetadata') {
      requestResume(video);
    }
  };
  ['play','playing','pause','timeupdate','seeked','ended','loadedmetadata'].forEach((name) =>
    document.addEventListener(name, onVideoEvent, true)
  );
  const onPageHide = () => {
    // Preserve the last known playing state on close/navigation. WebView2 may
    // dispatch a synthetic pause while tearing down the page.
    if (activeVideo) sendPosition(activeVideo, true, lastPlayingSample);
  };
  window.addEventListener('pagehide', onPageHide, {capture:true});
  const onNativeMessage = (event) => {
    const data = event && event.data;
    if (!window.__veloceResumePlaybackEnabled || !data || data.type !== 'veloce-playback-resume') return;
    const video = selectVideo();
    if (!video || data.key !== videoKey() || !Number.isFinite(data.positionSeconds)) {
      if (typeof data.key === 'string') resumed.delete(data.key);
      return;
    }
    const position = Math.max(0, Number(data.positionSeconds));
    const apply = () => {
      if (!Number.isFinite(video.duration) || video.duration < 1 || video.readyState < 1) return false;
      if (position >= video.duration - 1) return false;
      try { video.currentTime = position; } catch (_) { return false; }
      if (window.__veloceResumePlaybackEnabled && data.wasPlaying) {
        const result = video.play();
        if (result && typeof result.catch === 'function') result.catch(() => {});
      }
      return true;
    };
    if (!apply()) {
      video.addEventListener('loadedmetadata', apply, {once:true});
    }
  };
  const attachBridge = () => {
    const bridge = window.chrome && window.chrome.webview;
    if (bridge && typeof bridge.addEventListener === 'function') {
      bridge.addEventListener('message', onNativeMessage);
      refresh();
      return true;
    }
    return false;
  };
  if (!attachBridge()) document.addEventListener('DOMContentLoaded', attachBridge, {once:true});
  document.addEventListener('loadedmetadata', refresh, true);
  document.addEventListener('playing', refresh, true);
  if (isYouTube()) {
    window.addEventListener('yt-navigate-finish', () => { resumed.clear(); refresh(); });
    window.addEventListener('popstate', () => { resumed.clear(); refresh(); });
  }
  // Initial document-created scripts can precede the first video node.
  document.addEventListener('DOMContentLoaded', refresh, {once:true});
  refresh();
})()"#;

fn playback_script(resume_enabled: bool) -> String {
    VIDEO_RESUME_SCRIPT.replace(
        "__RESUME_ENABLED__",
        if resume_enabled { "true" } else { "false" },
    )
}

fn youtube_settings_script(
    video_fill: bool,
    hide_interruptions: bool,
    resume_video: bool,
) -> String {
    format!(
        "{}; {}; {}; {}",
        youtube_fill_script(video_fill),
        youtube_interruption_notice_script(hide_interruptions),
        YOUTUBE_PLAYBACK_DIAGNOSTICS_SCRIPT,
        playback_script(resume_video)
    )
}

#[cfg(windows)]
fn register_youtube_diagnostics(
    app: &AppHandle<Wry>,
    view: &Webview<Wry>,
    tab_id: String,
) -> Result<(), String> {
    let registration_app = app.clone();
    let registration_tab_id = tab_id.clone();
    view.with_webview(move |platform| {
        let callback_app = registration_app.clone();
        let callback_tab_id = registration_tab_id.clone();
        let result = (|| -> windows::core::Result<()> {
            let core = unsafe { platform.controller().CoreWebView2()? };
            let handler = WebMessageReceivedEventHandler::create(Box::new(
                move |sender, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut raw_source = PWSTR::null();
                    if unsafe { args.Source(&mut raw_source) }.is_err() {
                        return Ok(());
                    }
                    let source = CoTaskMemPWSTR::from(raw_source).to_string();
                    let Ok(source_url) = Url::parse(&source) else { return Ok(()) };
                    let Some(host) = source_url.host_str() else { return Ok(()) };
                    if source_url.scheme() != "https" {
                        return Ok(());
                    }

                    let mut raw_message = PWSTR::null();
                    if unsafe { args.WebMessageAsJson(&mut raw_message) }.is_err() {
                        return Ok(());
                    }
                    let message = CoTaskMemPWSTR::from(raw_message).to_string();
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(&message) else {
                        return Ok(());
                    };
                    let message_type = value.get("type").and_then(serde_json::Value::as_str);
                    if message_type == Some("veloce-media-state") {
                        let Some(frame_id) = value
                            .get("frameId")
                            .and_then(serde_json::Value::as_str)
                            .filter(|frame_id| {
                                !frame_id.is_empty()
                                    && frame_id.len() <= 80
                                    && frame_id.chars().all(|character| {
                                        character.is_ascii_alphanumeric()
                                            || character == '-'
                                            || character == '_'
                                    })
                            })
                        else {
                            return Ok(());
                        };
                        let playing_video = value
                            .get("playingVideo")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        let frame_key = format!("{callback_tab_id}:{frame_id}");
                        if let Ok(mut frames) = callback_app
                            .state::<AppState>()
                            .active_video_frames
                            .lock()
                        {
                            if playing_video {
                                if frames.len() < 2048 || frames.contains(&frame_key) {
                                    frames.insert(frame_key);
                                }
                            } else {
                                frames.remove(&frame_key);
                            }
                        }
                        return Ok(());
                    }
                    if matches!(
                        message_type,
                        Some("veloce-playback-query")
                            | Some("veloce-playback-position")
                            | Some("veloce-playback-clear")
                    ) {
                        let playback_ops = callback_app.state::<AppState>().playback_ops.clone();
                        let Ok(_playback_guard) = playback_ops.lock() else {
                            let _ = callback_app.emit(
                                "veloce-native-error",
                                "Der lokale Video-Fortschritt ist gerade belegt.",
                            );
                            return Ok(());
                        };
                        let Some(raw_url) = value
                            .get("url")
                            .and_then(serde_json::Value::as_str)
                            .filter(|value| !value.is_empty() && value.len() <= 4096)
                        else {
                            return Ok(());
                        };
                        let Ok(page_url) = Url::parse(raw_url) else { return Ok(()) };
                        let Some(page_host) = page_url.host_str() else { return Ok(()) };
                        let youtube_source = host == "youtube.com" || host.ends_with(".youtube.com");
                        let youtube_page = page_host == "youtube.com" || page_host.ends_with(".youtube.com");
                        if page_url.scheme() != "https"
                            || !page_url.username().is_empty()
                            || page_url.password().is_some()
                            || !(host.eq_ignore_ascii_case(page_host)
                                || (youtube_source && youtube_page))
                        {
                            return Ok(());
                        }
                        match message_type {
                            Some("veloce-playback-query") => {
                                let requested_key = value
                                    .get("key")
                                    .and_then(serde_json::Value::as_str)
                                    .filter(|key| key.len() <= 4096);
                                if requested_key.is_some_and(|key| key != playback_key(&page_url)) {
                                    return Ok(());
                                }
                                match get_playback_checkpoint(&callback_app, &page_url) {
                                    Ok(Some(checkpoint)) => {
                                        let response = json!({
                                            "type": "veloce-playback-resume",
                                            "key": checkpoint.key,
                                            "positionSeconds": checkpoint.position_seconds,
                                            "durationSeconds": checkpoint.duration_seconds,
                                            "wasPlaying": checkpoint.was_playing,
                                        });
                                        if let Some(sender) = sender.as_ref() {
                                            let response = HSTRING::from(response.to_string());
                                            unsafe { sender.PostWebMessageAsJson(&response)?; }
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(error) => {
                                        let _ = callback_app.emit("veloce-native-error", error);
                                    }
                                }
                            }
                            Some("veloce-playback-position") => {
                                if value
                                    .get("key")
                                    .and_then(serde_json::Value::as_str)
                                    .is_some_and(|key| key != playback_key(&page_url))
                                {
                                    return Ok(());
                                }
                                let Some(position) = value
                                    .get("positionSeconds")
                                    .and_then(serde_json::Value::as_f64)
                                else {
                                    return Ok(());
                                };
                                let Some(duration) = value
                                    .get("durationSeconds")
                                    .and_then(serde_json::Value::as_f64)
                                else {
                                    return Ok(());
                                };
                                let was_playing = value
                                    .get("wasPlaying")
                                    .and_then(serde_json::Value::as_bool)
                                    .unwrap_or(false);
                                if let Err(error) = save_playback_checkpoint(
                                    &callback_app,
                                    &page_url,
                                    position,
                                    duration,
                                    was_playing,
                                ) {
                                    let _ = callback_app.emit("veloce-native-error", error);
                                }
                            }
                            Some("veloce-playback-clear") => {
                                if let Err(error) = clear_playback_checkpoint(&callback_app, &page_url) {
                                    let _ = callback_app.emit("veloce-native-error", error);
                                }
                            }
                            _ => {}
                        }
                        return Ok(());
                    }
                    if !(host == "youtube.com" || host.ends_with(".youtube.com")) {
                        return Ok(());
                    }
                    match message_type {
                        Some("veloce-youtube-location") => {
                            let Some(raw_url) = value
                                .get("url")
                                .and_then(serde_json::Value::as_str)
                                .filter(|value| !value.is_empty() && value.len() <= 4096)
                            else {
                                return Ok(());
                            };
                            let Ok(page_url) = Url::parse(raw_url) else { return Ok(()) };
                            let Some(page_host) = page_url.host_str() else { return Ok(()) };
                            if page_url.scheme() != "https"
                                || !page_url.username().is_empty()
                                || page_url.password().is_some()
                                || !(page_host == "youtube.com" || page_host.ends_with(".youtube.com"))
                            {
                                return Ok(());
                            }
                            let next_url = page_url.to_string();
                            let state = callback_app.state::<AppState>();
                            let changed = if let Ok(mut tabs) = state.tabs.lock() {
                                if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == callback_tab_id) {
                                    if tab.url != next_url {
                                        tab.url = next_url;
                                        tab.sleeping = false;
                                        tab.suspended = false;
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            if changed {
                                persist_session(&callback_app, &state);
                                emit_tabs(&callback_app, &state);
                            }
                            return Ok(());
                        }
                        Some("veloce-youtube-metric") => {}
                        _ => return Ok(()),
                    }
                    let Some(phase) = value.get("phase").and_then(serde_json::Value::as_str)
                    else {
                        return Ok(());
                    };
                    let allowed = [
                        "page-start", "dom-content-loaded", "window-load", "video-click",
                        "player-request", "metadata", "canplay", "first-frame",
                        "buffer-wait", "buffer-resume", "slow-resource",
                    ];
                    if !allowed.contains(&phase) {
                        return Ok(());
                    }
                    let Some(elapsed_ms) = value
                        .get("elapsedMs")
                        .and_then(serde_json::Value::as_u64)
                        .filter(|value| *value <= 3_600_000)
                    else {
                        return Ok(());
                    };
                    let attempt_id = value
                        .get("attemptId")
                        .and_then(serde_json::Value::as_str)
                        .filter(|value| !value.is_empty() && value.len() <= 80)
                        .unwrap_or("unknown");
                    let source_kind = match value
                        .get("source")
                        .and_then(serde_json::Value::as_str)
                    {
                        Some("click") => "click",
                        _ => "page",
                    };
                    let mut payload = json!({
                        "id": callback_tab_id,
                        "attemptId": attempt_id,
                        "source": source_kind,
                        "phase": phase,
                        "elapsedMs": elapsed_ms,
                    });
                    if phase == "slow-resource" {
                        let resource_host = value
                            .get("resourceHost")
                            .and_then(serde_json::Value::as_str)
                            .filter(|host| ["youtube.com", "youtubei.googleapis.com", "ytimg.com", "googlevideo.com"].contains(host))
                            .unwrap_or("unknown");
                        let resource_type = value
                            .get("resourceType")
                            .and_then(serde_json::Value::as_str)
                            .filter(|kind| kind.len() <= 24)
                            .unwrap_or("resource");
                        let duration = value
                            .get("resourceDurationMs")
                            .and_then(serde_json::Value::as_u64)
                            .filter(|value| *value <= 3_600_000)
                            .unwrap_or(0);
                        let status = value
                            .get("responseStatus")
                            .and_then(serde_json::Value::as_u64)
                            .filter(|value| (100..=599).contains(value));
                        let transfer_bytes = value
                            .get("transferBytes")
                            .and_then(serde_json::Value::as_u64)
                            .filter(|value| *value <= 1_000_000_000)
                            .unwrap_or(0);
                        payload["resourceHost"] = json!(resource_host);
                        payload["resourceType"] = json!(resource_type);
                        payload["resourceDurationMs"] = json!(duration);
                        payload["responseStatus"] = json!(status);
                        payload["transferBytes"] = json!(transfer_bytes);
                    }
                    if phase == "buffer-resume" {
                        payload["stallMs"] = json!(value
                            .get("stallMs")
                            .and_then(serde_json::Value::as_u64)
                            .filter(|value| *value <= 3_600_000)
                            .unwrap_or(0));
                    }
                    let _ = callback_app.emit("veloce-youtube-metric", payload);
                    Ok(())
                },
            ));
            let mut token = 0_i64;
            unsafe { core.add_WebMessageReceived(&handler, &mut token) }
        })()
        .map_err(|error| error.to_string());
        if let Err(error) = result {
            let _ = registration_app.emit(
                "veloce-native-error",
                format!("YouTube-Diagnose für Tab {registration_tab_id} konnte nicht gestartet werden: {error}"),
            );
        }
    })
    .map_err(|error| format!("YouTube-Diagnose konnte nicht an WebView2 angeschlossen werden: {error}"))
}

fn create_webview(app: &AppHandle<Wry>, state: &AppState, tab: &BrowserTab) -> Result<(), String> {
    if tab.url == START_URL || app.get_webview(&tab.id).is_some() {
        return Ok(());
    }
    let url = Url::parse(&tab.url).map_err(|error| format!("Ungültige Seitenadresse: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Veloce öffnet nur HTTP- und HTTPS-Webseiten.".into());
    }
    // Install/check extensions against the existing shared WebView2 profile
    // before the first external page starts navigating. Errors are shown in
    // Settings but do not prevent browsing.
    ensure_extensions(app, state);
    let window = app
        .get_window("main")
        .ok_or("Das Veloce-Hauptfenster fehlt.")?;
    let width = *state
        .width
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let height = *state
        .height
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let top = *state
        .chrome_height
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let id_navigation = tab.id.clone();
    let app_navigation = app.clone();
    let id_load = tab.id.clone();
    let app_load = app.clone();
    let id_title = tab.id.clone();
    let app_title = app.clone();
    let app_popup = app.clone();
    let app_downloads = app.clone();
    let (youtube_video_fill, hide_youtube_interruptions, password_autosave, resume_video_playback) =
        state
            .settings
            .lock()
            .map(|settings| {
                (
                    settings.youtube_video_fill,
                    settings.youtube_hide_interruptions,
                    settings.password_autosave,
                    settings.resume_video_playback,
                )
            })
            .unwrap_or((true, true, true, true));
    let builder = WebviewBuilder::new(tab.id.clone(), WebviewUrl::External(url))
        .browser_extensions_enabled(true)
        .initialization_script(MEDIA_ACTIVITY_SCRIPT)
        .initialization_script(youtube_settings_script(
            youtube_video_fill,
            hide_youtube_interruptions,
            resume_video_playback,
        ))
        .zoom_hotkeys_enabled(true)
        .on_download(move |_webview, event| {
            match event {
                DownloadEvent::Requested { url, destination } => {
                    record_download_requested(&app_downloads, &url, destination);
                }
                DownloadEvent::Finished { url, path, success } => {
                    record_download_finished(&app_downloads, &url, path.as_ref(), success);
                }
                _ => {}
            }
            true
        })
        .on_navigation(move |url| {
            if !matches!(url.scheme(), "http" | "https") {
                return false;
            }
            let state = app_navigation.state::<AppState>();
            clear_tab_video_activity(&state, &id_navigation);
            let protection = state
                .settings
                .lock()
                .map(|settings| settings.tracking_protection)
                .unwrap_or(false);
            if protection && url.host_str().is_some_and(is_tracking_host) {
                let _ = app_navigation.emit(
                    "veloce-navigation-blocked",
                    json!({"id": id_navigation, "host": url.host_str()}),
                );
                return false;
            }
            if let Ok(mut tabs) = state.tabs.lock() {
                if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id_navigation) {
                    tab.url = url.to_string();
                    tab.title = url.host_str().unwrap_or("Lädt …").to_string();
                    tab.sleeping = false;
                    tab.suspended = false;
                }
            }
            persist_session(&app_navigation, &state);
            let _ = app_navigation.emit(
                "veloce-navigation-started",
                json!({"id": id_navigation, "url": url.as_str()}),
            );
            true
        })
        .on_new_window(move |url, _features| {
            if matches!(url.scheme(), "http" | "https") {
                let _ = app_popup.emit("veloce-new-tab-requested", json!({"url": url.as_str()}));
            }
            NewWindowResponse::Deny
        })
        .on_page_load(move |_webview, payload| {
            if payload.event() == PageLoadEvent::Started {
                let _ = app_load.emit(
                    "veloce-page-loading",
                    json!({"id": id_load, "url": payload.url().as_str()}),
                );
                return;
            }
            if payload.event() == PageLoadEvent::Finished {
                let state = app_load.state::<AppState>();
                if let Ok(mut tabs) = state.tabs.lock() {
                    if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id_load) {
                        tab.url = payload.url().to_string();
                    }
                }
                persist_session(&app_load, &state);
                let _ = app_load.emit(
                    "veloce-page-loaded",
                    json!({"id": id_load, "url": payload.url().as_str()}),
                );
            }
        })
        .on_document_title_changed(move |_webview, title| {
            let state = app_title.state::<AppState>();
            if let Ok(mut tabs) = state.tabs.lock() {
                if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id_title) {
                    tab.title = title;
                }
            }
            emit_tabs(&app_title, &state);
        });
    #[cfg(windows)]
    let builder = builder.with_environment(shared_webview2_environment(app, state)?);
    let view = window
        .add_child(
            builder,
            LogicalPosition::new(0.0, top),
            LogicalSize::new(width.max(200.0), (height - top).max(100.0)),
        )
        .map_err(|error| format!("WebView konnte nicht erstellt werden: {error}"))?;
    view.set_zoom(clamp_zoom(tab.zoom))
        .map_err(|error| format!("Zoom konnte nicht gesetzt werden: {error}"))?;
    #[cfg(windows)]
    if let Err(error) = set_webview_password_autosave(&view, password_autosave) {
        let _ = app.emit(
            "veloce-native-error",
            format!("Passwortspeicherung konnte nicht aktiviert werden: {error}"),
        );
    }
    #[cfg(windows)]
    if let Err(error) = register_youtube_diagnostics(app, &view, tab.id.clone()) {
        let _ = app.emit("veloce-native-error", error);
    }
    #[cfg(windows)]
    {
        let fullscreen_app = app.clone();
        let fullscreen_id = tab.id.clone();
        view.with_webview(move |platform| {
            let result = (|| -> windows::core::Result<()> {
                let core = unsafe { platform.controller().CoreWebView2()? };
                let callback_app = fullscreen_app.clone();
                let callback_id = fullscreen_id.clone();
                let handler = ContainsFullScreenElementChangedEventHandler::create(Box::new(
                    move |sender, _args| {
                        let Some(sender) = sender else { return Ok(()) };
                        let mut contains_fullscreen = windows::core::BOOL(0);
                        if let Err(error) =
                            unsafe { sender.ContainsFullScreenElement(&mut contains_fullscreen) }
                        {
                            let _ = callback_app.emit(
                                "veloce-native-error",
                                format!("Vollbildstatus konnte nicht gelesen werden: {error}"),
                            );
                            return Ok(());
                        }
                        let _ = callback_app.emit(
                            "veloce-fullscreen-changed",
                            json!({"id": callback_id, "fullscreen": contains_fullscreen.as_bool()}),
                        );
                        Ok(())
                    },
                ));
                let mut token = 0_i64;
                unsafe { core.add_ContainsFullScreenElementChanged(&handler, &mut token) }
            })();
            if let Err(error) = result {
                let _ = fullscreen_app.emit(
                    "veloce-native-error",
                    format!(
                        "YouTube-Vollbild konnte nicht mit dem Fenster gekoppelt werden: {error}"
                    ),
                );
            }
        })
        .map_err(|error| format!("Vollbildbehandlung konnte nicht registriert werden: {error}"))?;
    }
    Ok(())
}

fn activate_view(app: &AppHandle<Wry>, state: &AppState, id: &str) -> Result<(), String> {
    let previous_id = state
        .active_id
        .lock()
        .map_err(|_| "Aktiver Tab ist nicht verfügbar.".to_string())?
        .clone();
    if let Ok(mut tabs) = state.tabs.lock() {
        if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id) {
            tab.sleeping = false;
            tab.suspended = false;
        }
    }
    let target = get_tab(state, id)?;
    *state
        .active_id
        .lock()
        .map_err(|_| "Aktiver Tab ist nicht verfügbar.".to_string())? = Some(id.to_string());
    if target.url != START_URL {
        create_webview(app, state, &target)?;
    }
    let tabs = state
        .tabs
        .lock()
        .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?
        .clone();
    let top = *state
        .chrome_height
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let width = *state
        .width
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let height = *state
        .height
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let memory_saver = state
        .settings
        .lock()
        .map(|settings| settings.memory_saver)
        .unwrap_or(true);
    for tab in tabs {
        if let Some(view) = app.get_webview(&tab.id) {
            if tab.id == id && target.url != START_URL {
                resume_webview(&view)?;
                match set_webview_memory_target(&view, false) {
                    Ok(supported) => {
                        *state
                            .memory_target_supported
                            .lock()
                            .unwrap_or_else(|error| error.into_inner()) = Some(supported);
                    }
                    Err(error) => {
                        *state
                            .memory_target_supported
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner()) = Some(false);
                        let _ = app.emit(
                            "veloce-native-error",
                            format!(
                                "WebView2-Speichermodus konnte nicht angewendet werden: {error}"
                            ),
                        );
                    }
                }
                view.set_position(LogicalPosition::new(0.0, top))
                    .map_err(|error| error.to_string())?;
                view.set_size(LogicalSize::new(
                    width.max(200.0),
                    (height - top).max(100.0),
                ))
                .map_err(|error| error.to_string())?;
                view.show().map_err(|error| error.to_string())?;
                view.set_focus().map_err(|error| error.to_string())?;
            } else {
                view.hide().map_err(|error| error.to_string())?;
                if memory_saver
                    && tab.url != START_URL
                    && previous_id.as_deref() == Some(tab.id.as_str())
                {
                    if let Err(error) = apply_background_tab_policy(app, state, &tab.id, &view) {
                        set_tab_suspended_state(state, &tab.id, false);
                        let _ = app.emit(
                            "veloce-native-warning",
                            json!({"id": tab.id, "message": format!("Hintergrundtab wurde nicht pausiert: {error}")}),
                        );
                    }
                }
            }
        }
    }
    if target.url == START_URL {
        if let Some(shell) = app.get_webview("main") {
            shell.set_focus().map_err(|error| error.to_string())?;
        }
    }
    emit_tabs(app, state);
    persist_session(app, state);
    Ok(())
}

#[tauri::command]
fn restore_session(app: AppHandle<Wry>, state: State<'_, AppState>) -> SavedSession {
    let directory = app_dir(&app).ok();
    let mut saved_settings = directory
        .as_ref()
        .and_then(|dir| read_json::<BrowserSettings>(&dir.join("settings.json")))
        .unwrap_or_default();
    saved_settings.hardware_acceleration = true;
    if let Some(dir) = directory.as_ref() {
        let _ = write_json(&dir.join("settings.json"), &saved_settings);
    }
    *state
        .settings
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = saved_settings.clone();
    let saved = if saved_settings.restore_session {
        directory.and_then(|dir| read_json::<SavedSession>(&dir.join("session.json")))
    } else {
        None
    };
    let mut saved = saved.unwrap_or(SavedSession {
        tabs: Vec::new(),
        active_id: None,
    });
    saved.tabs.retain(|tab| {
        tab.url == START_URL
            || Url::parse(&tab.url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
    });
    saved.tabs.truncate(30);
    for tab in &mut saved.tabs {
        // Suspension is a runtime-only state; WebView2 views are recreated on
        // launch, so persisted tabs must start as resumed.
        tab.suspended = false;
    }
    if let Some(url) = external_launch_url() {
        if saved.tabs.len() >= 30 {
            saved.tabs.remove(0);
        }
        let external_tab = new_tab(url);
        saved.active_id = Some(external_tab.id.clone());
        saved.tabs.push(external_tab);
    }
    if !saved
        .tabs
        .iter()
        .any(|tab| Some(tab.id.as_str()) == saved.active_id.as_deref())
    {
        saved.active_id = saved.tabs.last().map(|tab| tab.id.clone());
    }
    *state.tabs.lock().unwrap_or_else(|error| error.into_inner()) = saved.tabs.clone();
    *state
        .active_id
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = saved.active_id.clone();
    emit_tabs(&app, &state);
    saved
}

fn open_tab_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    input: String,
    width: f64,
    height: f64,
    chrome_height: f64,
) -> Result<BrowserTab, String> {
    let _native = lock_native_ops(&state)?;
    *state
        .width
        .lock()
        .map_err(|_| "Fensterbreite nicht verfügbar.".to_string())? = width.clamp(200.0, 12000.0);
    *state
        .height
        .lock()
        .map_err(|_| "Fensterhöhe nicht verfügbar.".to_string())? = height.clamp(200.0, 12000.0);
    *state
        .chrome_height
        .lock()
        .map_err(|_| "Browserleistenhöhe nicht verfügbar.".to_string())? =
        chrome_height.clamp(0.0, 500.0);
    let engine = state
        .settings
        .lock()
        .map_err(|_| "Einstellungen nicht verfügbar.".to_string())?
        .search_engine
        .clone();
    let tab = new_tab(normalize_input(&input, &engine)?);
    state
        .tabs
        .lock()
        .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?
        .push(tab.clone());
    create_webview(&app, &state, &tab)?;
    activate_view(&app, &state, &tab.id)?;
    Ok(tab)
}

#[tauri::command]
async fn open_tab(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    input: String,
    width: f64,
    height: f64,
    chrome_height: f64,
) -> Result<BrowserTab, String> {
    let state = state.inner().clone();
    run_blocking(move || open_tab_blocking(app, state, input, width, height, chrome_height)).await
}

fn activate_tab_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    id: String,
    width: f64,
    height: f64,
    chrome_height: f64,
) -> Result<(), String> {
    let _native = lock_native_ops(&state)?;
    *state
        .width
        .lock()
        .map_err(|_| "Fensterbreite nicht verfügbar.".to_string())? = width.clamp(200.0, 12000.0);
    *state
        .height
        .lock()
        .map_err(|_| "Fensterhöhe nicht verfügbar.".to_string())? = height.clamp(200.0, 12000.0);
    *state
        .chrome_height
        .lock()
        .map_err(|_| "Browserleistenhöhe nicht verfügbar.".to_string())? =
        chrome_height.clamp(0.0, 500.0);
    activate_view(&app, &state, &id)
}

#[tauri::command]
async fn activate_tab(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    id: String,
    width: f64,
    height: f64,
    chrome_height: f64,
) -> Result<(), String> {
    let state = state.inner().clone();
    run_blocking(move || activate_tab_blocking(app, state, id, width, height, chrome_height)).await
}

fn close_tab_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    id: String,
) -> Result<TabsSnapshot, String> {
    let _native = lock_native_ops(&state)?;
    let was_active = state
        .active_id
        .lock()
        .map_err(|_| "Aktiver Tab ist nicht verfügbar.".to_string())?
        .as_deref()
        == Some(&id);
    {
        let mut tabs = state
            .tabs
            .lock()
            .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?;
        let index = tabs
            .iter()
            .position(|tab| tab.id == id)
            .ok_or("Der Tab existiert nicht mehr.")?;
        tabs.remove(index);
    }
    if let Some(view) = app.get_webview(&id) {
        let _ = view.close();
    }
    clear_tab_video_activity(&state, &id);
    if was_active {
        let next = state
            .tabs
            .lock()
            .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?
            .last()
            .cloned();
        let next = next.unwrap_or_else(|| new_tab(START_URL.into()));
        if !state
            .tabs
            .lock()
            .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?
            .iter()
            .any(|tab| tab.id == next.id)
        {
            state
                .tabs
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(next.clone());
        }
        activate_view(&app, &state, &next.id)?;
    } else {
        emit_tabs(&app, &state);
        persist_session(&app, &state);
    }
    Ok(tabs_snapshot(&state))
}

#[tauri::command]
async fn close_tab(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    id: String,
) -> Result<TabsSnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || close_tab_blocking(app, state, id)).await
}

fn navigate_tab_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    id: String,
    input: String,
) -> Result<String, String> {
    let _native = lock_native_ops(&state)?;
    let engine = state
        .settings
        .lock()
        .map_err(|_| "Einstellungen nicht verfügbar.".to_string())?
        .search_engine
        .clone();
    let url = normalize_input(&input, &engine)?;
    let mut tab = get_tab(&state, &id)?;
    if url == START_URL {
        if let Some(view) = app.get_webview(&id) {
            let _ = view.close();
        }
        clear_tab_video_activity(&state, &id);
        tab.url = START_URL.into();
        tab.title = "Neuer Tab".into();
        tab.sleeping = false;
        tab.suspended = false;
        if let Ok(mut tabs) = state.tabs.lock() {
            if let Some(current) = tabs.iter_mut().find(|current| current.id == id) {
                *current = tab;
            }
        }
    } else {
        tab.url = url.clone();
        tab.sleeping = false;
        tab.suspended = false;
        if let Ok(mut tabs) = state.tabs.lock() {
            if let Some(current) = tabs.iter_mut().find(|current| current.id == id) {
                *current = tab.clone();
            }
        }
        if let Some(view) = app.get_webview(&id) {
            view.navigate(Url::parse(&url).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        } else {
            create_webview(&app, &state, &tab)?;
        }
    }
    activate_view(&app, &state, &id)?;
    Ok(url)
}

#[tauri::command]
async fn navigate_tab(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    id: String,
    input: String,
) -> Result<String, String> {
    let state = state.inner().clone();
    run_blocking(move || navigate_tab_blocking(app, state, id, input)).await
}

fn history_move_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    id: String,
    direction: String,
) -> Result<(), String> {
    let _native = lock_native_ops(&state)?;
    let view = app
        .get_webview(&id)
        .ok_or("Dieser Tab ist noch nicht geladen.")?;
    match direction.as_str() {
        "back" => view
            .eval("history.back();")
            .map_err(|error| error.to_string()),
        "forward" => view
            .eval("history.forward();")
            .map_err(|error| error.to_string()),
        _ => Err("Unbekannte Verlaufsrichtung.".into()),
    }
}

#[tauri::command]
async fn history_move(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    id: String,
    direction: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    run_blocking(move || history_move_blocking(app, state, id, direction)).await
}

fn reload_tab_blocking(app: AppHandle<Wry>, state: AppState, id: String) -> Result<(), String> {
    let _native = lock_native_ops(&state)?;
    app.get_webview(&id)
        .ok_or("Dieser Tab ist im Ruhezustand.")?
        .reload()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn reload_tab(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    run_blocking(move || reload_tab_blocking(app, state, id)).await
}

fn set_tab_zoom_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    id: String,
    zoom: f64,
) -> Result<f64, String> {
    let _native = lock_native_ops(&state)?;
    let zoom = clamp_zoom(zoom);
    {
        let mut tabs = state
            .tabs
            .lock()
            .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?;
        let tab = tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .ok_or("Der Tab existiert nicht mehr.")?;
        tab.zoom = zoom;
    }
    if let Some(view) = app.get_webview(&id) {
        view.set_zoom(zoom).map_err(|error| error.to_string())?;
    }
    persist_session(&app, &state);
    emit_tabs(&app, &state);
    Ok(zoom)
}

#[tauri::command]
async fn set_tab_zoom(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    id: String,
    zoom: f64,
) -> Result<f64, String> {
    let state = state.inner().clone();
    run_blocking(move || set_tab_zoom_blocking(app, state, id, zoom)).await
}

#[tauri::command]
fn toggle_pin(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    let mut tabs = state
        .tabs
        .lock()
        .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?;
    let tab = tabs
        .iter_mut()
        .find(|tab| tab.id == id)
        .ok_or("Der Tab existiert nicht mehr.")?;
    tab.pinned = !tab.pinned;
    Ok(tab.pinned)
}

fn discard_tab_if_inactive_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    id: String,
) -> Result<bool, String> {
    let _native = lock_native_ops(&state)?;
    if state
        .active_id
        .lock()
        .map_err(|_| "Aktiver Tab ist nicht verfügbar.".to_string())?
        .as_deref()
        == Some(&id)
    {
        return Ok(false);
    }
    let mut tabs = state
        .tabs
        .lock()
        .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?;
    let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id) else {
        return Ok(false);
    };
    if tab.pinned || tab.url == START_URL {
        return Ok(false);
    }
    if let Some(view) = app.get_webview(&id) {
        view.close().map_err(|error| error.to_string())?;
    }
    clear_tab_video_activity(&state, &id);
    tab.sleeping = true;
    tab.suspended = false;
    drop(tabs);
    emit_tabs(&app, &state);
    persist_session(&app, &state);
    Ok(true)
}

#[tauri::command]
async fn discard_tab_if_inactive(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    let state = state.inner().clone();
    run_blocking(move || discard_tab_if_inactive_blocking(app, state, id)).await
}

fn sync_layout_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    width: f64,
    height: f64,
    chrome_height: f64,
) -> Result<(), String> {
    let _native = lock_native_ops(&state)?;
    let width = width.clamp(200.0, 12000.0);
    let height = height.clamp(200.0, 12000.0);
    let top = chrome_height.clamp(0.0, 500.0);
    *state
        .width
        .lock()
        .map_err(|_| "Fensterbreite nicht verfügbar.".to_string())? = width;
    *state
        .height
        .lock()
        .map_err(|_| "Fensterhöhe nicht verfügbar.".to_string())? = height;
    *state
        .chrome_height
        .lock()
        .map_err(|_| "Browserleistenhöhe nicht verfügbar.".to_string())? = top;
    let tabs = state
        .tabs
        .lock()
        .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?
        .clone();
    for tab in tabs {
        if let Some(view) = app.get_webview(&tab.id) {
            view.set_position(LogicalPosition::new(0.0, top))
                .map_err(|error| error.to_string())?;
            view.set_size(LogicalSize::new(width, (height - top).max(100.0)))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn sync_layout(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    width: f64,
    height: f64,
    chrome_height: f64,
) -> Result<(), String> {
    let state = state.inner().clone();
    run_blocking(move || sync_layout_blocking(app, state, width, height, chrome_height)).await
}

fn set_content_fullscreen_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    fullscreen: bool,
    width: f64,
    height: f64,
    chrome_height: f64,
) -> Result<(), String> {
    let _native = lock_native_ops(&state)?;
    let window = app
        .get_window("main")
        .ok_or("Das Veloce-Hauptfenster fehlt.")?;
    window
        .set_fullscreen(fullscreen)
        .map_err(|error| format!("Fenstervollbild konnte nicht geändert werden: {error}"))?;

    let measured = window.inner_size().ok();
    let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
    let width = measured
        .map(|size| size.width as f64 / scale)
        .unwrap_or(width)
        .clamp(200.0, 12000.0);
    let height = measured
        .map(|size| size.height as f64 / scale)
        .unwrap_or(height)
        .clamp(200.0, 12000.0);
    let top = if fullscreen {
        0.0
    } else {
        chrome_height.clamp(0.0, 500.0)
    };
    *state
        .width
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = width;
    *state
        .height
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = height;
    *state
        .chrome_height
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = top;

    let active_id = state
        .active_id
        .lock()
        .map_err(|_| "Aktiver Tab ist nicht verfügbar.".to_string())?
        .clone();
    if let Some(id) = active_id {
        if let Some(view) = app.get_webview(&id) {
            view.set_position(LogicalPosition::new(0.0, top))
                .map_err(|error| error.to_string())?;
            view.set_size(LogicalSize::new(width, (height - top).max(100.0)))
                .map_err(|error| error.to_string())?;
            view.show().map_err(|error| error.to_string())?;
            view.set_focus().map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn set_content_fullscreen(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    fullscreen: bool,
    width: f64,
    height: f64,
    chrome_height: f64,
) -> Result<(), String> {
    let state = state.inner().clone();
    run_blocking(move || {
        set_content_fullscreen_blocking(app, state, fullscreen, width, height, chrome_height)
    })
    .await
}

fn set_view_visible_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    visible: bool,
) -> Result<(), String> {
    let _native = lock_native_ops(&state)?;
    if visible {
        let id = state
            .active_id
            .lock()
            .map_err(|_| "Aktiver Tab ist nicht verfügbar.".to_string())?
            .clone()
            .ok_or("Kein aktiver Tab vorhanden.")?;
        return activate_view(&app, &state, &id);
    }
    let tabs = state
        .tabs
        .lock()
        .map_err(|_| "Tabverwaltung ist nicht verfügbar.".to_string())?
        .clone();
    for tab in tabs {
        if let Some(view) = app.get_webview(&tab.id) {
            view.hide().map_err(|error| error.to_string())?;
        }
    }
    if let Some(shell) = app.get_webview("main") {
        shell.set_focus().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(windows)]
fn process_memory_usage(pid: u32, kind: String) -> Option<MemoryProcessUsage> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let result = unsafe {
        GetProcessMemoryInfo(
            process,
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    Some(MemoryProcessUsage {
        pid,
        kind,
        working_set_bytes: counters.WorkingSetSize as u64,
        private_commit_bytes: counters.PrivateUsage as u64,
    })
}

#[cfg(windows)]
fn get_memory_snapshot_blocking(
    app: AppHandle<Wry>,
    state: AppState,
) -> Result<MemorySnapshot, String> {
    let _native = lock_native_ops(&state)?;
    let shell = app
        .get_webview("main")
        .ok_or("Die WebView2-Oberfläche ist noch nicht bereit.")?;
    let (sender, receiver) = mpsc::channel::<Result<MemorySnapshot, String>>();
    shell
        .with_webview(move |platform| {
            let result = (|| -> windows::core::Result<MemorySnapshot> {
                let core = unsafe { platform.controller().CoreWebView2()? };
                let memory_target_supported = core.cast::<ICoreWebView2_19>().is_ok();
                let core2 = core.cast::<ICoreWebView2_2>()?;
                let environment = unsafe { core2.Environment()? };
                let environment8 = environment.cast::<ICoreWebView2Environment8>()?;
                let processes = unsafe { environment8.GetProcessInfos()? };
                let mut count = 0;
                unsafe { processes.Count(&mut count)? };

                let mut entries = Vec::new();
                let mut seen = HashSet::new();
                let mut inaccessible = 0_usize;
                for index in 0..count {
                    let process = unsafe { processes.GetValueAtIndex(index)? };
                    let mut pid = 0_i32;
                    let mut kind = Default::default();
                    unsafe {
                        process.ProcessId(&mut pid)?;
                        process.Kind(&mut kind)?;
                    }
                    if pid <= 0 || !seen.insert(pid as u32) {
                        continue;
                    }
                    let kind_name = match kind.0 {
                        0 => "Browser",
                        1 => "Renderer",
                        2 => "Utility",
                        3 => "Sandbox-Helfer",
                        4 => "GPU",
                        5 => "Plugin",
                        6 => "Plugin-Broker",
                        _ => "WebView2-Prozess",
                    };
                    if let Some(usage) = process_memory_usage(pid as u32, kind_name.into()) {
                        entries.push(usage);
                    } else {
                        inaccessible += 1;
                    }
                }

                let host_pid = std::process::id();
                if seen.insert(host_pid) {
                    if let Some(usage) = process_memory_usage(host_pid, "Veloce-App".into()) {
                        entries.push(usage);
                    } else {
                        inaccessible += 1;
                    }
                }
                entries.sort_by_key(|item| item.pid);
                let working_set_bytes = entries.iter().map(|item| item.working_set_bytes).sum();
                let private_commit_bytes =
                    entries.iter().map(|item| item.private_commit_bytes).sum();
                Ok(MemorySnapshot {
                    process_count: entries.len(),
                    webview_process_count: count as usize,
                    inaccessible_process_count: inaccessible,
                    working_set_bytes,
                    private_commit_bytes,
                    processes: entries,
                    background_memory_target_supported: memory_target_supported,
                    gpu_memory_included: false,
                    working_set_may_include_shared_pages: true,
                })
            })()
            .map_err(|error| format!("WebView2-Prozessmessung nicht verfügbar: {error}"));
            let _ = sender.send(result);
        })
        .map_err(|error| {
            format!("WebView2-Prozessmessung konnte nicht gestartet werden: {error}")
        })?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "Zeitüberschreitung beim Erfassen der WebView2-Prozesse.".to_string())?
}

#[cfg(not(windows))]
fn get_memory_snapshot_blocking(
    _app: AppHandle<Wry>,
    _state: AppState,
) -> Result<MemorySnapshot, String> {
    Err("Die Prozessmessung ist in diesem Build nur unter Windows verfügbar.".into())
}

#[tauri::command]
async fn get_memory_snapshot(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
) -> Result<MemorySnapshot, String> {
    let state = state.inner().clone();
    run_blocking(move || get_memory_snapshot_blocking(app, state)).await
}

#[tauri::command]
async fn set_view_visible(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    visible: bool,
) -> Result<(), String> {
    let state = state.inner().clone();
    run_blocking(move || set_view_visible_blocking(app, state, visible)).await
}

#[tauri::command]
fn get_settings(app: AppHandle<Wry>, state: State<'_, AppState>) -> BrowserSettings {
    if let Ok(dir) = app_dir(&app) {
        if let Some(mut settings) = read_json::<BrowserSettings>(&dir.join("settings.json")) {
            settings.hardware_acceleration = true;
            let _ = write_json(&dir.join("settings.json"), &settings);
            *state
                .settings
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = settings.clone();
            return settings;
        }
    }
    state
        .settings
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

#[tauri::command]
fn get_quick_shortcuts(app: AppHandle<Wry>) -> Result<Vec<QuickShortcut>, String> {
    let path = app_dir(&app)?.join("shortcuts.json");
    if path.exists() {
        let stored = read_json::<Vec<QuickShortcut>>(&path)
            .ok_or("Die gespeicherten Schnellzugriffe konnten nicht gelesen werden.")?;
        validate_shortcuts(stored)
    } else {
        let defaults = default_shortcuts();
        write_json(&path, &defaults)?;
        Ok(defaults)
    }
}

#[tauri::command]
fn save_quick_shortcuts(
    app: AppHandle<Wry>,
    shortcuts: Vec<QuickShortcut>,
) -> Result<Vec<QuickShortcut>, String> {
    let shortcuts = validate_shortcuts(shortcuts)?;
    write_json(&app_dir(&app)?.join("shortcuts.json"), &shortcuts)?;
    Ok(shortcuts)
}

#[tauri::command]
fn get_bookmarks(app: AppHandle<Wry>) -> Result<Vec<BookmarkEntry>, String> {
    let path = app_dir(&app)?.join("bookmarks.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_json::<Vec<BookmarkEntry>>(&path)
        .ok_or_else(|| "Die gespeicherten Lesezeichen konnten nicht gelesen werden.".into())
}

#[tauri::command]
fn set_bookmark(
    app: AppHandle<Wry>,
    url: String,
    title: String,
    saved: bool,
) -> Result<Vec<BookmarkEntry>, String> {
    let url = validate_web_url(&url, "das Lesezeichen")?;
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 256 {
        return Err("Der Lesezeichentitel muss 1 bis 256 Zeichen lang sein.".into());
    }
    let path = app_dir(&app)?.join("bookmarks.json");
    let mut bookmarks = if path.exists() {
        read_json::<Vec<BookmarkEntry>>(&path)
            .ok_or("Die gespeicherten Lesezeichen konnten nicht gelesen werden.")?
    } else {
        Vec::new()
    };
    if saved {
        if let Some(bookmark) = bookmarks.iter_mut().find(|bookmark| bookmark.url == url) {
            bookmark.title = title.to_string();
        } else {
            if bookmarks.len() >= 1000 {
                return Err("Der Lesezeichenspeicher ist voll (maximal 1000 Einträge).".into());
            }
            bookmarks.push(BookmarkEntry {
                id: format!("bookmark-{}", uuid::Uuid::new_v4().simple()),
                title: title.to_string(),
                url,
            });
        }
    } else {
        bookmarks.retain(|bookmark| bookmark.url != url);
    }
    write_json(&path, &bookmarks)?;
    Ok(bookmarks)
}

#[tauri::command]
async fn get_extension_status(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
) -> Result<Vec<ExtensionStatus>, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let _native = lock_native_ops(&state)?;
        let statuses = get_extensions_blocking(app)?;
        *state
            .extension_status
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = statuses.clone();
        Ok(statuses)
    })
    .await
}

#[tauri::command]
async fn set_extension_enabled(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let _native = lock_native_ops(&state)?;
        set_extension_enabled_blocking(app, id, enabled)
    })
    .await
}

#[tauri::command]
async fn remove_extension(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let _native = lock_native_ops(&state)?;
        remove_extension_blocking(app, id)
    })
    .await
}

#[tauri::command]
async fn add_custom_extension(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    path: String,
    expected_name: String,
    expected_version: String,
) -> Result<ExtensionStatus, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let _native = lock_native_ops(&state)?;
        add_custom_extension_blocking(app, path, expected_name, expected_version)
    })
    .await
}

#[tauri::command]
async fn initialize_extensions(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
) -> Result<Vec<ExtensionStatus>, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let _native = lock_native_ops(&state)?;
        *state
            .extension_attempted
            .lock()
            .map_err(|_| "Erweiterungsstatus ist nicht verfügbar.".to_string())? = false;
        ensure_extensions(&app, &state);
        Ok(state
            .extension_status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone())
    })
    .await
}

fn save_settings_blocking(
    app: AppHandle<Wry>,
    state: AppState,
    mut settings: BrowserSettings,
) -> Result<BrowserSettings, String> {
    // Keep the WebView2 default GPU path enabled. There is intentionally no
    // --disable-gpu startup flag or UI option that turns acceleration off.
    settings.hardware_acceleration = true;
    if !["duckduckgo", "brave", "google", "bing"].contains(&settings.search_engine.as_str()) {
        return Err("Diese Suchmaschine ist nicht unterstützt.".into());
    }
    if settings.sleep_after_minutes > 60 {
        return Err("Die Tab-Ruhezeit muss zwischen 0 und 60 Minuten liegen.".into());
    }
    if ![
        "balanced",
        "memory_saver",
        "smooth_video",
        "gaming",
        "custom",
    ]
    .contains(&settings.performance_profile.as_str())
    {
        return Err("Dieses Leistungsprofil ist ungültig.".into());
    }
    if !["graphite", "oled"].contains(&settings.theme.as_str()) {
        return Err("Dieses Erscheinungsbild ist ungültig.".into());
    }
    if !["violet", "cyan", "orange", "green"].contains(&settings.accent_color.as_str()) {
        return Err("Diese Akzentfarbe ist nicht unterstützt.".into());
    }
    let previous_settings = state
        .settings
        .lock()
        .map_err(|_| "Einstellungen sind nicht verfügbar.".to_string())?
        .clone();
    let memory_target_changed = previous_settings.memory_saver != settings.memory_saver;
    let youtube_video_fill_changed =
        previous_settings.youtube_video_fill != settings.youtube_video_fill;
    let youtube_interruptions_changed =
        previous_settings.youtube_hide_interruptions != settings.youtube_hide_interruptions;
    let password_autosave_changed =
        previous_settings.password_autosave != settings.password_autosave;
    let resume_video_playback_changed =
        previous_settings.resume_video_playback != settings.resume_video_playback;
    write_json(&app_dir(&app)?.join("settings.json"), &settings)?;
    *state
        .settings
        .lock()
        .map_err(|_| "Einstellungen sind nicht verfügbar.".to_string())? = settings.clone();
    let tabs = state
        .tabs
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let active_id = state
        .active_id
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    if memory_target_changed {
        for tab in &tabs {
            if let Some(view) = app.get_webview(&tab.id) {
                let is_active = active_id.as_deref() == Some(tab.id.as_str());
                if settings.memory_saver && !is_active && !tab.suspended {
                    if let Err(error) = apply_background_tab_policy(&app, &state, &tab.id, &view) {
                        let _ = set_webview_memory_target(&view, false);
                        set_tab_suspended_state(&state, &tab.id, false);
                        let _ = app.emit(
                            "veloce-native-warning",
                            json!({"id": tab.id, "message": format!("Hintergrundtab wurde nicht pausiert: {error}")}),
                        );
                    }
                } else {
                    if !settings.memory_saver && tab.suspended {
                        if let Err(error) = resume_webview(&view) {
                            let _ = app.emit(
                                "veloce-native-warning",
                                json!({"id": tab.id, "message": format!("Pausierter Tab konnte beim Ausschalten des Speichersparmodus nicht fortgesetzt werden: {error}")}),
                            );
                        } else {
                            set_tab_suspended_state(&state, &tab.id, false);
                        }
                    }
                    match set_webview_memory_target(&view, settings.memory_saver && !is_active) {
                        Ok(supported) => {
                            *state
                                .memory_target_supported
                                .lock()
                                .unwrap_or_else(|error| error.into_inner()) = Some(supported);
                        }
                        Err(error) => {
                            *state
                                .memory_target_supported
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner()) = Some(false);
                            let _ = app.emit(
                                "veloce-native-error",
                                format!(
                                    "WebView2-Speicherziel konnte nicht aktualisiert werden: {error}"
                                ),
                            );
                        }
                    }
                }
            }
        }
        emit_tabs(&app, &state);
        persist_session(&app, &state);
    }
    if password_autosave_changed {
        for tab in &tabs {
            if let Some(view) = app.get_webview(&tab.id) {
                #[cfg(windows)]
                if let Err(error) = set_webview_password_autosave(&view, settings.password_autosave)
                {
                    let _ = app.emit(
                        "veloce-native-error",
                        format!(
                            "Passwörter konnten für Tab {} nicht umgestellt werden: {error}",
                            tab.id
                        ),
                    );
                }
            }
        }
    }
    if youtube_video_fill_changed || youtube_interruptions_changed || resume_video_playback_changed
    {
        let script = youtube_settings_script(
            settings.youtube_video_fill,
            settings.youtube_hide_interruptions,
            settings.resume_video_playback,
        );
        for tab in tabs {
            let is_youtube = Url::parse(&tab.url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
                .is_some_and(|host| host == "youtube.com" || host.ends_with(".youtube.com"));
            if resume_video_playback_changed || is_youtube {
                if let Some(view) = app.get_webview(&tab.id) {
                    view.eval(script.clone()).map_err(|error| {
                        format!("YouTube-Videoanpassung konnte nicht angewendet werden: {error}")
                    })?;
                }
            }
        }
    }
    Ok(settings)
}

#[tauri::command]
async fn save_settings(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
    settings: BrowserSettings,
) -> Result<BrowserSettings, String> {
    let state = state.inner().clone();
    run_blocking(move || {
        let _native = lock_native_ops(&state)?;
        save_settings_blocking(app, state.clone(), settings)
    })
    .await
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let state = app.state::<AppState>().inner().clone();
            if let Ok(dir) = app_dir(&app.handle()) {
                if let Some(downloads) =
                    read_json::<Vec<DownloadEntry>>(&dir.join("downloads.json"))
                {
                    *state
                        .downloads
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = downloads;
                }
            }
            if let Err(error) = restore_window_geometry(app.handle()) {
                eprintln!("{error}");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            let app = window.app_handle().clone();
            let state = app.state::<AppState>().inner().clone();
            match event {
                WindowEvent::Moved(_)
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. } => {
                    schedule_window_geometry_save(app, state);
                }
                WindowEvent::CloseRequested { .. } => {
                    state.window_geometry_closing.store(true, Ordering::Release);
                    state
                        .window_geometry_revision
                        .fetch_add(1, Ordering::AcqRel);
                    state
                        .window_geometry_save_scheduled
                        .store(false, Ordering::Release);
                    persist_session(&app, &state);
                    if let Err(error) = save_window_geometry(&app) {
                        eprintln!("{error}");
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            restore_session,
            open_tab,
            activate_tab,
            close_tab,
            navigate_tab,
            history_move,
            reload_tab,
            set_tab_zoom,
            toggle_pin,
            discard_tab_if_inactive,
            sync_layout,
            set_content_fullscreen,
            set_view_visible,
            get_memory_snapshot,
            get_settings,
            save_settings,
            set_panel_overlay,
            register_default_browser,
            get_downloads,
            open_downloaded_file,
            clear_download_history,
            get_quick_shortcuts,
            save_quick_shortcuts,
            get_bookmarks,
            set_bookmark,
            get_extension_status,
            initialize_extensions,
            inspect_extension_manifest,
            set_extension_enabled,
            remove_extension,
            add_custom_extension
        ])
        .run(tauri::generate_context!())
        .expect("Veloce Browser konnte nicht gestartet werden.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_and_search_are_normalized() {
        assert_eq!(
            normalize_input("veloce://start", "duckduckgo").unwrap(),
            START_URL
        );
        assert_eq!(
            normalize_input("example.com", "duckduckgo").unwrap(),
            "https://example.com/"
        );
        assert_eq!(
            normalize_input("rust tauri", "google").unwrap(),
            "https://www.google.com/search?q=rust+tauri"
        );
    }

    #[test]
    fn tracking_host_matching_does_not_match_lookalikes() {
        assert!(is_tracking_host("ads.doubleclick.net"));
        assert!(is_tracking_host("doubleclick.net"));
        assert!(!is_tracking_host("notdoubleclick.net"));
    }

    #[test]
    fn zoom_is_clamped_and_invalid_values_reset_to_default() {
        assert_eq!(clamp_zoom(0.2), 0.5);
        assert_eq!(clamp_zoom(3.8), 3.0);
        assert_eq!(clamp_zoom(f64::NAN), 1.0);
    }

    #[test]
    fn download_history_stores_local_file_metadata_without_source_urls() {
        let url = Url::parse("https://files.example.test/path/report.pdf?token=private").unwrap();
        assert_eq!(
            download_file_name(&url, &PathBuf::from(r"C:\Users\User\Downloads\report.pdf")),
            "report.pdf"
        );
        assert_eq!(download_file_name(&url, &PathBuf::from("")), "report.pdf");

        let entry = DownloadEntry {
            id: "download-test".into(),
            file_name: "report.pdf".into(),
            path: r"C:\Users\User\Downloads\report.pdf".into(),
            started_at_ms: 1,
            status: "completed".into(),
            source_url: url.to_string(),
        };
        let saved = serde_json::to_string(&entry).unwrap();
        assert!(!saved.contains("token=private"));
        assert!(saved.contains("report.pdf"));
    }

    #[test]
    fn old_settings_files_receive_safe_defaults() {
        let settings: BrowserSettings = serde_json::from_str(
            r#"{"searchEngine":"brave","trackingProtection":true,"sleepAfterMinutes":4,"gamingMode":false,"reduceMotion":true}"#,
        ).unwrap();
        assert_eq!(settings.search_engine, "brave");
        assert!(settings.memory_saver);
        assert!(settings.hardware_acceleration);
        assert!(settings.restore_session);
        assert!(settings.youtube_video_fill);
    }

    #[test]
    fn extension_manifest_resources_reject_traversal_and_missing_files() {
        let root = std::env::temp_dir().join(format!("veloce-ext-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        fs::write(root.join("worker.js"), "// test").unwrap();
        let valid: serde_json::Value =
            serde_json::from_str(r#"{"background":{"service_worker":"worker.js"}}"#).unwrap();
        assert!(
            validate_manifest_resources(&root, &valid).is_ok(),
            "{:?}",
            validate_manifest_resources(&root, &valid)
        );
        let missing: serde_json::Value =
            serde_json::from_str(r#"{"options_page":"missing.html"}"#).unwrap();
        assert!(validate_manifest_resources(&root, &missing).is_err());
        let traversal: serde_json::Value =
            serde_json::from_str(r#"{"options_page":"../outside.html"}"#).unwrap();
        assert!(validate_manifest_resources(&root, &traversal).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

