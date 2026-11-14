package app.stafy.player.media

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.util.Base64
import java.io.File
import java.net.URL

/**
 * Serves album art to Android Auto / System UI / Bluetooth, which can only load content:// URIs.
 * Read-only, and only ever serves files from the app's cover cache (remote art is cached first).
 */
class CoverProvider : ContentProvider() {
    override fun onCreate() = true

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor? {
        val ctx = context ?: return null
        val covers = File(ctx.cacheDir, "covers").canonicalFile
        val file = when (uri.pathSegments.firstOrNull()) {
            "local" -> File(covers, uri.lastPathSegment ?: return null).canonicalFile
            "remote" -> fetch(ctx, String(Base64.decode(uri.lastPathSegment, Base64.URL_SAFE or Base64.NO_WRAP)))
            else -> null
        } ?: return null
        // No path tricks: the file must live in the cover cache.
        if (file.parentFile?.canonicalFile?.startsWith(covers) != true || !file.isFile) return null
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    private fun fetch(ctx: Context, url: String): File? {
        if (!url.startsWith("https://")) return null
        val dir = File(ctx.cacheDir, "covers/remote").apply { mkdirs() }
        val file = File(dir, Integer.toHexString(url.hashCode()) + ".jpg")
        if (!file.exists()) {
            runCatching {
                val tmp = File(dir, file.name + ".part")
                URL(url).openStream().use { input -> tmp.outputStream().use { input.copyTo(it) } }
                tmp.renameTo(file)
            }
        }
        return file.takeIf { it.exists() }
    }

    override fun getType(uri: Uri) = "image/*"
    override fun query(uri: Uri, p: Array<out String>?, s: String?, a: Array<out String>?, o: String?): Cursor? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, s: String?, a: Array<out String>?) = 0
    override fun update(uri: Uri, v: ContentValues?, s: String?, a: Array<out String>?) = 0

    companion object {
        fun uriFor(ctx: Context, art: String?): Uri? {
            if (art.isNullOrEmpty()) return null
            val base = "content://${ctx.packageName}.covers"
            return if (art.startsWith("https://")) {
                Uri.parse("$base/remote/" + Base64.encodeToString(art.toByteArray(), Base64.URL_SAFE or Base64.NO_WRAP))
            } else {
                Uri.parse("$base/local/" + File(art).name)
            }
        }
    }
}
