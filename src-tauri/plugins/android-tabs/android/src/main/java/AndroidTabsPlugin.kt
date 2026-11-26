package com.veloce.browser.androidtabs

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.app.role.RoleManager
import android.provider.OpenableColumns
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.webkit.CookieManager
import android.webkit.DownloadListener
import android.webkit.PermissionRequest
import android.webkit.URLUtil
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong
import android.app.DownloadManager
import android.webkit.MimeTypeMap

@InvokeArg
class CreateTabArgs {
    lateinit var id: String
    lateinit var url: String
    var width: Double = 360.0
    var height: Double = 800.0
    var chromeHeight: Double = 96.0
    var chromePosition: String = "top"
    var zoom: Double = 1.0
    var trackingProtection: Boolean = true
    var initScript: String = ""
}

@InvokeArg
class ActivateTabArgs {
    lateinit var id: String
    var isStart: Boolean = false
}

@InvokeArg
class TabIdArgs { lateinit var id: String }

@InvokeArg
class NavigateArgs {
    lateinit var id: String
    lateinit var url: String
}

@InvokeArg
class HistoryArgs {
    lateinit var id: String
    lateinit var direction: String
}

@InvokeArg
class ZoomArgs {
    lateinit var id: String
    var zoom: Double = 1.0
}

@InvokeArg
class EvalScriptArgs {
    lateinit var id: String
    lateinit var script: String
}

@InvokeArg
class LayoutArgs {
    var width: Double = 360.0
    var height: Double = 800.0
    var chromeHeight: Double = 96.0
    var chromePosition: String = "top"
}

@InvokeArg
class OverlayArgs { var enabled: Boolean = false }

@InvokeArg
class PathArgs { lateinit var path: String }

@InvokeArg
class SharePageArgs {
    lateinit var url: String
    var title: String = "Veloce"
}

@InvokeArg
class TrackingProtectionArgs { var enabled: Boolean = true }

@TauriPlugin
class AndroidTabsPlugin(private val activity: Activity) : Plugin(activity) {
    private data class DownloadRecord(val fileName: String, val startedAtMs: Long)

    private val tabs = LinkedHashMap<String, WebView>()
    private val trackingProtection = ConcurrentHashMap<String, Boolean>()
    private val tabZoom = ConcurrentHashMap<String, Float>()
    private val downloadRecords = ConcurrentHashMap<Long, DownloadRecord>()
    private val nextViewId = AtomicLong(1)
    private var shell: WebView? = null
    private var root: FrameLayout? = null
    private var pageLayer: FrameLayout? = null
    private var activeTabId: String? = null
    private var activeIsStart = true
    private var overlay = false
    private var viewportWidth = 360
    private var viewportHeight = 800
    private var chromeHeight = 96
    private var chromePosition = "top"
    private var fullScreenView: View? = null
    private var fullScreenCallback: WebChromeClient.CustomViewCallback? = null
    private var filePicker: ActivityResultLauncher<Intent>? = null
    private var pendingFileCallback: ValueCallback<Array<Uri>>? = null
    private var downloadReceiverRegistered = false
    private var pendingExternalUrl: String? = null

