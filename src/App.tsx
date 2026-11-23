import { type FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowLeft, ArrowRight, AudioLines, Bookmark, Check,
  Compass, Download, ExternalLink, Globe2, Home, LockKeyhole,
  MoreHorizontal, Pause, Plus, RefreshCw, Search, Settings2, ShieldCheck,
  Sparkles, X, ZoomIn, ZoomOut,
} from "lucide-react";

type Tab = { id: string; url: string; title: string; pinned: boolean; sleeping: boolean; suspended?: boolean; zoom: number };
type Snapshot = { tabs: Tab[]; activeId: string | null };
type QuickShortcut = { id: string; label: string; url: string };
type BookmarkEntry = { id: string; title: string; url: string };
type Panel = "more" | "settings" | "bookmarks" | "downloads" | null;
type SettingsData = {
  searchEngine: string;
  trackingProtection: boolean;
  sleepAfterMinutes: number;
  gamingMode: boolean;
  reduceMotion: boolean;
  performanceProfile: "balanced" | "memory_saver" | "smooth_video" | "gaming" | "custom";
  memorySaver: boolean;
  hardwareAcceleration: boolean;
  restoreSession: boolean;
  passwordAutosave: boolean;
  resumeVideoPlayback: boolean;
  theme: "graphite" | "oled";
  accentColor: "violet" | "cyan" | "orange" | "green";
  youtubeVideoFill: boolean;
  youtubeHideInterruptions: boolean;
};
type DownloadEntry = { id: string; fileName: string; path: string; startedAtMs: number; status: "downloading" | "completed" | "failed" };
type ExtensionStatus = {
  key: string;
  id: string;
  name: string;
  bundledVersion: string;
  installed: boolean;
  enabled: boolean;
  removable: boolean;
  detail: string;
};
type ExtensionManifestPreview = {
  path: string; name: string; version: string; manifestVersion: number;
  permissions: string[]; hostPermissions: string[];
};
type MemoryProcessUsage = { pid: number; kind: string; workingSetBytes: number; privateCommitBytes: number };
type MemorySnapshot = {
  processCount: number; webviewProcessCount: number; inaccessibleProcessCount: number;
  workingSetBytes: number; privateCommitBytes: number; processes: MemoryProcessUsage[];
  backgroundMemoryTargetSupported: boolean; gpuMemoryIncluded: boolean;
  workingSetMayIncludeSharedPages: boolean;
};
type YoutubeMetric = {
  id: string; attemptId: string; source: "click" | "page"; phase: string; elapsedMs: number;
  resourceHost?: string; resourceType?: string; resourceDurationMs?: number;
  responseStatus?: number | null; transferBytes?: number; stallMs?: number;
};

const YOUTUBE_METRIC_LABELS: Record<string, string> = {
  "page-start": "YouTube-Dokument gestartet",
  "dom-content-loaded": "Dokument-Grundgerüst bereit",
  "window-load": "Dokumentressourcen geladen",
  "video-click": "Video angeklickt",
  "player-request": "Videoplayer fordert Medien an",
  metadata: "Videometadaten bereit",
  canplay: "Player kann Wiedergabe starten",
  "first-frame": "Erstes Videobild dargestellt",
  "buffer-wait": "Player wartet auf Videodaten",
  "buffer-resume": "Pufferung fortgesetzt",
  "slow-resource": "Langsame YouTube-Ressource",
};

const START = "veloce://start";
const DEFAULT_SHORTCUTS: QuickShortcut[] = [
  { id: "youtube", label: "YouTube", url: "https://www.youtube.com/" },
  { id: "brave-search", label: "Brave Search", url: "https://search.brave.com/" },
  { id: "wikipedia", label: "Wikipedia", url: "https://www.wikipedia.org/" },
  { id: "github", label: "GitHub", url: "https://github.com/" },
];
const DEFAULT_SETTINGS: SettingsData = {
  searchEngine: "duckduckgo", trackingProtection: true,
  sleepAfterMinutes: 3, gamingMode: false, reduceMotion: true,
  performanceProfile: "balanced", memorySaver: true,
  hardwareAcceleration: true, restoreSession: true, passwordAutosave: true,
  resumeVideoPlayback: true,
  theme: "graphite", accentColor: "violet", youtubeVideoFill: true,
  youtubeHideInterruptions: true,
};

const SETTING_SECTIONS = [
  ["general", "Allgemein"], ["appearance", "Aussehen"], ["tabs", "Tabs"],
  ["privacy", "Datenschutz"], ["performance", "Leistung"],
  ["video", "Video & Medien"], ["extensions", "Erweiterungen"],
] as const;

const PROFILE_SETTINGS: Record<Exclude<SettingsData["performanceProfile"], "custom">, Partial<SettingsData>> = {
  balanced: { memorySaver: true, sleepAfterMinutes: 5, gamingMode: false, reduceMotion: true },
  memory_saver: { memorySaver: true, sleepAfterMinutes: 2, gamingMode: false, reduceMotion: true },
  smooth_video: { memorySaver: false, sleepAfterMinutes: 10, gamingMode: false, reduceMotion: true },
  gaming: { memorySaver: true, sleepAfterMinutes: 1, gamingMode: true, reduceMotion: true },
};

function mib(bytes: number) { return `${(bytes / 1024 / 1024).toFixed(1)} MiB`; }

function hostOf(value: string) {
  try { return new URL(value).hostname.replace(/^www\./, ""); } catch { return "Veloce"; }
}

function Logo() {
  return <svg className="brand-symbol" viewBox="0 0 36 36" aria-hidden="true">
    <defs><linearGradient id="veloce-gradient" x1="0" y1="0" x2="1" y2="1"><stop stopColor="#a78bfa"/><stop offset="1" stopColor="#55d6be"/></linearGradient></defs>
    <path d="M5 8.5h8.1L18 22.2l4.9-13.7H31L21.2 29h-6.4L5 8.5Z" fill="url(#veloce-gradient)"/>
    <path d="M25.2 26.5 31 28l-5.8 1.8 1.8 4-5.4-2.2 3.6-5.1Z" fill="#d8d1ff" opacity=".9"/>
  </svg>;
}

