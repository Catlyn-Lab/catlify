package app.stafy.player

import android.Manifest
import android.content.ComponentName
import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.webkit.JavascriptInterface
import android.view.WindowManager
import android.webkit.WebView
import android.graphics.Color
import android.net.wifi.WifiManager
import android.provider.MediaStore
import androidx.activity.result.IntentSenderRequest
import androidx.activity.result.contract.ActivityResultContracts
import org.json.JSONArray
import org.json.JSONObject
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import app.stafy.player.media.StafyMediaService
import app.stafy.player.youtube.StafyYouTube
import com.google.common.util.concurrent.ListenableFuture

class MainActivity : TauriActivity() {
  private var controller: ListenableFuture<MediaController>? = null
  private var webView: WebView? = null

  // Deleting songs other apps put in Music/ needs the user's OK in a system dialog (Android 11+).
  private var deleteCallId: String? = null
  private val deleteLauncher = registerForActivityResult(ActivityResultContracts.StartIntentSenderForResult()) { result ->
    val id = deleteCallId ?: return@registerForActivityResult
    deleteCallId = null
    val ok = result.resultCode == RESULT_OK
    webView?.evaluateJavascript("window.__stafyDeleted && window.__stafyDeleted(${JSONObject.quote(id)}, $ok)", null)
  }

  /** Safe-area insets in CSS px (status bar, nav bar, camera cutout / punch hole, keyboard). */
  @Volatile private var insetsJson = """{"top":0,"right":0,"bottom":0,"left":0,"keyboard":0}"""

  override fun onCreate(savedInstanceState: Bundle?) {
    // Catlify is always dark: light status/nav bar icons regardless of the system theme.
    enableEdgeToEdge(SystemBarStyle.dark(Color.TRANSPARENT), SystemBarStyle.dark(Color.TRANSPARENT))
    super.onCreate(savedInstanceState)

    // Bind the media service so playback started from the UI gets a session, notification,
    // Bluetooth metadata and Android Auto support, and keeps going in the background.
    val token = SessionToken(this, ComponentName(this, StafyMediaService::class.java))
    controller = MediaController.Builder(this, token).buildAsync()

    val wanted = buildList {
      if (Build.VERSION.SDK_INT >= 33) {
        add(Manifest.permission.POST_NOTIFICATIONS)
        add(Manifest.permission.READ_MEDIA_AUDIO)
      } else {
        add(Manifest.permission.READ_EXTERNAL_STORAGE)
      }
      // Android 12+: "Nearby devices", to resume playback when the car's Bluetooth connects.
      if (Build.VERSION.SDK_INT >= 31) add(Manifest.permission.BLUETOOTH_CONNECT)
      // Android 8-9: saving downloads and synced songs to Music/Catlify.
      if (Build.VERSION.SDK_INT < 29) add(Manifest.permission.WRITE_EXTERNAL_STORAGE)
    }.filter { ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED }
    if (wanted.isNotEmpty()) ActivityCompat.requestPermissions(this, wanted.toTypedArray(), 1)
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    // Android WebView doesn't reliably expose env(safe-area-inset-*), so the page asks us.
    webView.addJavascriptInterface(InsetsBridge(), "StafyInsets")
    webView.addJavascriptInterface(ScreenBridge(), "StafyScreen")
    webView.addJavascriptInterface(FilesBridge(), "StafyFiles")
    StafyYouTube.install(this, webView)
    intent?.let { handleShare(it) }
    ViewCompat.setOnApplyWindowInsetsListener(webView) { view, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
      val d = resources.displayMetrics.density
      fun css(px: Int) = Math.round(px / d)
      insetsJson = """{"top":${css(bars.top)},"right":${css(bars.right)},"bottom":${css(bars.bottom)},"left":${css(bars.left)},"keyboard":${css(ime.bottom)}}"""
      view.post { (view as WebView).evaluateJavascript("window.__stafyInsets && window.__stafyInsets()", null) }
      insets
    }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    handleShare(intent)
  }

  private fun handleShare(intent: Intent) {
    if (intent.action == Intent.ACTION_SEND) {
      intent.getStringExtra(Intent.EXTRA_TEXT)?.let { StafyYouTube.share(it) }
    }
  }

