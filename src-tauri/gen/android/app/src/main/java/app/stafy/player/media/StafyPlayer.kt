package app.stafy.player.media

import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import org.json.JSONArray
import org.json.JSONObject

/**
 * Media3 Player backed by the Rust engine, so Android Auto, Bluetooth (AVRCP), the
 * notification and lock screen all control the same gapless, any-format playback.
 */
@UnstableApi
class StafyPlayer(private val artUri: (String?) -> Uri?) : SimpleBasePlayer(Looper.getMainLooper()) {
    private val main = Handler(Looper.getMainLooper())
    private var state = JSONObject()
    private var items: List<JSONObject> = emptyList()
    /** The slice of `items` last published to the session; media item indices refer to this. */
    private var visible: List<JSONObject> = emptyList()
    private var queueVersion = -1L

    /** Set by the service right after a car's Bluetooth connects, when auto-play is disabled. */
    var ignorePlayUntil = 0L
    /** Notified when Rust reports playing/paused, so the service can manage audio focus. */
    var onPlayingChanged: ((Boolean) -> Unit)? = null
    /** Notified when a new song starts (car lyrics and Recent lists refresh). */
    var onTrackChanged: (() -> Unit)? = null
    private var lastPlaying = false

    private val listener = object : StafyNative.Listener {
        override fun onState(json: String, trackChanged: Boolean) {
            main.post { applyState(JSONObject(json), trackChanged) }
        }
    }

    init {
        StafyNative.setListener(listener)
        refreshSession()
    }

    private fun applyState(s: JSONObject, trackChanged: Boolean) {
        state = s
        if (trackChanged || s.optLong("queueVersion") != queueVersion) refreshSession()
        if (trackChanged) onTrackChanged?.invoke()
        val playing = s.optBoolean("playing")
        if (playing != lastPlaying) {
            lastPlaying = playing
            onPlayingChanged?.invoke(playing)
        }
        invalidateState()
    }

    private fun refreshSession() {
        val session = StafyNative.rpc("session") as? JSONObject ?: return
        queueVersion = session.optLong("queueVersion")
        val arr = session.optJSONArray("items") ?: JSONArray()
        items = (0 until arr.length()).map { arr.getJSONObject(it) }
        if (state.length() == 0) state = session
    }

    /** Position of the current song in the full queue (`items`). */
    private fun queuePosition(): Int {
        val queueIndex = if (state.isNull("index")) -1 else state.optInt("index", -1)
        return items.indexOfFirst { it.optInt("index") == queueIndex }.coerceAtLeast(0)
    }

    /** Position of the current song in `visible`. */
    private fun currentPosition(): Int {
        val queueIndex = if (state.isNull("index")) -1 else state.optInt("index", -1)
        return visible.indexOfFirst { it.optInt("index") == queueIndex }.coerceAtLeast(0)
    }