export default function App() {
  const chromeRef = useRef<HTMLDivElement>(null);
  const addressRef = useRef<HTMLInputElement>(null);
  const activeIdRef = useRef<string | null>(null);
  const tabsRef = useRef<Tab[]>([]);
  const sleepTimers = useRef(new Map<string, number>());
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [shortcuts, setShortcuts] = useState<QuickShortcut[]>(DEFAULT_SHORTCUTS);
  const [bookmarks, setBookmarks] = useState<BookmarkEntry[]>([]);
  const [downloads, setDownloads] = useState<DownloadEntry[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  activeIdRef.current = activeId;
  tabsRef.current = tabs;
  const activeTab = useMemo(() => tabs.find((tab) => tab.id === activeId), [tabs, activeId]);
  const [address, setAddress] = useState("");
  const [homeSearch, setHomeSearch] = useState("");
  const [shortcutFormOpen, setShortcutFormOpen] = useState(false);
  const [shortcutLabel, setShortcutLabel] = useState("");
  const [shortcutUrl, setShortcutUrl] = useState("");
  const draggedShortcutId = useRef<string | null>(null);
  const [settings, setSettings] = useState<SettingsData>(DEFAULT_SETTINGS);
  const [extensionStatuses, setExtensionStatuses] = useState<ExtensionStatus[]>([]);
  const [checkingExtensions, setCheckingExtensions] = useState(false);
  const [settingsSection, setSettingsSection] = useState<(typeof SETTING_SECTIONS)[number][0]>("general");
  const [memorySnapshot, setMemorySnapshot] = useState<MemorySnapshot | null>(null);
  const [measuringMemory, setMeasuringMemory] = useState(false);
  const [youtubeMetrics, setYoutubeMetrics] = useState<YoutubeMetric[]>([]);
  const [extensionFolder, setExtensionFolder] = useState("");
  const [extensionPreview, setExtensionPreview] = useState<ExtensionManifestPreview | null>(null);
  const [installingExtension, setInstallingExtension] = useState(false);
  const [videoFullscreen, setVideoFullscreen] = useState(false);
  const [panel, setPanel] = useState<Panel>(null);
  const [toast, setToast] = useState("");
  const toastTimer = useRef<number | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const report = useCallback((message: string) => {
    setError(message);
    console.error("[Veloce]", message);
  }, []);

  const showToast = useCallback((message: string) => {
    if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
    setToast(message);
    toastTimer.current = window.setTimeout(() => {
      setToast("");
      toastTimer.current = null;
    }, 1800);
  }, []);

  const geometry = useCallback(() => ({
    width: window.innerWidth,
    height: window.innerHeight,
    chromeHeight: videoFullscreen ? 0 : (chromeRef.current?.getBoundingClientRect().height ?? 98),
  }), [videoFullscreen]);

  const syncLayout = useCallback(() => {
    void invoke("sync_layout", geometry()).catch((reason) => report(String(reason)));
  }, [geometry, report]);

  const openTab = useCallback(async (input = START) => {
    try {
      const tab = await invoke<Tab>("open_tab", { input, ...geometry() });
      setTabs((current) => current.some((item) => item.id === tab.id) ? current : [...current, tab]);
      setActiveId(tab.id);
      setAddress(tab.url === START ? "" : tab.url);
      setHomeSearch("");
      setPanel(null);
      setError("");
      return tab;
    } catch (reason) { report(String(reason)); return null; }
  }, [geometry, report]);

  const activateTab = useCallback(async (id: string) => {
    try {
      await invoke("activate_tab", { id, ...geometry() });
      setActiveId(id);
      const tab = tabs.find((item) => item.id === id);
      setAddress(tab?.url === START ? "" : tab?.url ?? "");
      setPanel(null);
      setError("");
    } catch (reason) { report(String(reason)); }
  }, [geometry, report, tabs]);

  const closeTab = useCallback(async (id: string) => {
    try {
      const snapshot = await invoke<Snapshot>("close_tab", { id });
      setPanel(null);
      setTabs(snapshot.tabs);
      setActiveId(snapshot.activeId);
      const next = snapshot.tabs.find((tab) => tab.id === snapshot.activeId);
      setAddress(next?.url === START ? "" : next?.url ?? "");
    } catch (reason) { report(String(reason)); }
  }, [report]);

  const navigate = useCallback(async (raw = address) => {
    if (!activeId) { await openTab(raw); return; }
    setLoading(true);
    try {
      const url = await invoke<string>("navigate_tab", { id: activeId, input: raw });
      setAddress(url === START ? "" : url);
      setHomeSearch("");
      setPanel(null);
      setError("");
      addressRef.current?.blur();
    } catch (reason) {
      setLoading(false);
      report(`Navigation fehlgeschlagen: ${String(reason)}`);
    }
  }, [activeId, address, openTab, report]);

  const setZoom = useCallback(async (next: number) => {
    if (!activeId) return;
    const zoom = Math.min(3, Math.max(0.5, Math.round(next * 100) / 100));
    setTabs((current) => current.map((tab) => tab.id === activeId ? { ...tab, zoom } : tab));
    try {
      const actual = await invoke<number>("set_tab_zoom", { id: activeId, zoom });
      setTabs((current) => current.map((tab) => tab.id === activeId ? { ...tab, zoom: actual } : tab));
    } catch (reason) { report(String(reason)); }
  }, [activeId, report]);

  const updateSettings = useCallback(async (next: SettingsData) => {
    try {
      const saved = await invoke<SettingsData>("save_settings", { settings: next });
      setSettings(saved);
      showToast("Einstellungen gespeichert");
    } catch (reason) { report(String(reason)); }
  }, [report, showToast]);

  const changeSettings = useCallback((patch: Partial<SettingsData>) => {
    const profileOnly = Object.keys(patch).length === 1 && "performanceProfile" in patch;
    void updateSettings({ ...settings, ...patch, ...(!profileOnly ? { performanceProfile: "custom" as const } : {}) });
  }, [settings, updateSettings]);

  const chooseProfile = useCallback((profile: Exclude<SettingsData["performanceProfile"], "custom">) => {
    void updateSettings({ ...settings, ...PROFILE_SETTINGS[profile], performanceProfile: profile });
  }, [settings, updateSettings]);

  const refreshExtensions = useCallback(async () => {
    setCheckingExtensions(true);
    try { setExtensionStatuses(await invoke<ExtensionStatus[]>("get_extension_status")); }
    catch (reason) { report(`Erweiterungsstatus konnte nicht gelesen werden: ${String(reason)}`); }
    finally { setCheckingExtensions(false); }
  }, [report]);

  const measureMemory = useCallback(async () => {
    setMeasuringMemory(true);
    try { setMemorySnapshot(await invoke<MemorySnapshot>("get_memory_snapshot")); }
    catch (reason) { report(`WebView2-Verbrauch konnte nicht gemessen werden: ${String(reason)}`); }
    finally { setMeasuringMemory(false); }
  }, [report]);

  const toggleExtension = useCallback(async (extension: ExtensionStatus) => {
    if (!extension.installed || !extension.id) return;
    try {
      await invoke("set_extension_enabled", { id: extension.id, enabled: !extension.enabled });
      await refreshExtensions();
      showToast(`${extension.name}: ${extension.enabled ? "deaktiviert" : "aktiviert"}`);
    } catch (reason) { report(`Erweiterung konnte nicht geändert werden: ${String(reason)}`); }
  }, [refreshExtensions, report, showToast]);

  const inspectExtension = useCallback(async () => {
    setExtensionPreview(null);
    try {
      const preview = await invoke<ExtensionManifestPreview>("inspect_extension_manifest", { path: extensionFolder });
      setExtensionPreview(preview);
      setError("");
    } catch (reason) { report(`Erweiterung konnte nicht geprüft werden: ${String(reason)}`); }
  }, [extensionFolder, report]);

  const installCustomExtension = useCallback(async () => {
    if (!extensionPreview) return;
    const accepted = window.confirm(`Erweiterung „${extensionPreview.name}“ ${extensionPreview.version} hinzufügen?\n\nBerechtigungen: ${extensionPreview.permissions.join(", ") || "keine deklariert"}\nWebseitenzugriff: ${extensionPreview.hostPermissions.join(", ") || "keine Host-Muster deklariert"}\n\nNur vertrauenswürdige Erweiterungen installieren.`);
    if (!accepted) return;
    setInstallingExtension(true);
    try {
      const installed = await invoke<ExtensionStatus>("add_custom_extension", {
        path: extensionPreview.path, expectedName: extensionPreview.name, expectedVersion: extensionPreview.version,
      });
      setExtensionStatuses((current) => [...current.filter((item) => item.id !== installed.id), installed]);
      setExtensionPreview(null);
      setExtensionFolder("");
      showToast(`WebView2 bestätigt: ${installed.name} hinzugefügt`);
    } catch (reason) { report(`Erweiterung wurde nicht installiert: ${String(reason)}`); }
    finally { setInstallingExtension(false); }
  }, [extensionPreview, report, showToast]);

  const removeCustomExtension = useCallback(async (extension: ExtensionStatus) => {
    if (!extension.id || !window.confirm(`„${extension.name}“ aus dem Veloce-WebView2-Profil entfernen?`)) return;
    try {
      await invoke("remove_extension", { id: extension.id });
      await refreshExtensions();
    } catch (reason) { report(`Erweiterung konnte nicht entfernt werden: ${String(reason)}`); }
  }, [refreshExtensions, report]);

  const toggleBookmark = useCallback(async () => {
    if (!activeTab || activeTab.url === START) return;
    const alreadySaved = bookmarks.some((bookmark) => bookmark.url === activeTab.url);
    try {
      const updated = await invoke<BookmarkEntry[]>("set_bookmark", {
        url: activeTab.url,
        title: activeTab.title || hostOf(activeTab.url),
        saved: !alreadySaved,
      });
      setBookmarks(updated);
      showToast(alreadySaved ? "Lesezeichen entfernt" : "Lesezeichen gespeichert");
    } catch (reason) { report(`Lesezeichen konnte nicht gespeichert werden: ${String(reason)}`); }
  }, [activeTab, bookmarks, report, showToast]);

  const saveShortcuts = useCallback(async (next: QuickShortcut[]) => {
    try {
      const saved = await invoke<QuickShortcut[]>("save_quick_shortcuts", { shortcuts: next });
      setShortcuts(saved);
      setError("");
      return true;
    } catch (reason) {
      report(`Schnellzugriffe konnten nicht gespeichert werden: ${String(reason)}`);
      return false;
    }
  }, [report]);

  const addShortcut = useCallback(async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const label = shortcutLabel.trim();
    const rawUrl = shortcutUrl.trim();
    const url = /^https?:\/\//i.test(rawUrl) ? rawUrl : `https://${rawUrl}`;
    if (!label || !rawUrl) return;
    const next = [...shortcuts, { id: `custom-${crypto.randomUUID()}`, label, url }];
    if (await saveShortcuts(next)) {
      setShortcutLabel("");
      setShortcutUrl("");
      setShortcutFormOpen(false);
      showToast("Schnellzugriff hinzugefügt");
    }
  }, [saveShortcuts, showToast, shortcutLabel, shortcutUrl, shortcuts]);

  const removeShortcut = useCallback(async (id: string) => {
    if (!id.startsWith("custom-")) return;
    const next = shortcuts.filter((shortcut) => shortcut.id !== id);
    if (await saveShortcuts(next)) showToast("Schnellzugriff entfernt");
  }, [saveShortcuts, showToast, shortcuts]);

  const moveShortcut = useCallback(async (targetId: string) => {
    const sourceId = draggedShortcutId.current;
    draggedShortcutId.current = null;
    if (!sourceId || sourceId === targetId) return;
    const next = [...shortcuts];
    const from = next.findIndex((shortcut) => shortcut.id === sourceId);
    const to = next.findIndex((shortcut) => shortcut.id === targetId);
    if (from < 0 || to < 0) return;
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    await saveShortcuts(next);
  }, [saveShortcuts, shortcuts]);

  const openGooglePasswordManager = useCallback(() => {
    void openTab("https://passwords.google.com");
  }, [openTab]);

  const configureDefaultBrowser = useCallback(async () => {
    try {
      await invoke("register_default_browser");
      showToast("Windows-Einstellungen geöffnet — Veloce dort als Browser auswählen");
    } catch (reason) {
      report(`Windows-Standardbrowser konnte nicht eingerichtet werden: ${String(reason)}`);
    }
  }, [report, showToast]);

  const showPanel = useCallback(async (next: Panel) => {
    if (!next) {
      setPanel(null);
      return;
    }
    setPanel(next);
    void (async () => {
      try {
        if (next === "settings") {
          setSettingsSection("general");
          setSettings(await invoke<SettingsData>("get_settings"));
        }
      } catch (reason) {
        report(`Menü konnte nicht vollständig geöffnet werden: ${String(reason)}`);
      }
    })();
  }, [report]);

  useEffect(() => {
    document.documentElement.classList.toggle("native-overlay", Boolean(panel));
    document.body.classList.toggle("native-overlay", Boolean(panel));
    void invoke("set_panel_overlay", { enabled: Boolean(panel) }).catch((reason) => {
      report(`Browserüberlagerung konnte nicht angepasst werden: ${String(reason)}`);
    });
  }, [panel, report]);

  useEffect(() => () => {
    if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
    document.documentElement.classList.remove("native-overlay");
    document.body.classList.remove("native-overlay");
  }, []);

  useEffect(() => {
    let alive = true;
    const stops: Array<() => void> = [];
    void (async () => {
      const registrations = await Promise.allSettled([
        listen<Snapshot>("veloce-tabs-changed", (event) => {
          setTabs(event.payload.tabs);
          setActiveId(event.payload.activeId);
          const active = event.payload.tabs.find((tab) => tab.id === event.payload.activeId);
          if (active) setAddress(active.url === START ? "" : active.url);
        }),
        listen<{ id: string; url: string }>("veloce-navigation-started", (event) => {
          setTabs((current) => current.map((tab) => tab.id === event.payload.id ? { ...tab, url: event.payload.url, title: hostOf(event.payload.url), sleeping: false } : tab));
          if (event.payload.id === activeIdRef.current) setAddress(event.payload.url);
        }),
        listen<{ id: string; url: string }>("veloce-page-loaded", (event) => {
          setLoading(false);
          setTabs((current) => current.map((tab) => tab.id === event.payload.id ? { ...tab, url: event.payload.url, sleeping: false } : tab));
        }),
        listen<{ id: string; url: string }>("veloce-page-loading", (event) => {
          if (event.payload.id === activeIdRef.current) setLoading(true);
        }),
        listen<{ url: string }>("veloce-new-tab-requested", (event) => { void openTab(event.payload.url); }),
        listen<{ id: string; host: string }>("veloce-navigation-blocked", (event) => {
          if (event.payload.id === activeIdRef.current) showToast("Tracker-Domainnavigation blockiert: " + event.payload.host);
        }),
        listen<ExtensionStatus[]>("veloce-extension-status", (event) => setExtensionStatuses(event.payload)),
        listen<YoutubeMetric>("veloce-youtube-metric", (event) => {
          setYoutubeMetrics((current) => [event.payload, ...current].slice(0, 32));
        }),
        listen<string>("veloce-native-error", (event) => report(event.payload)),
        listen<{ id?: string; message: string }>("veloce-native-warning", (event) => report(event.payload.message)),
        listen<{ id: string; fullscreen: boolean }>("veloce-fullscreen-changed", (event) => {
          if (event.payload.id !== activeIdRef.current) return;
          const full = event.payload.fullscreen;
          setVideoFullscreen(full);
          void invoke("set_content_fullscreen", {
            fullscreen: full,
            width: window.innerWidth,
            height: window.innerHeight,
            // The chrome is display:none while fullscreen, so on exit use its
            // normal two-row height instead of measuring the hidden element.
            chromeHeight: full ? 0 : 96,
          }).catch((reason) => report(`Videovollbild konnte nicht angepasst werden: ${String(reason)}`));
        }),
      ]);
      for (const item of registrations) {
        if (item.status === "fulfilled") stops.push(item.value);
        else report("Ein Browser-Ereignis konnte nicht registriert werden: " + String(item.reason));
      }
      if (!alive) { stops.forEach((stop) => stop()); return; }
      try {
        const [session, storedSettings] = await Promise.all([
          invoke<Snapshot>("restore_session"), invoke<SettingsData>("get_settings"),
        ]);
        if (!alive) return;
        setSettings(storedSettings);
        void invoke<QuickShortcut[]>("get_quick_shortcuts").then(setShortcuts).catch((reason) => report(`Schnellzugriffe konnten nicht geladen werden: ${String(reason)}`));
        void invoke<BookmarkEntry[]>("get_bookmarks").then(setBookmarks).catch((reason) => report(`Lesezeichen konnten nicht geladen werden: ${String(reason)}`));
        void invoke<ExtensionStatus[]>("get_extension_status").then(setExtensionStatuses).catch((reason) => report(String(reason)));
        setTabs(session.tabs);
        if (session.tabs.length) {
          const id = session.activeId ?? session.tabs.at(-1)!.id;
          setActiveId(id);
          const tab = session.tabs.find((item) => item.id === id);
          setAddress(tab?.url === START ? "" : tab?.url ?? "");
          await invoke("activate_tab", { id, ...geometry() });
        } else {
          const tab = await invoke<Tab>("open_tab", { input: START, ...geometry() });
          setTabs([tab]); setActiveId(tab.id);
        }
      } catch (reason) { report("Veloce konnte nicht gestartet werden: " + String(reason)); }
    })();
    return () => { alive = false; stops.forEach((stop) => stop()); for (const timer of sleepTimers.current.values()) window.clearTimeout(timer); sleepTimers.current.clear(); };
    // Initialize once; native events own subsequent tab changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let alive = true;
    let stop: (() => void) | undefined;
    void invoke<DownloadEntry[]>("get_downloads").then((items) => {
      if (alive) setDownloads(items);
    }).catch((reason) => report(`Downloadliste konnte nicht geladen werden: ${String(reason)}`));
    void listen<DownloadEntry[]>("veloce-downloads-changed", (event) => {
      if (alive) setDownloads(event.payload);
    }).then((unlisten) => {
      if (alive) stop = unlisten;
      else unlisten();
    }).catch((reason) => report(`Download-Ereignisse konnten nicht registriert werden: ${String(reason)}`));
    return () => { alive = false; stop?.(); };
  }, [report]);

  useEffect(() => {
    const observer = new ResizeObserver(syncLayout);
    if (chromeRef.current) observer.observe(chromeRef.current);
    window.addEventListener("resize", syncLayout);
    const timer = window.setTimeout(syncLayout, 120);
    return () => { observer.disconnect(); window.removeEventListener("resize", syncLayout); window.clearTimeout(timer); };
  }, [syncLayout]);

  useEffect(() => {
    for (const timer of sleepTimers.current.values()) window.clearTimeout(timer);
    sleepTimers.current.clear();
    if (settings.sleepAfterMinutes === 0) return;
    const delayMinutes = settings.gamingMode ? Math.min(settings.sleepAfterMinutes, 1) : settings.sleepAfterMinutes;
    for (const tab of tabs) {
      if (tab.id === activeId || tab.pinned || tab.sleeping || tab.url === START) continue;
      const timer = window.setTimeout(() => {
        const current = tabsRef.current.find((item) => item.id === tab.id);
        if (!current || current.pinned || current.sleeping || activeIdRef.current === tab.id) return;
        void invoke<boolean>("discard_tab_if_inactive", { id: tab.id })
          .catch((reason) => report(`Hintergrundtab konnte nicht schlafen gelegt werden: ${String(reason)}`));
        sleepTimers.current.delete(tab.id);
      }, delayMinutes * 60_000);
      sleepTimers.current.set(tab.id, timer);
    }
    return () => { for (const timer of sleepTimers.current.values()) window.clearTimeout(timer); sleepTimers.current.clear(); };
  }, [activeId, tabs.map((tab) => `${tab.id}:${tab.pinned}:${tab.sleeping}:${tab.url === START}`).join("|"), settings.gamingMode, settings.sleepAfterMinutes, report]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.ctrlKey && !event.altKey && !event.metaKey) {
        if (event.key.toLowerCase() === "l") { event.preventDefault(); addressRef.current?.focus(); addressRef.current?.select(); }
        else if (event.key.toLowerCase() === "t") { event.preventDefault(); void openTab(); }
        else if (event.key.toLowerCase() === "w" && activeId) { event.preventDefault(); void closeTab(activeId); }
        else if (event.key.toLowerCase() === "r" && activeId) { event.preventDefault(); void invoke("reload_tab", { id: activeId }).catch((reason) => report(String(reason))); }
        else if (event.key === "-" || event.key === "Subtract") { event.preventDefault(); void setZoom((activeTab?.zoom ?? 1) - 0.1); }
        else if (["+", "=", "Add"].includes(event.key)) { event.preventDefault(); void setZoom((activeTab?.zoom ?? 1) + 0.1); }
        else if (event.key === "0") { event.preventDefault(); void setZoom(1); }
      }
      if (event.altKey && event.key === "ArrowLeft" && activeId) { event.preventDefault(); void invoke("history_move", { id: activeId, direction: "back" }).catch((reason) => report(String(reason))); }
      if (event.altKey && event.key === "ArrowRight" && activeId) { event.preventDefault(); void invoke("history_move", { id: activeId, direction: "forward" }).catch((reason) => report(String(reason))); }
      if (event.key === "Escape" && panel) { event.preventDefault(); void showPanel(null); }
    };
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey) return;
      event.preventDefault();
      void setZoom((activeTab?.zoom ?? 1) + (event.deltaY < 0 ? 0.05 : -0.05));
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("wheel", onWheel, { passive: false });
    return () => { window.removeEventListener("keydown", onKey); window.removeEventListener("wheel", onWheel); };
  }, [activeId, activeTab?.zoom, closeTab, openTab, panel, report, setZoom, showPanel]);

  const isStart = !activeTab || activeTab.url === START;
  const pageTitle = activeTab?.title || "Neuer Tab";
  const zoomPercent = Math.round((activeTab?.zoom ?? 1) * 100);

  const verifyExtensions = async () => {
    setCheckingExtensions(true);
    try {
      await invoke<ExtensionStatus[]>("initialize_extensions");
      await refreshExtensions();
    } catch (reason) {
      report(`Erweiterungen konnten nicht geprüft werden: ${String(reason)}`);
    } finally {
      setCheckingExtensions(false);
    }
  };

  const currentSectionLabel = SETTING_SECTIONS.find(([key]) => key === settingsSection)?.[1] ?? "Allgemein";
  const isBookmarked = Boolean(activeTab && bookmarks.some((bookmark) => bookmark.url === activeTab.url));

  return <main data-theme={settings.theme} data-accent={settings.accentColor} className={"app-shell" + (settings.reduceMotion ? " reduce-motion" : "") + (videoFullscreen ? " video-fullscreen" : "")}>
    <div ref={chromeRef} className="chrome">
      <div className="tabs-row">
        <button className="brand" type="button" onClick={() => void navigate(START)} aria-label="Veloce Startseite"><Logo/><span>Veloce</span></button>
        <div className="tabs-strip" role="tablist" aria-label="Browser-Tabs">
          {tabs.map((tab) => <div key={tab.id} className={"tab-card" + (tab.id === activeId ? " active" : "")} role="presentation">
            <button type="button" className="tab-select" role="tab" aria-selected={tab.id === activeId} title={tab.suspended ? `${tab.url} · Hintergrundtab pausiert` : tab.url} onClick={() => void activateTab(tab.id)}>
              <span className="tab-mark">{tab.sleeping ? <span className="sleep-dot"/> : tab.suspended ? <Pause size={11}/> : tab.url === START ? <Sparkles size={13}/> : hostOf(tab.url).slice(0, 1).toUpperCase()}</span>
              <span className="tab-label">{tab.title || "Neuer Tab"}</span>
              {tab.pinned && <LockKeyhole size={11} className="pin-indicator"/>}
            </button>
            <button type="button" className="tab-close" aria-label="Tab schließen" onClick={() => void closeTab(tab.id)}><X size={13}/></button>
          </div>)}
        </div>
        <button type="button" className="new-tab" onClick={() => void openTab()} title="Neuer Tab · Ctrl+T" aria-label="Neuer Tab"><Plus size={16}/></button>
      </div>
      <div className="toolbar-row">
        <div className="nav-group">
          <button type="button" aria-label="Zurück" title="Zurück · Alt+←" disabled={!activeTab || isStart} onClick={() => activeId && void invoke("history_move", { id: activeId, direction: "back" }).catch((reason) => report(String(reason)))}><ArrowLeft size={16}/></button>
          <button type="button" aria-label="Vorwärts" title="Vorwärts · Alt+→" disabled={!activeTab || isStart} onClick={() => activeId && void invoke("history_move", { id: activeId, direction: "forward" }).catch((reason) => report(String(reason)))}><ArrowRight size={16}/></button>
          <button type="button" aria-label="Neu laden" title="Neu laden · Ctrl+R" disabled={!activeTab || isStart} onClick={() => activeId && void invoke("reload_tab", { id: activeId }).catch((reason) => report(String(reason)))}><RefreshCw size={15}/></button>
          <button type="button" aria-label="Startseite" title="Startseite" onClick={() => void navigate(START)}><Home size={15}/></button>
        </div>
        <form className="omnibox" onSubmit={(event) => { event.preventDefault(); void navigate(); }}>
          <span className={"connection-light" + (activeTab?.url.startsWith("https:") ? " secure" : "")}/>
          <input ref={addressRef} value={address} onChange={(event) => setAddress(event.target.value)} onFocus={(event) => event.currentTarget.select()} placeholder="Adresse oder Suche eingeben" aria-label="Adresse oder Suche" />
          <span className="omnibox-shortcut">Ctrl + L</span>
          <button type="submit" className="submit-navigation" aria-label="Öffnen" title="Öffnen"><ArrowRight size={16}/></button>
        </form>
        <div className="toolbar-actions">
          <button type="button" className={"tool-button download-toggle" + (panel === "downloads" ? " selected" : "")} aria-label="Downloads anzeigen" title="Downloads" onClick={() => void showPanel(panel === "downloads" ? null : "downloads")}>
            <Download size={15}/>{downloads.some((item) => item.status === "downloading") && <i className="download-active-dot" aria-label="Download läuft"/>}
          </button>
          <button type="button" className="tool-button protection" aria-label="Tracker-Navigationsschutz" title="Tracker-Domainnavigation-Schutz" onClick={() => void showPanel("settings")}><ShieldCheck size={16}/></button>
          <button type="button" className={"tool-button bookmark-toggle" + (isBookmarked ? " selected" : "")} aria-label={isBookmarked ? "Lesezeichen entfernen" : "Lesezeichen speichern"} aria-pressed={isBookmarked} title={isBookmarked ? "Lesezeichen entfernen" : "Lesezeichen speichern"} disabled={!activeTab || isStart} onClick={() => void toggleBookmark()}><Bookmark size={15} fill={isBookmarked ? "currentColor" : "none"}/></button>
          <button type="button" className="tool-button" aria-label="Zoom verkleinern" title="Zoom verkleinern" onClick={() => void setZoom((activeTab?.zoom ?? 1) - 0.1)}><ZoomOut size={15}/></button>
          <button type="button" className="zoom-value" aria-label="Zoom zurücksetzen" title="Auf 100 % setzen" onClick={() => void setZoom(1)}>{zoomPercent}%</button>
          <button type="button" className="tool-button" aria-label="Zoom vergrößern" title="Zoom vergrößern" onClick={() => void setZoom((activeTab?.zoom ?? 1) + 0.1)}><ZoomIn size={15}/></button>
          <button type="button" className={"tool-button" + (panel === "more" ? " selected" : "")} aria-label="Weitere Optionen" title="Weitere Optionen" onClick={() => void showPanel(panel === "more" ? null : "more")}><MoreHorizontal size={17}/></button>
        </div>
      </div>
      {loading && <div className="loading-indicator" aria-label="Seite wird geöffnet"/>}
    </div>

    <section className={"page-surface" + (panel ? " panel-open" : "")}>
      {panel && <button type="button" className="panel-scrim" aria-label="Menü schließen" onClick={() => void showPanel(null)}/>}
      {isStart && <section className="start-page">
        <div className="hero-mark"><Logo/></div>
        <p className="overline">SCHNELL · FOKUSSIERT · DEINS</p>
        <h1>Dein Web. Ohne Umwege.</h1>
        <p className="hero-subtitle">Ein klarer Browser für Arbeit, Spiel und alles dazwischen.</p>
        <form className="start-search" onSubmit={(event) => { event.preventDefault(); void navigate(homeSearch); }}>
          <Search size={18}/><input value={homeSearch} onChange={(event) => setHomeSearch(event.target.value)} placeholder={`Mit ${settings.searchEngine === "duckduckgo" ? "DuckDuckGo" : settings.searchEngine[0].toUpperCase() + settings.searchEngine.slice(1)} suchen`} aria-label="Im Web suchen"/><button type="submit" aria-label="Suchen"><ArrowRight size={17}/></button>
        </form>
        <div className="quick-links">
          {shortcuts.map((item) => <div key={item.id} className="quick-link" draggable title="Zum Verschieben ziehen" onDragStart={(event) => { draggedShortcutId.current = item.id; event.dataTransfer.setData("text/plain", item.id); event.dataTransfer.effectAllowed = "move"; }} onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = "move"; }} onDrop={(event) => { event.preventDefault(); void moveShortcut(event.dataTransfer.getData("text/plain") || item.id); }}>
            <button className="quick-link-open" type="button" onClick={() => void navigate(item.url)}>
              {item.id === "youtube" ? <AudioLines size={16}/> : item.id === "brave-search" ? <Search size={16}/> : item.id === "wikipedia" ? <Globe2 size={16}/> : item.id === "github" ? <ExternalLink size={16}/> : <Globe2 size={16}/>}
              <span>{item.label}</span><ArrowRight size={13}/>
            </button>
            {item.id.startsWith("custom-") && <button className="quick-link-remove" type="button" aria-label={`${item.label} entfernen`} title="Schnellzugriff entfernen" onClick={() => void removeShortcut(item.id)}><X size={12}/></button>}
          </div>)}
          <button className="quick-link-add" type="button" onClick={() => setShortcutFormOpen((open) => !open)}><Plus size={15}/><span>Hinzufügen</span></button>
        </div>
        {shortcutFormOpen && <form className="shortcut-form" onSubmit={(event) => void addShortcut(event)}>
          <input value={shortcutLabel} onChange={(event) => setShortcutLabel(event.target.value)} maxLength={30} placeholder="Name" aria-label="Name des Schnellzugriffs" required/>
          <input value={shortcutUrl} onChange={(event) => setShortcutUrl(event.target.value)} placeholder="https://beispiel.de" aria-label="Adresse des Schnellzugriffs" required/>
          <button type="submit">Speichern</button><button type="button" onClick={() => setShortcutFormOpen(false)} aria-label="Abbrechen"><X size={14}/></button>
        </form>}
        <div className="start-footer"><span><i/> Native WebView · Tauri + Rust</span><span>Fokus statt Ballast</span></div>
      </section>}
      {!isStart && activeTab?.sleeping && <div className="sleeping-card"><div className="sleep-glyph"><Compass size={20}/></div><strong>{pageTitle} ruht gerade</strong><span>Beim Wechsel wird die Seite erneut geladen. Nicht gespeicherte Seitenzustände können verloren sein.</span><button type="button" onClick={() => void activateTab(activeTab.id)}>Seite laden <ArrowRight size={15}/></button></div>}
      {panel === "more" && <section className="floating-panel more-panel"><div className="panel-title"><span>VELOCE</span><button type="button" className="close-panel" onClick={() => void showPanel(null)} aria-label="Schließen"><X size={15}/></button></div>
        <button type="button" onClick={() => { const tab = activeTab; if (tab) void invoke<boolean>("toggle_pin", { id: tab.id }).then((pinned) => setTabs((current) => current.map((item) => item.id === tab.id ? { ...item, pinned } : item))).catch((reason) => report(String(reason))); }}><Bookmark size={16}/>{activeTab?.pinned ? "Tab lösen" : "Tab anheften"}</button>
        <button type="button" onClick={() => void showPanel("settings")}><Settings2 size={16}/>Einstellungen</button>
        <button type="button" onClick={() => void showPanel("downloads")}><Download size={16}/>Downloads</button>
        <button type="button" onClick={() => void showPanel("bookmarks")}><Bookmark size={16}/>Lesezeichen</button>
        <button type="button" onClick={openGooglePasswordManager}><LockKeyhole size={16}/>Google Passwortmanager</button>
        <div className="menu-divider"/>
        <button type="button" onClick={() => void openTab()}><Plus size={16}/>Neuer Tab <span className="shortcut-hint">Ctrl+T</span></button>
        <p className="menu-footnote">Browserdaten bleiben lokal in deinem Veloce-Profil.</p>
      </section>}
      {panel === "bookmarks" && <section className="floating-panel more-panel bookmarks-panel"><div className="panel-title"><span>LESEZEICHEN</span><button type="button" className="close-panel" onClick={() => void showPanel(null)} aria-label="Schließen"><X size={15}/></button></div>
        {bookmarks.length ? bookmarks.map((bookmark) => <div className="bookmark-entry" key={bookmark.id}><button type="button" className="bookmark-open" title={bookmark.url} onClick={() => void navigate(bookmark.url)}><Bookmark size={14}/><span><strong>{bookmark.title}</strong><small>{hostOf(bookmark.url)}</small></span></button><button type="button" className="bookmark-remove" aria-label={`${bookmark.title} entfernen`} title="Lesezeichen entfernen" onClick={() => { void invoke<BookmarkEntry[]>("set_bookmark", { url: bookmark.url, title: bookmark.title, saved: false }).then(setBookmarks).catch((reason) => report(`Lesezeichen konnte nicht entfernt werden: ${String(reason)}`)); }}><X size={13}/></button></div>) : <p className="menu-footnote">Noch keine Lesezeichen. Speichere eine Seite über das Lesezeichen-Symbol in der Browserleiste.</p>}
      </section>}
      {panel === "downloads" && <section className="floating-panel more-panel downloads-panel"><div className="panel-title"><span>DOWNLOADS</span><div className="download-panel-actions"><button type="button" className="clear-downloads" disabled={!downloads.length} onClick={() => { void invoke("clear_download_history").catch((reason) => report(`Downloadliste konnte nicht geleert werden: ${String(reason)}`)); }} title="Einträge entfernen, heruntergeladene Dateien bleiben erhalten">Liste leeren</button><button type="button" className="close-panel" onClick={() => void showPanel(null)} aria-label="Schließen"><X size={15}/></button></div></div>
        {downloads.length ? <div className="download-list">{[...downloads].reverse().map((item) => <button type="button" className="download-entry" key={item.id} title={item.status === "completed" ? "Doppelklicken, um die Datei mit der Standard-App zu öffnen" : item.path} disabled={item.status !== "completed"} onDoubleClick={() => { void invoke("open_downloaded_file", { id: item.id }).then(() => showToast("Öffnen an System-App übergeben")).catch((reason) => report(`Download konnte nicht geöffnet werden: ${String(reason)}`)); }} aria-label={`${item.fileName}. ${item.status === "completed" ? "Doppelklicken zum Öffnen." : "Download nicht abgeschlossen."}`}>
          <span className={"download-state-icon " + item.status}>{item.status === "downloading" ? <RefreshCw size={14} className="spin"/> : item.status === "completed" ? <Check size={14}/> : <X size={14}/>}</span>
          <span className="download-entry-copy"><strong>{item.fileName}</strong><small>{item.status === "downloading" ? "Wird heruntergeladen … · Fortschritt nicht von WebView2 bereitgestellt" : item.status === "completed" ? "Abgeschlossen · Doppelklick zum Öffnen" : "Abgebrochen oder fehlgeschlagen"}</small><small className="download-entry-path">{item.path}</small><small>{new Date(item.startedAtMs).toLocaleString()}</small></span>
        </button>)}</div> : <p className="menu-footnote">Noch keine Downloads. Heruntergeladene Dateien werden im Downloadordner gespeichert.</p>}
        <p className="menu-footnote">Download-Einträge liegen lokal im Veloce-Profil. „Liste leeren“ löscht nur die Einträge, nicht die Dateien.</p>
      </section>}
      {panel === "settings" && <section className="floating-panel settings-panel">
        <div className="panel-title"><span>EINSTELLUNGEN · {currentSectionLabel.toUpperCase()}</span><button type="button" className="close-panel" onClick={() => void showPanel(null)} aria-label="Schließen"><X size={15}/></button></div>
        <div className="settings-layout">
          <nav className="settings-nav" aria-label="Einstellungskategorien">
            {SETTING_SECTIONS.map(([key, label]) => <button type="button" key={key} className={settingsSection === key ? "active" : ""} onClick={() => setSettingsSection(key)}>{label}</button>)}
          </nav>
          <div className="settings-content">
            {settingsSection === "general" && <>
              <h2>Allgemein</h2>
              <label className="setting-row"><span><strong>Suchmaschine</strong><small>Für Adresszeile und Startseite.</small></span><select value={settings.searchEngine} onChange={(event) => changeSettings({searchEngine:event.target.value})}><option value="duckduckgo">DuckDuckGo</option><option value="brave">Brave Search</option><option value="google">Google</option><option value="bing">Bing</option></select></label>
              <label className="setting-row"><span><strong>Passwörter speichern und ausfüllen</strong><small>WebView2 zeigt beim Anmelden Speichern/Aktualisieren an und füllt gespeicherte Logins auf passenden Seiten aus. Gilt für das Veloce-Profil.</small></span><input type="checkbox" checked={settings.passwordAutosave} onChange={(event) => changeSettings({passwordAutosave:event.target.checked})}/></label>
              <button type="button" className="extension-check" onClick={openGooglePasswordManager}><LockKeyhole size={14}/>Google Passwortmanager im Web öffnen</button>
              <div className="restart-note">Google-Konto-Synchronisierung ist nicht in Veloce integriert. Google-Passwörter lassen sich auf passwords.google.com verwalten; WebView2 speichert und synchronisiert sie nicht automatisch mit diesem Konto.</div>
              <label className="setting-row"><span><strong>Letzte Tabs beim Start wiederherstellen</strong><small>Speichert nur Tab-Adressen und stellt die Seiten beim nächsten Start erneut her.</small></span><input type="checkbox" checked={settings.restoreSession} onChange={(event) => changeSettings({restoreSession:event.target.checked})}/></label>
              <button type="button" className="extension-check" onClick={() => void configureDefaultBrowser()}><Globe2 size={14}/>Als Standardbrowser festlegen</button>
              <div className="restart-note">Veloce wird bei der Installation für HTTP- und HTTPS-Links registriert. Diese Schaltfläche öffnet direkt die Windows-Seite für Veloce. Wähle dort „Als Standard festlegen“ oder Veloce im Windows-Dialog „App zum Öffnen dieses Links auswählen“. Windows bestätigt die Auswahl; Veloce ändert sie nicht selbst.</div>
              <div className="capability-note"><Compass size={16}/><span><strong>Lokales Profil</strong><small>Veloce speichert Einstellungen und Sitzung im lokalen App-Datenordner. Das Profil kann unabhängig vom Browserfenster weiter WebView2-Prozesse verwenden.</small></span></div>
            </>}
            {settingsSection === "appearance" && <>
              <h2>Aussehen</h2>
              <label className="setting-row"><span><strong>Oberflächenstil</strong><small>Graphit nutzt dunkle Grautöne; OLED setzt auf nahezu reines Schwarz.</small></span><select value={settings.theme} onChange={(event) => changeSettings({theme:event.target.value as SettingsData["theme"]})}><option value="graphite">Graphit</option><option value="oled">OLED-Schwarz</option></select></label>
              <label className="setting-row"><span><strong>Akzentfarbe</strong><small>Ändert Hervorhebungen und Fokusfarben in Veloce.</small></span><select value={settings.accentColor} onChange={(event) => changeSettings({accentColor:event.target.value as SettingsData["accentColor"]})}><option value="violet">Violett</option><option value="cyan">Cyan</option><option value="orange">Orange</option><option value="green">Grün</option></select></label>
            </>}
            {settingsSection === "tabs" && <>
              <h2>Tabs</h2>
              <label className="setting-row"><span><strong>Hintergrundtabs nach Ruhezeit verwerfen</strong><small>Schließt eine inaktive WebView. Beim Wechsel wird die Adresse neu geladen; temporärer Seitenzustand kann verloren gehen. Angeheftete und aktive Tabs bleiben erhalten.</small></span><select value={settings.sleepAfterMinutes} onChange={(event) => changeSettings({sleepAfterMinutes:Number(event.target.value)})}><option value={0}>Aus</option><option value={1}>1 Minute</option><option value={2}>2 Minuten</option><option value={3}>3 Minuten</option><option value={5}>5 Minuten</option><option value={10}>10 Minuten</option><option value={15}>15 Minuten</option><option value={30}>30 Minuten</option><option value={60}>60 Minuten</option></select></label>
              <p className="settings-explainer">WebView2 bietet keine universelle, sichere Pause für beliebige Seitenskripte. Veloce verwirft daher nur die native Ansicht nach Ablauf der Ruhezeit.</p>
            </>}
            {settingsSection === "privacy" && <>
              <h2>Datenschutz</h2>
              <label className="setting-row"><span><strong>Tracker-Domainnavigation blockieren</strong><small>Blockiert bekannte Tracker nur, wenn sie als Hauptadresse geöffnet werden. Unterressourcen einer Webseite werden damit nicht gefiltert.</small></span><input type="checkbox" checked={settings.trackingProtection} onChange={(event) => changeSettings({trackingProtection:event.target.checked})}/></label>
              <p className="settings-explainer">Webseiten und Erweiterungen können eigene Netzwerkverbindungen verwenden. Diese Option ist kein vollständiger Inhaltsblocker.</p>
            </>}
            {settingsSection === "performance" && <>
              <h2>Leistung</h2>
              <label className="setting-row"><span><strong>Leistungsprofil</strong><small>Passt Hintergrundtab-Ruhe und WebView-Speicherziel an. Kein Profil begrenzt Webseiten auf feste RAM-Werte.</small></span><select value={settings.performanceProfile} onChange={(event) => event.target.value !== "custom" && chooseProfile(event.target.value as Exclude<SettingsData["performanceProfile"], "custom">)}><option value="balanced">Ausgewogen</option><option value="memory_saver">Ressourcenschonend</option><option value="smooth_video">Flüssige Wiedergabe</option><option value="gaming">Gaming</option><option value="custom">Benutzerdefiniert</option></select></label>
              <label className="setting-row"><span><strong>Stille Hintergrundtabs pausieren</strong><small>Verbirgt inaktive Tabs sofort und nutzt WebView2 TrySuspend plus das niedrige Speicherziel, wenn weder Video noch Audio wiedergegeben wird. Beim Wechsel wird die Seite vor dem Anzeigen fortgesetzt. WebView2 kann eine Pause ablehnen.</small></span><input type="checkbox" checked={settings.memorySaver} onChange={(event) => changeSettings({memorySaver:event.target.checked})}/></label>
              <p className="settings-explainer">TrySuspend pausiert Seitenskripte, Timer und Animationen bestmöglich. Bereits laufende Netzwerkanfragen können trotzdem fertiglaufen. WebView2 ordnet Renderer nicht zuverlässig einzelnen Tabs zu; Veloce senkt deshalb keine Windows-Prozesspriorität, die auch aktive Seiten ausbremsen könnte.</p>
              <label className="setting-row"><span><strong>Browseroberflächen-Animationen reduzieren</strong><small>Schaltet Animationen und Übergänge der Veloce-Oberfläche weitgehend ab. Animationen auf Webseiten bleiben unverändert.</small></span><input type="checkbox" checked={settings.reduceMotion} onChange={(event) => changeSettings({reduceMotion:event.target.checked})}/></label>
              <label className="setting-row"><span><strong>Hardwarebeschleunigung</strong><small>Aktiv und fest eingeschaltet für Video, WebGL und flüssige Darstellung. Veloce setzt kein GPU-Deaktivierungsflag; WebView2 wählt den passenden GPU-Pfad.</small></span><input type="checkbox" checked={settings.hardwareAcceleration} disabled aria-label="Hardwarebeschleunigung ist aktiv"/></label>
              <div className="restart-note">Der WebView2-Datenträgercache bleibt im gemeinsamen Browserprofil aktiv. Veloce ändert keine Windows-Energie- oder Grafikeinstellungen und nutzt keine experimentellen Browserflags.</div>
              <button type="button" className="extension-check" disabled={measuringMemory} onClick={() => void measureMemory()}><RefreshCw size={14} className={measuringMemory ? "spin" : ""}/>{measuringMemory ? "WebView2-Prozesse werden gemessen …" : "Verbrauch jetzt messen"}</button>
              {memorySnapshot && <div className="memory-card">
                <div><strong>Veloce + zugehörige WebView2-Prozesse</strong><span>Working Set {mib(memorySnapshot.workingSetBytes)} · privates Commit {mib(memorySnapshot.privateCommitBytes)}</span></div>
                <div><strong>{memorySnapshot.processCount} Prozesse erfasst</strong><span>{memorySnapshot.webviewProcessCount} WebView2-Prozesse gemeldet · {memorySnapshot.inaccessibleProcessCount} nicht messbar</span></div>
                <small>Momentaufnahme. Working-Set-Summen können gemeinsam genutzte Seiten enthalten; GPU-Speicher ist nicht enthalten. Einzelne Tabs/Erweiterungen werden von WebView2 nicht zuverlässig einem eigenen Prozess zugeordnet.</small>
                <details><summary>Prozessdetails</summary>{memorySnapshot.processes.map((process) => <div className="memory-process" key={process.pid}><span>{process.kind} · PID {process.pid}</span><span>{mib(process.workingSetBytes)} / {mib(process.privateCommitBytes)}</span></div>)}</details>
              </div>}
            </>}
            {settingsSection === "video" && <>
              <h2>Video & Medien</h2>
              <label className="setting-row"><span><strong>Video an der letzten Stelle fortsetzen</strong><small>Speichert Videopositionen lokal auf diesem PC und stellt sie beim nächsten Öffnen wieder her. Wenn aus, werden keine neuen Positionen erfasst oder wiederhergestellt. War das Video in Wiedergabe, versucht Veloce den automatischen Start; Website- und Autoplay-Regeln können das verhindern.</small></span><input type="checkbox" checked={settings.resumeVideoPlayback} onChange={(event) => changeSettings({resumeVideoPlayback:event.target.checked})}/></label>
              <label className="setting-row"><span><strong>YouTube-Hinweis „Kommt es zu Unterbrechungen?“ ausblenden</strong><small>Verbirgt nur diese Hinweisblase. Werbe- und SponsorBlocker, Videoanfragen und Wiedergabe bleiben unverändert. Das Ausblenden beschleunigt den Start nicht.</small></span><input type="checkbox" checked={settings.youtubeHideInterruptions} onChange={(event) => changeSettings({youtubeHideInterruptions:event.target.checked})}/></label>
              <label className="setting-row"><span><strong>YouTube-Bild auf Playerfläche zuschneiden</strong><small>Füllt den Videobereich durch Beschnitt. Bildränder können fehlen; originale Balken bleiben möglich. Betrifft weder Qualität noch Netzwerkrequests.</small></span><input type="checkbox" checked={settings.youtubeVideoFill} onChange={(event) => changeSettings({youtubeVideoFill:event.target.checked})}/></label>
              <div className="youtube-diagnostics">
                <div className="youtube-diagnostics-heading"><strong>YouTube-Ladezeiten</strong><button type="button" onClick={() => setYoutubeMetrics([])} disabled={!youtubeMetrics.length}>Leeren</button></div>
                <p>Lokale Momentaufnahme: Klick bis Player, Metadaten, Startfähigkeit und erstes Videobild. Langsame Ressourcen werden nur nach Host und Typ erfasst. Keine Videotitel, Suchbegriffe oder vollständigen URLs; nichts wird übertragen oder dauerhaft gespeichert.</p>
                {youtubeMetrics.length ? <div className="youtube-metric-list">{youtubeMetrics.slice(0, 12).map((metric, index) => <div className="youtube-metric-row" key={`${metric.id}-${metric.attemptId}-${metric.phase}-${index}`}>
                  <span>{YOUTUBE_METRIC_LABELS[metric.phase] ?? metric.phase}<small>{metric.source === "click" ? "ab Videoklick" : "ab Seitenstart"}</small></span>
                  <strong>{metric.phase === "slow-resource" ? `${metric.resourceHost ?? "Ressource"} · ${metric.resourceDurationMs ?? 0} ms${metric.responseStatus ? ` · HTTP ${metric.responseStatus}` : " · Status nicht verfügbar"}` : metric.phase === "buffer-resume" ? `${metric.stallMs ?? 0} ms Pufferpause` : `${metric.elapsedMs} ms`}</strong>
                </div>)}</div> : <div className="youtube-metrics-empty">Noch keine YouTube-Messwerte. YouTube öffnen und ein Video anklicken; die Messung erscheint hier.</div>}
              </div>
              <div className="capability-note"><AudioLines size={16}/><span><strong>Website-Player steuert Qualität und Ton</strong><small>Veloce setzt keine universelle Videoauflösung, Bildrate oder Lautstärkenormalisierung durch: WebView2 und die jeweilige Website stellen dafür keine verlässliche allgemeine Steuerung bereit. YouTube-Qualität bitte im Player wählen; Änderungen hier verändern keine Videoanfragen.</small></span></div>
            </>}
            {settingsSection === "extensions" && <>
              <h2>Erweiterungen</h2>
              <p className="settings-explainer">Installations-, Aktivierungs- und Fehlerstatus werden direkt von WebView2 gelesen. Chrome-Web-Store-Installationen über dessen Webseite werden nicht vorgetäuscht.</p>
              <button type="button" className="extension-check" onClick={() => void openTab("https://chromewebstore.google.com/")}><ExternalLink size={14}/>Chrome Web Store durchsuchen</button>
              <p className="settings-explainer">Der Store kann zum Stöbern geöffnet werden. WebView2 akzeptiert Erweiterungen aus entpackten lokalen Ordnern; der Store-Button „Zu Chrome hinzufügen“ installiert nicht in Veloce. Prüfe die Runtime und Manifest-Berechtigungen vor dem Hinzufügen.</p>
              <div className="extension-list">
                {(extensionStatuses.length ? extensionStatuses : [
                  {key:"ublock-origin-lite",id:"",name:"uBlock Origin Lite",bundledVersion:"2026.1116.1650",installed:false,enabled:false,removable:false,detail:"Status noch nicht geprüft"},
                  {key:"sponsorblock",id:"",name:"SponsorBlock",bundledVersion:"6.1.7",installed:false,enabled:false,removable:false,detail:"Status noch nicht geprüft"},
                ]).map((extension) => <div className="extension-card" key={extension.key}>
                  <div className="extension-card-heading"><strong>{extension.name}</strong><span className={"extension-badge" + (extension.enabled ? " active" : "") + (!extension.installed && !extension.detail.toLowerCase().includes("noch nicht") && !extension.detail.toLowerCase().includes("ausstehend") ? " failed" : "")}>{extension.enabled ? "Aktiv" : extension.installed ? "Deaktiviert" : "Nicht installiert"}</span></div>
                  {extension.bundledVersion && <small>{extension.key.startsWith("custom:") ? `Version ${extension.bundledVersion}` : `Mitgelieferte Version ${extension.bundledVersion}`}</small>}<small>{extension.detail}</small>
                  <div className="extension-actions"><button type="button" disabled={!extension.installed} onClick={() => void toggleExtension(extension)}>{extension.enabled ? "Deaktivieren" : "Aktivieren"}</button>{extension.removable && <button type="button" className="danger-action" onClick={() => void removeCustomExtension(extension)}>Entfernen</button>}</div>
                </div>)}
              </div>
              <button type="button" className="extension-check" disabled={checkingExtensions} onClick={() => void verifyExtensions()}><RefreshCw size={14} className={checkingExtensions ? "spin" : ""}/>{checkingExtensions ? "WebView2 prüft …" : "Gebündelte Erweiterungen prüfen / installieren"}</button>
              <h3>Eigene Erweiterung hinzufügen</h3>
              <p className="settings-explainer">Wähle einen entpackten lokalen Erweiterungsordner mit manifest.json. WebView2 unterstützt die Installation über sein Profil; nicht alle Chrome-APIs sind verfügbar.</p>
              <div className="extension-import"><input aria-label="Pfad des Erweiterungsordners" value={extensionFolder} onChange={(event) => { setExtensionFolder(event.target.value); setExtensionPreview(null); }} placeholder="C:\\Pfad\\zum\\Erweiterungsordner"/><button type="button" disabled={!extensionFolder.trim()} onClick={() => void inspectExtension()}>Prüfen</button></div>
              {extensionPreview && <div className="manifest-preview"><strong>{extensionPreview.name} · {extensionPreview.version} · Manifest {extensionPreview.manifestVersion}</strong><small>Berechtigungen: {extensionPreview.permissions.join(", ") || "keine deklariert"}</small><small>Webseitenzugriff: {extensionPreview.hostPermissions.join(", ") || "keine Host-Muster deklariert"}</small><button type="button" disabled={installingExtension} onClick={() => void installCustomExtension()}>{installingExtension ? "WebView2 installiert …" : "Nach Prüfung hinzufügen"}</button></div>}
              <div className="capability-note"><ShieldCheck size={16}/><span><strong>WebView2-Unterstützung</strong><small>Eigene Erweiterungen benötigen eine unterstützte WebView2-Runtime (Erweiterungs-API ab Runtime 120). Veloce kann Chrome-Web-Store-„Hinzufügen“-Buttons nicht als Chrome installieren. Erweiterungen erhalten nur die Berechtigungen ihres Manifests; installiere nur vertrauenswürdige Dateien.</small></span></div>
            </>}
          </div>
        </div>
      </section>}
    </section>
    {error && <div className="error-banner" role="alert"><span>{error}</span><button type="button" onClick={() => setError("")} aria-label="Fehlermeldung schließen"><X size={14}/></button></div>}
    {toast && <div className="toast" role="status"><Check size={14}/>{toast}</div>}
  </main>;
}