  /** Keeps the display awake while lyrics are on screen (the WebView has no wake lock API). */
  private inner class ScreenBridge {
    @JavascriptInterface fun keepOn(on: Boolean) = runOnUiThread {
      if (on) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      else window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    /** "See video": landscape (either way up) with the status and navigation bars hidden. */
    @JavascriptInterface fun videoMode(on: Boolean) = runOnUiThread {
      requestedOrientation = if (on) ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE else ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
      val bars = WindowCompat.getInsetsController(window, window.decorView)
      if (on) {
        bars.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        bars.hide(WindowInsetsCompat.Type.systemBars())
      } else {
        bars.show(WindowInsetsCompat.Type.systemBars())
      }
    }
  }

  private inner class FilesBridge {
    /** Asks Android to delete these files (system confirmation). Answers via window.__stafyDeleted. */
    @JavascriptInterface fun requestDelete(callId: String, pathsJson: String) = runOnUiThread {
      val paths = JSONArray(pathsJson).let { a -> (0 until a.length()).map { a.getString(it) } }
      val known = mutableListOf<android.net.Uri>()
      val unindexed = mutableListOf<String>()
      for (path in paths) {
        // Delete requests only accept media (audio) URIs, not generic file ones.
        val audio = MediaStore.Audio.Media.EXTERNAL_CONTENT_URI
        @Suppress("DEPRECATION")
        val uri = contentResolver.query(audio, arrayOf(MediaStore.MediaColumns._ID), "${MediaStore.MediaColumns.DATA} = ?", arrayOf(path), null)
          ?.use { c -> if (c.moveToFirst()) android.content.ContentUris.withAppendedId(audio, c.getLong(0)) else null }
        if (uri != null) known.add(uri) else unindexed.add(path)
      }
      if (unindexed.isEmpty()) return@runOnUiThread askToDelete(callId, known)
      // Files Android hasn't indexed (e.g. copied in by an older version) can't be put in a delete
      // request; index them first.
      var left = unindexed.size
      android.media.MediaScannerConnection.scanFile(this@MainActivity, unindexed.toTypedArray(), null) { _, uri ->
        synchronized(known) {
          uri?.let { known.add(it) }
          left--
          if (left == 0) runOnUiThread { askToDelete(callId, known.toList()) }
        }
      }
    }
  }

  private fun askToDelete(callId: String, uris: List<android.net.Uri>) {
    if (uris.isEmpty() || Build.VERSION.SDK_INT < 30) {
      webView?.evaluateJavascript("window.__stafyDeleted && window.__stafyDeleted(${JSONObject.quote(callId)}, false)", null)
      return
    }
    try {
      val pending = MediaStore.createDeleteRequest(contentResolver, uris)
      deleteCallId = callId
      deleteLauncher.launch(IntentSenderRequest.Builder(pending.intentSender).build())
    } catch (e: Exception) {
      android.util.Log.w("Catlify", "delete request refused", e)
      webView?.evaluateJavascript("window.__stafyDeleted && window.__stafyDeleted(${JSONObject.quote(callId)}, false)", null)
    }
  }

  private inner class InsetsBridge {
    @JavascriptInterface fun get(): String = insetsJson
  }

  override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
    super.onRequestPermissionsResult(requestCode, permissions, grantResults)
    // Music folders were scanned before the permission existed; look again now.
    val media = permissions.indexOfFirst {
      it == Manifest.permission.READ_MEDIA_AUDIO || it == Manifest.permission.READ_EXTERNAL_STORAGE
    }
    if (media >= 0 && grantResults.getOrNull(media) == PackageManager.PERMISSION_GRANTED) {
      webView?.evaluateJavascript("window.__stafyRescan && window.__stafyRescan()", null)
    }
  }

  // Android drops mDNS (multicast) packets unless an app holds this lock: needed to find other
  // devices for Wi-Fi sync. Held only while Catlify is on screen.
  private var multicast: WifiManager.MulticastLock? = null

  override fun onResume() {
    super.onResume()
    val wifi = applicationContext.getSystemService(WIFI_SERVICE) as? WifiManager
    multicast = wifi?.createMulticastLock("catlify-sync")?.apply { setReferenceCounted(false); acquire() }
  }

  override fun onPause() {
    multicast?.release()
    multicast = null
    super.onPause()
  }

  override fun onDestroy() {
    controller?.let { MediaController.releaseFuture(it) }
    super.onDestroy()
  }
}
