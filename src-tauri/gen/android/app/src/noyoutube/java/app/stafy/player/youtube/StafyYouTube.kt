package app.stafy.player.youtube

import android.app.Activity
import android.webkit.WebView

/** Store builds (-Pstafy.youtube=false): no YouTube downloads, no yt-dlp bundled. */
object StafyYouTube {
    const val ENABLED = false
    fun install(activity: Activity, webView: WebView) {}
    fun share(text: String) {}
    fun registerStreams(ctx: android.content.Context) {}
}