    override fun getState(): State {
        val total = items.size
        // Publish a window around the current song, not the whole queue (which can be the whole
        // library). Media3 loads artwork for every queue item before it hands the queue to the
        // platform session. With thousands of items the Bluetooth stack read a stale queue,
        // so head units (Mazda Connect) showed the wrong song or "Unknown".
        val current = queuePosition()
        val end = (maxOf(current - WINDOW_BEFORE, 0) + WINDOW).coerceAtMost(total)
        val start = (end - WINDOW).coerceAtLeast(0)
        visible = items.subList(start, end)
        val playlist = visible.mapIndexed { i, it ->
            val pos = start + i
            val title = it.optString("title").ifEmpty { "Unknown Title" }
            val artist = it.optString("artist").ifEmpty { "Unknown Artist" }
            val album = it.optString("album").ifEmpty { "Unknown Album" }
            val durationMs = (it.optDouble("duration", 0.0) * 1000).toLong()
            // Android's AVRCP service reads track info from two places: the session metadata and
            // the current queue item (title/subtitle/description + extras). If they differ, it
            // treats them as "out of sync". It then holds back the update or sends the car the
            // queue's version, which had no artist, track number or duration. So both carry the
            // same fields. Media3 doesn't forward track number / count to the platform metadata
            // at all, so those go in extras, which it copies to both.
            val extras = Bundle().apply {
                putLong(KEY_TRACK_NUMBER, (pos + 1).toLong()) // AVRCP "3 / 12" on car displays
                putLong(KEY_NUM_TRACKS, total.toLong())
                if (durationMs > 0) putLong(KEY_DURATION, durationMs)
            }
            val meta = MediaMetadata.Builder()
                .setTitle(title)
                .setDisplayTitle(title)
                .setArtist(artist)
                .setSubtitle(artist)
                .setAlbumTitle(album)
                .setDescription(album)
                .setArtworkUri(artUri(it.optString("art").ifEmpty { null }))
                .setTrackNumber(pos + 1)
                .setTotalTrackCount(total)
                .setDurationMs(if (durationMs > 0) durationMs else null)
                .setExtras(extras)
                .setIsPlayable(true)
                .setIsBrowsable(false)
                .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC)
                .build()
            val item = MediaItem.Builder().setMediaId(it.optString("mediaId")).setMediaMetadata(meta).build()
            MediaItemData.Builder("${it.optInt("index")}:${it.optString("mediaId")}")
                .setMediaItem(item)
                .setDurationUs(if (durationMs > 0) durationMs * 1000 else C.TIME_UNSET)
                .build()
        }
        val playing = state.optBoolean("playing")
        val ended = state.optBoolean("ended")
        val positionMs = (state.optDouble("position", 0.0) * 1000).toLong()
        val repeat = when (state.optString("repeat")) {
            "one" -> Player.REPEAT_MODE_ONE
            "all" -> Player.REPEAT_MODE_ALL
            else -> Player.REPEAT_MODE_OFF
        }
        val builder = State.Builder()
            .setAvailableCommands(COMMANDS)
            .setAudioAttributes(
                AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build()
            )
            .setPlayWhenReady(playing, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
            .setPlaybackState(
                when {
                    playlist.isEmpty() -> Player.STATE_IDLE
                    ended -> Player.STATE_ENDED
                    else -> Player.STATE_READY
                }
            )
            .setShuffleModeEnabled(state.optBoolean("shuffle"))
            .setRepeatMode(repeat)
            .setPlaylist(playlist)
        if (playlist.isNotEmpty()) {
            builder.setCurrentMediaItemIndex(currentPosition())
                .setContentPositionMs(PositionSupplier.getExtrapolating(positionMs, if (playing) 1f else 0f))
        }
        return builder.build()
    }

