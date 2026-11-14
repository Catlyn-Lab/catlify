package app.stafy.player.youtube

import android.app.Activity
import android.content.ContentValues
import android.content.Context
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.util.Log
import android.webkit.JavascriptInterface
import android.webkit.WebView
import app.stafy.player.media.StafyNative
import com.yausername.ffmpeg.FFmpeg
import com.yausername.youtubedl_android.YoutubeDL
import com.yausername.youtubedl_android.YoutubeDLRequest
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.locks.ReentrantReadWriteLock
import kotlin.concurrent.read
import kotlin.concurrent.write

/**
 * YouTube lookup + audio download on Android, using a bundled yt-dlp (youtubedl-android).
 * Exposed to the web UI as `window.StafyYouTube`; mirrors the desktop commands in youtube.rs.
 * Song title/artist guessing and tagging reuse the Rust code (ytmeta.rs) through StafyNative.
 *
 * Built only when the Gradle property `stafy.youtube` is true (default). Play Store builds must
 * pass -Pstafy.youtube=false: store policies forbid downloading from YouTube.
 */
object StafyYouTube {
    const val ENABLED = true
    private var bridge: Bridge? = null

    fun install(activity: Activity, webView: WebView) {
        bridge = Bridge(activity.applicationContext, webView).also { webView.addJavascriptInterface(it, "StafyYouTube") }
    }

    /** A YouTube link shared to Catlify from another app. */
    fun share(text: String) {
        bridge?.share(text)
    }

    /** Lets the player stream full songs from YouTube without downloading them to the library. */
    fun registerStreams(ctx: Context) {
        StafyNative.setStreamResolver(object : StafyNative.StreamResolver {
            override fun fetch(spec: String, stem: String): String = try {
                // yt-dlp must run on our worker threads (they carry a class loader), not the JNI thread.
                YtDlp.pool.submit<String> { YtDlp.fetchStream(ctx, spec, File(stem)) }.get()
            } catch (e: Throwable) {
                Log.w("StafyYouTube", "stream fetch failed for $spec", e.cause ?: e)
                "ERR:" + ((e.cause ?: e).message?.lineSequence()?.lastOrNull { it.contains("ERROR") } ?: "couldn't play from YouTube")
            }

            override fun extract(spec: String): String = try {
                YtDlp.pool.submit<String> { YtDlp.extractLink(ctx, spec) }.get()
            } catch (e: Throwable) {
                "ERR:" + ((e.cause ?: e).message?.lineSequence()?.lastOrNull { it.contains("ERROR") } ?: "couldn't find the song")
            }
        })
    }
}

/** One yt-dlp runtime for the UI bridge and the player. */
internal object YtDlp {
    lateinit var appContext: Context

    // Worker threads need a context class loader: yt-dlp's updater (commons-io) crashes without one.
    val pool = Executors.newCachedThreadPool { r ->
        Thread(r, "stafy-yt").apply { contextClassLoader = YtDlp::class.java.classLoader }
    }

    /** Updating replaces the yt-dlp files: searches/downloads/streams (read) wait for it (write). */
    val lock = ReentrantReadWriteLock()
    @Volatile private var ready = false

    /**
     * First use unpacks Python + yt-dlp (a few seconds). The bundled yt-dlp is usually too old for
     * YouTube, so the first update runs before anything else; later ones run daily in the background.
     */
    fun ensureReady(ctx: Context) {
        if (ready) return
        synchronized(this) {
            if (ready) return
            appContext = ctx.applicationContext
            YoutubeDL.getInstance().init(appContext)
            FFmpeg.getInstance().init(appContext)
            val last = prefs().getLong("updated", 0)
            if (last == 0L) updateNow() // blocking: never use the stale bundled copy
            ready = true
            if (last != 0L && System.currentTimeMillis() - last > 24 * 3600 * 1000L) pool.execute { updateNow() }
        }
    }

    private fun prefs() = appContext.getSharedPreferences("stafy-yt", Context.MODE_PRIVATE)

    fun updateNow(): String? = lock.write {
        runCatching { YoutubeDL.getInstance().updateYoutubeDL(appContext, YoutubeDL.UpdateChannel._STABLE)?.name }
            .onSuccess { prefs().edit().putLong("updated", System.currentTimeMillis()).apply() }
            .onFailure { Log.w("StafyYouTube", "yt-dlp update failed", it) }
            .getOrNull()
    }

    private fun target(spec: String) = when {
        spec.startsWith("yt:") -> "https://www.youtube.com/watch?v=${spec.removePrefix("yt:")}"
        // YouTube Music "songs" results are the audio uploads, not videos with intros.
        else -> "https://music.youtube.com/search?q=${java.net.URLEncoder.encode(spec.removePrefix("ytq:"), "UTF-8")}#songs"
    }

    /** Only looks up the audio link (JSON); much faster than a download. */
    fun extractLink(ctx: Context, spec: String): String {
        ensureReady(ctx)
        val req = YoutubeDLRequest(target(spec))
            .addOption("-J").addOption("--no-warnings").addOption("--no-playlist").addOption("--playlist-items", "1")
            .addOption("-f", "bestaudio[ext=m4a]/bestaudio")
        return lock.read { YoutubeDL.getInstance().execute(req).out }
    }