    private val downloadReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action != DownloadManager.ACTION_DOWNLOAD_COMPLETE) return
            val downloadId = intent.getLongExtra(DownloadManager.EXTRA_DOWNLOAD_ID, -1L)
            val record = downloadRecords[downloadId] ?: return
            val manager = activity.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
            val query = DownloadManager.Query().setFilterById(downloadId)
            manager.query(query)?.use { cursor ->
                if (!cursor.moveToFirst()) return
                val status = cursor.getInt(cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS))
                val error = cursor.getInt(cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_REASON))
                val uri = manager.getUriForDownloadedFile(downloadId)
                val payload = JSObject().apply {
                    put("id", "android-$downloadId")
                    put("fileName", record.fileName)
                    put("path", uri?.toString() ?: "")
                    put("startedAtMs", record.startedAtMs)
                    put("status", if (status == DownloadManager.STATUS_SUCCESSFUL) "completed" else "failed")
                    if (status != DownloadManager.STATUS_SUCCESSFUL) put("error", "Android-DownloadManager Fehler $error")
                }
                trigger("download", payload)
            }
            downloadRecords.remove(downloadId)
        }
    }

    override fun load(webView: WebView) {
        super.load(webView)
        shell = webView
        installLayout(webView)
        registerFilePicker()
        registerDownloadReceiver()
        activity.intent?.let(::handleExternalIntent)
        if (activity is AppCompatActivity) {
            activity.onBackPressedDispatcher.addCallback(activity, object : OnBackPressedCallback(false) {
                override fun handleOnBackPressed() {
                    if (fullScreenView != null) {
                        hideVideoFullscreen()
                    } else {
                        val current = activeTabId?.let { tabs[it] }
                        if (current?.canGoBack() == true) {
                            current.goBack()
                            updateBackHandler()
                        } else {
                            isEnabled = false
                            activity.onBackPressedDispatcher.onBackPressed()
                            isEnabled = true
                        }
                    }
                }
            }.also { backCallback = it })
        }
    }

    private var backCallback: OnBackPressedCallback? = null

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleExternalIntent(intent)
    }

    private fun handleExternalIntent(intent: Intent) {
        if (intent.action != Intent.ACTION_VIEW) return
        val uri = intent.data ?: return
        if (uri.scheme != "http" && uri.scheme != "https") return
        if (uri.userInfo != null || uri.host.isNullOrBlank()) return
        val url = uri.toString()
        if (hasListener("external-link")) {
            trigger("external-link", JSObject().apply { put("url", url) })
        } else {
            pendingExternalUrl = url
        }
    }

    private fun installLayout(webView: WebView) {
        val parent = webView.parent as? ViewGroup
            ?: throw IllegalStateException("Tauri-Oberfläche hat keinen nativen Android-Elternbereich.")
        val index = parent.indexOfChild(webView)
        val originalParams = webView.layoutParams
        parent.removeView(webView)

        val container = FrameLayout(activity).apply { setBackgroundColor(Color.rgb(16, 17, 20)) }
        val pages = FrameLayout(activity).apply { isClickable = false }
        container.addView(pages, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        container.addView(webView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        parent.addView(container, index.coerceAtLeast(0), originalParams)
        root = container
        pageLayer = pages
    }

    private fun registerFilePicker() {
        val component = activity as? AppCompatActivity ?: return
        filePicker = component.registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
            val callback = pendingFileCallback ?: return@registerForActivityResult
            pendingFileCallback = null
            callback.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(result.resultCode, result.data))
        }
    }

    private fun registerDownloadReceiver() {
        if (downloadReceiverRegistered) return
        val filter = IntentFilter(DownloadManager.ACTION_DOWNLOAD_COMPLETE)
        if (Build.VERSION.SDK_INT >= 33) {
            activity.registerReceiver(downloadReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("DEPRECATION")
            activity.registerReceiver(downloadReceiver, filter)
        }
        downloadReceiverRegistered = true
    }

    private fun result(invoke: Invoke, action: () -> Unit) {
        activity.runOnUiThread {
            try {
                action()
                invoke.resolve(JSObject().apply { put("ok", true) })
            } catch (error: Throwable) {
                invoke.reject(error.message ?: "Android-Browseraktion fehlgeschlagen.")
            }
        }
    }

    @Command
    fun createTab(invoke: Invoke) {
        val args = invoke.parseArgs(CreateTabArgs::class.java)
        result(invoke) {
            require(args.id.isNotBlank()) { "Tab-ID fehlt." }
            require(args.url.startsWith("https://") || args.url.startsWith("http://")) { "Nur HTTP- und HTTPS-Adressen werden geöffnet." }
            if (tabs.containsKey(args.id)) return@result
            val density = activity.resources.displayMetrics.density
            viewportWidth = (args.width * density).toInt().coerceAtLeast(200)
            viewportHeight = (args.height * density).toInt().coerceAtLeast(200)
            chromeHeight = (args.chromeHeight * density).toInt().coerceIn(0, (viewportHeight * 0.6f).toInt())
            chromePosition = if (args.chromePosition == "bottom") "bottom" else "top"
            val id = nextViewId.getAndIncrement().toInt()
            val view = WebView(activity)
            view.id = id
            view.settings.apply {
                javaScriptEnabled = true
                domStorageEnabled = true
                databaseEnabled = true
                setSupportZoom(true)
                builtInZoomControls = false
                displayZoomControls = false
                mediaPlaybackRequiresUserGesture = false
                javaScriptCanOpenWindowsAutomatically = true
                setSupportMultipleWindows(true)
                allowFileAccess = false
                allowContentAccess = true
                mixedContentMode = android.webkit.WebSettings.MIXED_CONTENT_NEVER_ALLOW
                cacheMode = android.webkit.WebSettings.LOAD_DEFAULT
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) safeBrowsingEnabled = true
            }
            view.setBackgroundColor(Color.WHITE)
            if (args.initScript.isNotBlank() && WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
                WebViewCompat.addDocumentStartJavaScript(view, args.initScript, setOf("*"))
            }
            view.setInitialScale((args.zoom.coerceIn(0.5, 3.0) * 100).toInt())
            CookieManager.getInstance().setAcceptCookie(true)
            trackingProtection[args.id] = args.trackingProtection
            tabZoom[args.id] = args.zoom.toFloat().coerceIn(0.5f, 3.0f)
            val tabId = args.id
            view.webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                    val uri = request.url
                    val scheme = uri.scheme?.lowercase().orEmpty()
                    if (scheme == "http" || scheme == "https" || scheme == "about") {
                        val host = uri.host?.lowercase().orEmpty()
                        if (trackingProtection[tabId] == true && isTrackingHost(host)) {
                            trigger("navigation-blocked", JSObject().apply { put("id", tabId); put("host", host) })
                            return true
                        }
                        return false
                    }
                    if (scheme == "mailto" || scheme == "tel") {
                        try { activity.startActivity(Intent(Intent.ACTION_VIEW, uri)) } catch (_: ActivityNotFoundException) { }
                    }
                    return true
                }

                override fun onPageStarted(view: WebView, url: String, favicon: android.graphics.Bitmap?) {
                    sendPageState(tabId, view, loading = true)
                }

                override fun onPageFinished(view: WebView, url: String) {
                    sendPageState(tabId, view, loading = false)
                    updateBackHandler()
                }

                override fun onReceivedError(view: WebView, request: WebResourceRequest, error: android.webkit.WebResourceError) {
                    if (request.isForMainFrame) {
                        val payload = JSObject().apply {
                            put("id", tabId); put("url", request.url.toString()); put("loading", false)
                            put("error", "Webseite konnte nicht geladen werden: ${error.description}")
                        }
                        trigger("page-state", payload)
                    }
                }
            }
            view.webChromeClient = object : WebChromeClient() {
                override fun onReceivedTitle(view: WebView, title: String?) { sendPageState(tabId, view, loading = false) }

                override fun onShowFileChooser(webView: WebView, filePathCallback: ValueCallback<Array<Uri>>, fileChooserParams: FileChooserParams): Boolean {
                    val launcher = filePicker ?: return false
                    pendingFileCallback?.onReceiveValue(null)
                    pendingFileCallback = filePathCallback
                    return try {
                        launcher.launch(fileChooserParams.createIntent())
                        true
                    } catch (_: Throwable) {
                        pendingFileCallback = null
                        filePathCallback.onReceiveValue(null)
                        false
                    }
                }

                override fun onCreateWindow(view: WebView, isDialog: Boolean, isUserGesture: Boolean, resultMsg: android.os.Message): Boolean {
                    if (!isUserGesture) return false
                    val popup = WebView(activity)
                    popup.settings.javaScriptEnabled = true
                    popup.webViewClient = object : WebViewClient() {
                        override fun onPageStarted(popupView: WebView, url: String, favicon: android.graphics.Bitmap?) {
                            if (url.startsWith("https://") || url.startsWith("http://")) {
                                trigger("new-tab", JSObject().apply { put("url", url) })
                            }
                            popupView.post { popupView.stopLoading(); popupView.destroy() }
                        }
                    }
                    (resultMsg.obj as? WebView.WebViewTransport)?.webView = popup
                    resultMsg.sendToTarget()
                    return true
                }

                override fun onShowCustomView(view: View, callback: CustomViewCallback) {
                    showVideoFullscreen(view, callback)
                }

                override fun onHideCustomView() { hideVideoFullscreen() }

                override fun onPermissionRequest(request: PermissionRequest) {
                    // Camera/microphone are denied until a dedicated in-app permission flow exists.
                    request.deny()
                }
            }
            view.setDownloadListener(DownloadListener { url, userAgent, contentDisposition, mimeType, _ ->
                startDownload(url, userAgent, contentDisposition, mimeType)
            })
            pageLayer?.addView(view, pageLayoutParams()) ?: throw IllegalStateException("Android-Seitenebene ist nicht bereit.")
            tabs[tabId] = view
            view.setInitialScale((args.zoom.coerceIn(0.5, 3.0) * 100).toInt())
            view.loadUrl(args.url)
            applyLayout()
        }
    }

    @Command
    fun activateTab(invoke: Invoke) {
        val args = invoke.parseArgs(ActivateTabArgs::class.java)
        result(invoke) {
            activeTabId = args.id
            activeIsStart = args.isStart
            applyLayout()
            tabs[args.id]?.requestFocus()
            if (args.isStart) shell?.requestFocus()
            updateBackHandler()
        }
    }

    @Command
    fun closeTab(invoke: Invoke) {
        val args = invoke.parseArgs(TabIdArgs::class.java)
        result(invoke) {
            val view = tabs.remove(args.id)
            trackingProtection.remove(args.id)
            tabZoom.remove(args.id)
            if (view != null) {
                pageLayer?.removeView(view)
                view.stopLoading()
                view.destroy()
            }
            if (activeTabId == args.id) activeTabId = null
            applyLayout()
            updateBackHandler()
        }
    }

    @Command
    fun navigate(invoke: Invoke) {
        val args = invoke.parseArgs(NavigateArgs::class.java)
        result(invoke) {
            val url = Uri.parse(args.url)
            require(url.scheme == "http" || url.scheme == "https") { "Nur HTTP- und HTTPS-Adressen werden geöffnet." }
            val view = tabs[args.id] ?: throw IllegalStateException("Dieser Android-Tab ist nicht geladen.")
            view.loadUrl(args.url)
        }
    }

    @Command
    fun historyMove(invoke: Invoke) {
        val args = invoke.parseArgs(HistoryArgs::class.java)
        result(invoke) {
            val view = tabs[args.id] ?: throw IllegalStateException("Dieser Android-Tab ist nicht geladen.")
            when (args.direction) {
                "back" -> if (view.canGoBack()) view.goBack() else throw IllegalStateException("Keine vorherige Seite vorhanden.")
                "forward" -> if (view.canGoForward()) view.goForward() else throw IllegalStateException("Keine nächste Seite vorhanden.")
                else -> throw IllegalArgumentException("Unbekannte Verlaufsrichtung.")
            }
            updateBackHandler()
        }
    }

    @Command
    fun reload(invoke: Invoke) {
        val args = invoke.parseArgs(TabIdArgs::class.java)
        result(invoke) { (tabs[args.id] ?: throw IllegalStateException("Dieser Android-Tab ist nicht geladen.")).reload() }
    }

    @Command
    fun setZoom(invoke: Invoke) {
        val args = invoke.parseArgs(ZoomArgs::class.java)
        result(invoke) {
            val view = tabs[args.id] ?: throw IllegalStateException("Dieser Android-Tab ist nicht geladen.")
            val scale = args.zoom.toFloat().coerceIn(0.5f, 3.0f)
            val oldScale = tabZoom[args.id] ?: 1.0f
            view.zoomBy(scale / oldScale)
            tabZoom[args.id] = scale
        }
    }

    @Command
    fun evalScript(invoke: Invoke) {
        val args = invoke.parseArgs(EvalScriptArgs::class.java)
        result(invoke) {
            val view = tabs[args.id] ?: throw IllegalStateException("Dieser Android-Tab ist nicht geladen.")
            view.evaluateJavascript(args.script, null)
        }
    }

    @Command
    fun hasTab(invoke: Invoke) {
        val args = invoke.parseArgs(TabIdArgs::class.java)
        invoke.resolve(JSObject().apply { put("present", tabs.containsKey(args.id)) })
    }

    @Command
    fun setTrackingProtection(invoke: Invoke) {
        val args = invoke.parseArgs(TrackingProtectionArgs::class.java)
        result(invoke) { for (id in tabs.keys) trackingProtection[id] = args.enabled }
    }

    @Command
    fun setLayout(invoke: Invoke) {
        val args = invoke.parseArgs(LayoutArgs::class.java)
        result(invoke) {
            val density = activity.resources.displayMetrics.density
            viewportWidth = (args.width * density).toInt().coerceAtLeast(200)
            viewportHeight = (args.height * density).toInt().coerceAtLeast(200)
            chromeHeight = (args.chromeHeight * density).toInt().coerceIn(0, (viewportHeight * 0.6f).toInt())
            chromePosition = if (args.chromePosition == "bottom") "bottom" else "top"
            applyLayout()
        }
    }

    @Command
    fun setOverlay(invoke: Invoke) {
        val args = invoke.parseArgs(OverlayArgs::class.java)
        result(invoke) { overlay = args.enabled; applyLayout() }
    }

    @Command
    fun openDownload(invoke: Invoke) {
        val args = invoke.parseArgs(PathArgs::class.java)
        result(invoke) {
            val uri = if (args.path.startsWith("content://")) Uri.parse(args.path) else Uri.fromFile(File(args.path))
            val type = activity.contentResolver.getType(uri)
                ?: MimeTypeMap.getSingleton().getMimeTypeFromExtension(MimeTypeMap.getFileExtensionFromUrl(uri.toString()))
                ?: "application/octet-stream"
            val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, type).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            activity.startActivity(Intent.createChooser(intent, "Datei öffnen"))
        }
    }

    @Command
    fun sharePage(invoke: Invoke) {
        val args = invoke.parseArgs(SharePageArgs::class.java)
        result(invoke) {
            val uri = Uri.parse(args.url)
            require(uri.scheme == "http" || uri.scheme == "https") { "Nur HTTP- und HTTPS-Seiten können geteilt werden." }
            val sendIntent = Intent(Intent.ACTION_SEND)
                .setType("text/plain")
                .putExtra(Intent.EXTRA_TEXT, uri.toString())
            activity.startActivity(Intent.createChooser(sendIntent, args.title.take(120).ifBlank { "Veloce" }))
        }
    }

    @Command
    fun requestDefaultBrowser(invoke: Invoke) {
        result(invoke) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
                throw IllegalStateException("Android bietet auf dieser Version keine Browser-Rollenauswahl. Wähle Veloce unter Einstellungen → Apps → Standard-Apps → Browser-App.")
            }
            val roles = activity.getSystemService(RoleManager::class.java)
                ?: throw IllegalStateException("Die Android-Browserrolle ist nicht verfügbar.")
            if (!roles.isRoleAvailable(RoleManager.ROLE_BROWSER)) {
                throw IllegalStateException("Dieses Android-Gerät stellt keine Browser-Rollenauswahl bereit.")
            }
            activity.startActivity(roles.createRequestRoleIntent(RoleManager.ROLE_BROWSER))
        }
    }

    @Command
    fun consumeExternalUrl(invoke: Invoke) {
        val url = pendingExternalUrl
        pendingExternalUrl = null
        invoke.resolve(JSObject().apply { put("url", url) })
    }

    private fun sendPageState(id: String, view: WebView, loading: Boolean) {
        if (activeTabId != id && !hasListener("page-state")) return
        val payload = JSObject().apply {
            put("id", id); put("url", view.url ?: ""); put("title", view.title ?: "")
            put("loading", loading); put("canGoBack", view.canGoBack()); put("canGoForward", view.canGoForward())
        }
        trigger("page-state", payload)
    }

    private fun pageLayoutParams(): FrameLayout.LayoutParams = FrameLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT,
        (viewportHeight - chromeHeight).coerceAtLeast(1),
        Gravity.TOP,
    ).apply { topMargin = if (chromePosition == "top") chromeHeight else 0 }

    private fun applyLayout() {
        val frame = root ?: return
        val pageTop = if (activeIsStart || chromePosition == "bottom") 0 else chromeHeight
        for ((id, view) in tabs) {
            // Keep the playing page resumed while its fullscreen custom view is
            // attached. A window/inset resize must not pause the video underneath it.
            val isActive = id == activeTabId && !activeIsStart
            view.visibility = if (isActive) View.VISIBLE else View.GONE
            val lp = FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                (viewportHeight - if (activeIsStart) 0 else chromeHeight).coerceAtLeast(1),
                Gravity.TOP,
            ).apply { topMargin = pageTop }
            view.layoutParams = lp
            if (isActive) view.onResume() else view.onPause()
        }
        val shellView = shell ?: return
        if (fullScreenView != null) {
            shellView.visibility = View.GONE
            return
        }
        shellView.visibility = View.VISIBLE
        shellView.setBackgroundColor(if (overlay && !activeIsStart) Color.TRANSPARENT else Color.rgb(16, 17, 20))
        val shellHeight = when {
            activeIsStart -> viewportHeight
            overlay -> viewportHeight
            else -> chromeHeight
        }
        val shellTop = if (!activeIsStart && !overlay && chromePosition == "bottom") (viewportHeight - shellHeight).coerceAtLeast(0) else 0
        shellView.layoutParams = FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, shellHeight.coerceAtLeast(1), Gravity.TOP).apply { topMargin = shellTop }
        shellView.bringToFront()
        frame.requestLayout()
    }

    private fun updateBackHandler() {
        val view = activeTabId?.let { tabs[it] }
        backCallback?.isEnabled = fullScreenView != null || view?.canGoBack() == true
    }

    private fun showVideoFullscreen(view: View, callback: WebChromeClient.CustomViewCallback) {
        if (fullScreenView != null) {
            callback.onCustomViewHidden()
            return
        }
        val frame = root ?: return
        fullScreenView = view
        fullScreenCallback = callback
        view.setBackgroundColor(Color.BLACK)
        frame.addView(view, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        if (Build.VERSION.SDK_INT >= 30) {
            activity.window.insetsController?.let {
                it.hide(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
                it.systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        } else {
            @Suppress("DEPRECATION")
            activity.window.decorView.systemUiVisibility = (View.SYSTEM_UI_FLAG_FULLSCREEN or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or View.SYSTEM_UI_FLAG_LAYOUT_STABLE)
        }
        updateBackHandler()
        trigger("fullscreen", JSObject().apply { put("fullscreen", true) })
    }

    private fun hideVideoFullscreen() {
        if (fullScreenView == null && fullScreenCallback == null) return
        val frame = root ?: return
        fullScreenView?.let { frame.removeView(it) }
        fullScreenView = null
        val callback = fullScreenCallback
        fullScreenCallback = null
        callback?.onCustomViewHidden()
        if (Build.VERSION.SDK_INT >= 30) {
            activity.window.insetsController?.show(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
        } else {
            @Suppress("DEPRECATION")
            activity.window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_LAYOUT_STABLE
        }
        applyLayout()
        updateBackHandler()
        trigger("fullscreen", JSObject().apply { put("fullscreen", false) })
    }

    private fun startDownload(url: String, userAgent: String?, contentDisposition: String?, mimeType: String?) {
        try {
            val uri = Uri.parse(url)
            val fileName = URLUtil.guessFileName(url, contentDisposition, mimeType).replace(Regex("[\\\\/:*?\"<>|]"), "_").take(180)
            val request = DownloadManager.Request(uri)
                .setTitle(fileName)
                .setDescription("Veloce Browser")
                .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
                .setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, fileName)
            if (!userAgent.isNullOrBlank()) request.addRequestHeader("User-Agent", userAgent)
            CookieManager.getInstance().getCookie(url)?.takeIf { it.isNotBlank() }?.let { request.addRequestHeader("Cookie", it) }
            if (!mimeType.isNullOrBlank()) request.setMimeType(mimeType)
            val manager = activity.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
            val downloadId = manager.enqueue(request)
            val timestamp = System.currentTimeMillis()
            downloadRecords[downloadId] = DownloadRecord(fileName, timestamp)
            trigger("download", JSObject().apply {
                put("id", "android-$downloadId"); put("fileName", fileName); put("path", "Downloads/$fileName")
                put("startedAtMs", timestamp); put("status", "downloading")
            })
        } catch (error: Throwable) {
            trigger("download-error", JSObject().apply { put("message", error.message ?: "Android-Download konnte nicht gestartet werden.") })
        }
    }

    override fun onDestroy(activity: AppCompatActivity) {
        super.onDestroy(activity)
        pendingFileCallback?.onReceiveValue(null)
        pendingFileCallback = null
        if (downloadReceiverRegistered) {
            try { activity.unregisterReceiver(downloadReceiver) } catch (_: IllegalArgumentException) { }
            downloadReceiverRegistered = false
        }
        tabs.values.forEach { it.stopLoading(); it.destroy() }
        tabs.clear()
        trackingProtection.clear()
        tabZoom.clear()
    }

    private fun isTrackingHost(host: String): Boolean {
        val domains = listOf("adservice.google.com", "doubleclick.net", "google-analytics.com", "googlesyndication.com", "googletagmanager.com", "scorecardresearch.com", "taboola.com", "outbrain.com")
        return domains.any { host == it || host.endsWith(".$it") }
    }
}