    private fun done(): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        if (playWhenReady) {
            if (System.currentTimeMillis() < ignorePlayUntil) return done() // car auto-play suppressed
            StafyNative.control("play")
        } else {
            StafyNative.control("pause")
        }
        return done()
    }

    override fun handlePrepare(): ListenableFuture<*> = done()

    override fun handleStop(): ListenableFuture<*> {
        StafyNative.control("pause")
        return done()
    }

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
        when (seekCommand) {
            Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> StafyNative.control("next")
            Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> StafyNative.control("previous")
            else -> {
                if (mediaItemIndex != C.INDEX_UNSET && mediaItemIndex != currentPosition()) {
                    visible.getOrNull(mediaItemIndex)?.let { StafyNative.rpc("jump", JSONObject().put("index", it.optInt("index"))) }
                }
                if (positionMs != C.TIME_UNSET && positionMs > 0) {
                    StafyNative.rpc("seek", JSONObject().put("position", positionMs / 1000.0))
                }
            }
        }
        return done()
    }

    override fun handleSetShuffleModeEnabled(shuffleModeEnabled: Boolean): ListenableFuture<*> {
        StafyNative.rpc("shuffle", JSONObject().put("on", shuffleModeEnabled))
        return done()
    }

    override fun handleSetRepeatMode(repeatMode: Int): ListenableFuture<*> {
        val mode = when (repeatMode) {
            Player.REPEAT_MODE_ONE -> "one"
            Player.REPEAT_MODE_ALL -> "all"
            else -> "off"
        }
        StafyNative.rpc("repeat", JSONObject().put("mode", mode))
        return done()
    }

    /** Items arrive from Android Auto / controllers as browse ids: "parent|child" or bare ids. */
    override fun handleSetMediaItems(mediaItems: MutableList<MediaItem>, startIndex: Int, startPositionMs: Long): ListenableFuture<*> {
        val first = mediaItems.firstOrNull() ?: return done()
        val query = first.requestMetadata.searchQuery
        when {
            query != null -> StafyNative.rpc("play", JSONObject().put("parent", "search:$query"))
            mediaItems.size == 1 && first.mediaId.contains('|') -> {
                // Tapping a song in a list plays the whole list from that song.
                val (parent, id) = first.mediaId.split('|', limit = 2)
                StafyNative.rpc("play", JSONObject().put("parent", parent).put("id", id))
            }
            else -> {
                val ids = JSONArray(mediaItems.map { it.mediaId.substringAfter('|') })
                StafyNative.rpc("playIds", JSONObject().put("ids", ids).put("start", maxOf(0, startIndex)))
            }
        }
        if (startPositionMs != C.TIME_UNSET && startPositionMs > 0) {
            StafyNative.rpc("seek", JSONObject().put("position", startPositionMs / 1000.0))
        }
        return done()
    }

    override fun handleAddMediaItems(index: Int, mediaItems: MutableList<MediaItem>): ListenableFuture<*> {
        val ids = JSONArray(mediaItems.map { it.mediaId.substringAfter('|') })
        StafyNative.rpc("enqueueIds", JSONObject().put("ids", ids).put("next", index <= currentPosition() + 1))
        return done()
    }

    override fun handleRemoveMediaItems(fromIndex: Int, toIndex: Int): ListenableFuture<*> {
        // Remove from the end so earlier queue indices stay valid.
        for (pos in (fromIndex until toIndex).reversed()) {
            visible.getOrNull(pos)?.let { StafyNative.rpc("remove", JSONObject().put("index", it.optInt("index"))) }
        }
        return done()
    }

    override fun handleRelease(): ListenableFuture<*> = done()

    companion object {
        /** Songs published to the session: a few before the current one, the rest after. */
        private const val WINDOW = 30
        private const val WINDOW_BEFORE = 5
        // android.media.MediaMetadata keys, as read by the Bluetooth (AVRCP) service.
        private const val KEY_TRACK_NUMBER = "android.media.metadata.TRACK_NUMBER"
        private const val KEY_NUM_TRACKS = "android.media.metadata.NUM_TRACKS"
        private const val KEY_DURATION = "android.media.metadata.DURATION"

        private val COMMANDS = Player.Commands.Builder().addAll(
            Player.COMMAND_PLAY_PAUSE,
            Player.COMMAND_PREPARE,
            Player.COMMAND_STOP,
            Player.COMMAND_SEEK_TO_DEFAULT_POSITION,
            Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
            Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
            Player.COMMAND_SEEK_TO_PREVIOUS,
            Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
            Player.COMMAND_SEEK_TO_NEXT,
            Player.COMMAND_SEEK_TO_MEDIA_ITEM,
            Player.COMMAND_SET_SHUFFLE_MODE,
            Player.COMMAND_SET_REPEAT_MODE,
            Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
            Player.COMMAND_GET_TIMELINE,
            Player.COMMAND_GET_METADATA,
            Player.COMMAND_SET_MEDIA_ITEM,
            Player.COMMAND_CHANGE_MEDIA_ITEMS,
            Player.COMMAND_GET_AUDIO_ATTRIBUTES,
            Player.COMMAND_RELEASE,
        ).build()
    }
}