    /** Audio as YouTube serves it (no conversion, so playback starts sooner). */
    fun fetchStream(ctx: Context, spec: String, stem: File): String {
        ensureReady(ctx)
        val target = when {
            spec.startsWith("yt:") -> "https://www.youtube.com/watch?v=${spec.removePrefix("yt:")}"
            // YouTube Music "songs" results are the audio uploads, not videos with intros.
            else -> "https://music.youtube.com/search?q=${java.net.URLEncoder.encode(spec.removePrefix("ytq:"), "UTF-8")}#songs"
        }
        val req = YoutubeDLRequest(target)
            .addOption("--no-playlist").addOption("--no-warnings").addOption("--playlist-items", "1")
            .addOption("-f", "bestaudio[ext=m4a]/bestaudio")
            .addOption("-o", "${stem.absolutePath}.%(ext)s")
        lock.read { YoutubeDL.getInstance().execute(req) }
        return stem.parentFile?.listFiles()?.firstOrNull { it.nameWithoutExtension == stem.name && it.extension != "part" }?.absolutePath
            ?: error("yt-dlp didn't produce audio")
    }
}

private class Bridge(private val ctx: Context, private val webView: WebView) {
    // Worker threads need a context class loader: yt-dlp's updater (commons-io) crashes without one.
    private val pool get() = YtDlp.pool
    private val running = ConcurrentHashMap<String, Boolean>()
    @Volatile private var pendingShare: String? = null

    // ---- plumbing ----------------------------------------------------------------------

    private fun js(code: String) = webView.post { webView.evaluateJavascript(code, null) }

    /** Runs [work] off the UI thread and resolves the JS promise registered under [callId]. */
    private fun async(callId: String, work: () -> Any?) {
        pool.execute {
            val payload = try {
                JSONObject().put("ok", work() ?: JSONObject.NULL)
            } catch (e: Throwable) {
                Log.w("StafyYouTube", "call failed", e)
                JSONObject().put("error", e.message ?: e.toString())
            }
            js("window.__stafyYt && window.__stafyYt(${JSONObject.quote(callId)}, $payload)")
        }
    }

    private fun progress(id: String, stage: String, percent: Double, path: String? = null, error: String? = null) {
        val p = JSONObject().put("id", id).put("stage", stage).put("percent", percent)
            .put("path", path ?: JSONObject.NULL).put("error", error ?: JSONObject.NULL)
        js("window.dispatchEvent(new CustomEvent('stafy-yt-progress', { detail: $p }))")
    }

    private fun ensureReady() = YtDlp.ensureReady(ctx)

    private fun updateNow() = YtDlp.updateNow()

    fun share(text: String) {
        val url = Regex("https?://\\S+").find(text)?.value ?: text.trim()
        pendingShare = url
        js("window.__stafyShare && window.__stafyShare(${JSONObject.quote(url)})")
    }

    // ---- JS API ------------------------------------------------------------------------

    @JavascriptInterface
    fun takeShare(): String? = pendingShare.also { pendingShare = null }

    @JavascriptInterface
    fun status(callId: String) = async(callId) {
        ensureReady()
        JSONObject()
            .put("ytDlp", (YoutubeDL.getInstance().versionName(ctx) ?: YoutubeDL.getInstance().version(ctx) ?: "bundled").removePrefix("yt-dlp").trim())
            .put("ffmpeg", true)
            .put("downloadDir", "Music/Catlify")
    }

    @JavascriptInterface
    fun update(callId: String) = async(callId) {
        ensureReady()
        updateNow()
    }

    @JavascriptInterface
    fun lookup(callId: String, query: String) = lookupAll(callId, query, false)

    /** `playlist`: list every song of a YouTube / YouTube Music playlist link (import). */
    @JavascriptInterface
    fun lookupAll(callId: String, query: String, playlist: Boolean) = async(callId) {
        ensureReady()
        val q = query.trim()
        val isUrl = q.startsWith("http") || q.contains("youtu.be/") || q.contains("youtube.com/")
        val req = if (isUrl && playlist) {
            YoutubeDLRequest(q).addOption("-J").addOption("--flat-playlist")
        } else if (isUrl) {
            YoutubeDLRequest(q).addOption("-J").addOption("--no-playlist").addOption("--skip-download")
        } else {
            YoutubeDLRequest("ytsearch15:$q").addOption("-J").addOption("--flat-playlist")
        }
        req.addOption("--no-warnings")
        val out = YtDlp.lock.read { YoutubeDL.getInstance().execute(req).out }
        StafyNative.rpc("ytVideos", JSONObject().put("json", out)) as? JSONArray ?: JSONArray()
    }

    /** "See video": a direct MP4 (picture + sound) for a song search, video id or link. Same as youtube.rs. */
    @JavascriptInterface
    fun video(callId: String, query: String) = async(callId) {
        ensureReady()
        val q = query.trim()
        val target = when {
            Regex("^[\\w-]{11}$").matches(q) -> "https://www.youtube.com/watch?v=$q"
            q.startsWith("http") -> q
            else -> "ytsearch1:$q official music video"
        }
        val req = YoutubeDLRequest(target)
            .addOption("-J").addOption("--no-warnings").addOption("--no-playlist").addOption("--force-ipv4")
            .addOption("-f", "best[ext=mp4][vcodec!=none][acodec!=none]/best[vcodec!=none][acodec!=none]")
        val out = YtDlp.lock.read { YoutubeDL.getInstance().execute(req).out }
        StafyNative.rpc("ytVideoStream", JSONObject().put("json", out)) as? JSONObject ?: error("no video found")
    }

    @JavascriptInterface
    fun cancel(id: String) {
        running.remove(id)
        YoutubeDL.getInstance().destroyProcessById(id)
    }

    @JavascriptInterface
    fun download(id: String, url: String, title: String, artist: String, format: String) {
        running[id] = true
        progress(id, "downloading", 0.0)
        pool.execute {
            val tmp = File(ctx.cacheDir, "yt/$id")
            try {
                ensureReady()
                tmp.deleteRecursively()
                tmp.mkdirs()
                val req = YoutubeDLRequest(url)
                    .addOption("--no-playlist").addOption("--no-warnings")
                    .addOption("-x").addOption("--embed-thumbnail").addOption("--embed-metadata")
                    .addOption("--convert-thumbnails", "jpg")
                    .addOption("-o", "${tmp.absolutePath}/audio.%(ext)s")
                if (format == "mp3") {
                    req.addOption("-f", "bestaudio").addOption("--audio-format", "mp3").addOption("--audio-quality", "0")
                } else {
                    req.addOption("-f", "bestaudio[ext=m4a]/bestaudio").addOption("--audio-format", "m4a")
                }
                YtDlp.lock.read {
                    YoutubeDL.getInstance().execute(req, id) { pct, _, _ ->
                        if (pct >= 100f) progress(id, "converting", 100.0) else if (pct >= 0f) progress(id, "downloading", pct.toDouble())
                    }
                }
                if (running.remove(id) == null) {
                    progress(id, "cancelled", 0.0)
                    return@execute
                }
                val audio = tmp.listFiles()?.firstOrNull { it.extension.lowercase() in setOf("mp3", "m4a", "opus", "webm", "ogg") }
                    ?: error("yt-dlp didn't produce an audio file")
                StafyNative.rpc("retag", JSONObject().put("path", audio.absolutePath).put("title", title).put("artist", artist))
                val name = safeName(if (artist.isBlank()) title else "$artist - $title")
                val path = saveToMusic(audio, name)
                // The page rescans on "done" (library_rescan), which also refreshes the song lists.
                progress(id, "done", 100.0, path = path)
            } catch (e: Throwable) {
                if (running.remove(id) == null) progress(id, "cancelled", 0.0)
                else progress(id, "error", 0.0, error = e.message?.lineSequence()?.lastOrNull { it.contains("ERROR") } ?: e.message ?: "Download failed")
            } finally {
                tmp.deleteRecursively()
            }
        }
    }

    // ---- saving ------------------------------------------------------------------------

    private fun safeName(s: String) =
        s.map { if ("/\\:*?\"<>|".contains(it) || it.isISOControl()) '_' else it }.joinToString("").trim().trimStart('.').take(150)

    /** Copies into the shared Music/Catlify folder (MediaStore on Android 10+), returns the file path. */
    private fun saveToMusic(file: File, name: String): String {
        val ext = file.extension.lowercase()
        if (Build.VERSION.SDK_INT >= 29) {
            val resolver = ctx.contentResolver
            val collection = MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
            val values = ContentValues().apply {
                put(MediaStore.MediaColumns.DISPLAY_NAME, "$name.$ext")
                put(MediaStore.MediaColumns.MIME_TYPE, if (ext == "mp3") "audio/mpeg" else "audio/mp4")
                put(MediaStore.MediaColumns.RELATIVE_PATH, "${Environment.DIRECTORY_MUSIC}/Catlify")
                put(MediaStore.MediaColumns.IS_PENDING, 1)
            }
            val uri = resolver.insert(collection, values) ?: error("couldn't create the file in Music/Catlify")
            resolver.openOutputStream(uri)!!.use { out -> file.inputStream().use { it.copyTo(out) } }
            values.clear()
            values.put(MediaStore.MediaColumns.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
            // MediaStore may rename on collision ("x (1).mp3"); ask for the real path.
            @Suppress("DEPRECATION")
            resolver.query(uri, arrayOf(MediaStore.MediaColumns.DATA), null, null, null)?.use { c ->
                if (c.moveToFirst()) return c.getString(0)
            }
            @Suppress("DEPRECATION")
            return File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MUSIC), "Catlify/$name.$ext").path
        }
        @Suppress("DEPRECATION")
        val dir = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_MUSIC), "Catlify").apply { mkdirs() }
        val dest = File(dir, "$name.$ext")
        file.copyTo(dest, overwrite = true)
        return dest.path
    }
}
