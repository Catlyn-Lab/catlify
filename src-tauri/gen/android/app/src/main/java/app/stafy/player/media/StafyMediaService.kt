package app.stafy.player.media

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.core.content.ContextCompat
import androidx.media.AudioAttributesCompat
import androidx.media.AudioFocusRequestCompat
import androidx.media.AudioManagerCompat
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.CommandButton
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionError
import androidx.media3.session.SessionResult
import app.stafy.player.R
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors
import java.util.concurrent.Executors
import org.json.JSONObject

/**
 * Hosts the media session for Android Auto, Bluetooth/AVRCP head units (e.g. Mazda Connect),
 * the notification and lock screen. Works with no UI running: a car can start it cold.
 */
@UnstableApi
class StafyMediaService : MediaLibraryService() {
    private lateinit var player: StafyPlayer
    private lateinit var session: MediaLibrarySession
    private lateinit var audio: AudioManager
    private val main = Handler(Looper.getMainLooper())
    private var focusRequest: AudioFocusRequestCompat? = null
    private var resumeOnFocusGain = false
    /** Set when CarConnectReceiver reports a car; playback resumes once its audio is routed. */
    private var carPendingUntil = 0L
    /** Browse lists can need the network (charts, lyrics): never build them on the main thread. */
    private val browse = MoreExecutors.listeningDecorator(Executors.newSingleThreadExecutor())
    /** Position in SLEEP_STEPS of the car's sleep timer button; -1 = off. */
    private var sleepStep = -1

    override fun onCreate() {
        super.onCreate()
        // Same directories Tauri uses (PathPlugin: dataDir / cacheDir), so UI and car share one library.
        StafyNative.init(dataDir.absolutePath, cacheDir.absolutePath)
        // Creates Android/data/<app>/files: Wi-Fi sync keeps formats Music/ rejects (WavPack, TTA) there.
        getExternalFilesDir(null)
        // Full songs from YouTube (charts, search) via the bundled yt-dlp; no-op in store builds.
        app.stafy.player.youtube.StafyYouTube.registerStreams(applicationContext)
        audio = getSystemService(AUDIO_SERVICE) as AudioManager

        player = StafyPlayer { CoverProvider.uriFor(this, it) }
        player.onPlayingChanged = { playing ->
            if (playing) requestFocus()
            // The sleep timer pausing is a playing change too: update the button's label.
            if (::session.isInitialized) updateSleepButton()
        }
        player.onTrackChanged = {
            // The lyrics entry and its lines follow the song that's playing.
            if (::session.isInitialized) {
                session.notifyChildrenChanged("recent", Int.MAX_VALUE, null)
                session.notifyChildrenChanged("lyrics", Int.MAX_VALUE, null)
            }
        }

        session = MediaLibrarySession.Builder(this, player, LibraryCallback())
            .setId("stafy")
            .setMediaButtonPreferences(listOf(sleepButton()))
            .build()

        audio.registerAudioDeviceCallback(deviceCallback, main)
        ContextCompat.registerReceiver(
            this, noisyReceiver, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), ContextCompat.RECEIVER_NOT_EXPORTED
        )
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession = session

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_CAR_CONNECTED) onCarConnected()
        return super.onStartCommand(intent, flags, startId)
    }

    override fun onDestroy() {
        main.removeCallbacks(carResume)
        browse.shutdown()
        audio.unregisterAudioDeviceCallback(deviceCallback)
        unregisterReceiver(noisyReceiver)
        abandonFocus()
        session.release()
        player.release()
        super.onDestroy()
    }

    // ---- Audio focus: pause for calls, resume after; navigation prompts duck automatically ----

    private val focusListener = AudioManager.OnAudioFocusChangeListener { change ->
        when (change) {
            AudioManager.AUDIOFOCUS_LOSS -> {
                resumeOnFocusGain = false
                StafyNative.control("pause")
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                resumeOnFocusGain = player.isPlaying
                StafyNative.control("pause")
            }
            AudioManager.AUDIOFOCUS_GAIN -> if (resumeOnFocusGain) {
                resumeOnFocusGain = false
                StafyNative.control("play")
            }
        }
    }

    private fun requestFocus() {
        val request = focusRequest ?: AudioFocusRequestCompat.Builder(AudioManagerCompat.AUDIOFOCUS_GAIN)
            .setAudioAttributes(
                AudioAttributesCompat.Builder()
                    .setUsage(AudioAttributesCompat.USAGE_MEDIA)
                    .setContentType(AudioAttributesCompat.CONTENT_TYPE_MUSIC)
                    .build()
            )
            .setOnAudioFocusChangeListener(focusListener, main)
            .build()
            .also { focusRequest = it }
        AudioManagerCompat.requestAudioFocus(audio, request)
    }

    private fun abandonFocus() {
        focusRequest?.let { AudioManagerCompat.abandonAudioFocusRequest(audio, it) }
    }

    // ---- Car / Bluetooth routing ----

    private val noisyReceiver = object : BroadcastReceiver() {
        // Headphones unplugged or Bluetooth dropped: never continue on the phone speaker.
        override fun onReceive(context: Context, intent: Intent) {
            StafyNative.control("pause")
        }
    }

    private val deviceCallback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) {
            val car = added.any { it.isSink && it.type in CAR_OUTPUTS }
            if (!car) return
            // Many head units (Mazda Connect among them) send PLAY as soon as Bluetooth connects.
            if (!StafyNative.carAutoplay) {
                player.ignorePlayUntil = System.currentTimeMillis() + 5000
            } else if (System.currentTimeMillis() < carPendingUntil && added.any { it.isSink && it.type in BLUETOOTH_OUTPUTS }) {
                scheduleCarResume()
            }
        }
    }

    /** A car's Bluetooth just connected (CarConnectReceiver). */
    private fun onCarConnected() {
        if (!StafyNative.carAutoplay) return
        // ACL comes first; the audio (A2DP) link follows a few seconds later. Wait for it so
        // nothing plays from the phone speaker.
        carPendingUntil = System.currentTimeMillis() + 30_000
        val routed = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS).any { it.type in BLUETOOTH_OUTPUTS }
        if (routed) scheduleCarResume()
    }

    private fun scheduleCarResume() {
        main.removeCallbacks(carResume)
        // Give the head unit's own PLAY a moment to arrive first; then make sure we're playing.
        main.postDelayed(carResume, 2500)
    }

    private val carResume = Runnable {
        carPendingUntil = 0
        val now = StafyNative.rpc("session") as? JSONObject ?: return@Runnable
        if (now.optBoolean("playing")) return@Runnable
        if ((now.optJSONArray("items")?.length() ?: 0) > 0) {
            StafyNative.control("play") // where it paused when the car turned off
        } else {
            StafyNative.rpc("play", JSONObject().put("parent", "recent"))
        }
    }

    // ---- Sleep timer button (Android Auto, notification): tap to step through the options ----

    private fun sleepButton(): CommandButton {
        val st = StafyNative.rpc("sleepStatus") as? JSONObject
        if (st?.optBoolean("active") != true) sleepStep = -1
        val label = when {
            st?.optBoolean("endOfTrack") == true -> "Sleep: end of song"
            st?.optBoolean("active") == true -> {
                val left = ((st.optLong("endsAt") - System.currentTimeMillis()) / 60_000.0).let { Math.ceil(it).toInt().coerceAtLeast(1) }
                "Sleep: $left min"
            }
            else -> "Sleep timer"
        }
        return CommandButton.Builder(CommandButton.ICON_UNDEFINED)
            .setCustomIconResId(R.drawable.ic_sleep)
            .setDisplayName(label)
            .setSessionCommand(SLEEP)
            .build()
    }

    private fun updateSleepButton() = session.setMediaButtonPreferences(listOf(sleepButton()))

    /** Off → 15 → 30 → 60 min → end of song → off. */
    private fun stepSleepTimer() {
        sleepStep = if (sleepStep + 1 >= SLEEP_STEPS.size) -1 else sleepStep + 1
        when (val step = SLEEP_STEPS.getOrNull(sleepStep)) {
            null -> StafyNative.rpc("sleepCancel")
            0 -> StafyNative.rpc("sleepSet", JSONObject().put("endOfTrack", true))
            else -> StafyNative.rpc("sleepSet", JSONObject().put("minutes", step))
        }
        val keep = sleepStep
        updateSleepButton()
        sleepStep = keep
    }

    // ---- Browse tree for Android Auto, AVRCP browsing and other controllers ----

    private inner class LibraryCallback : MediaLibrarySession.Callback {
        override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult =
            MediaSession.ConnectionResult.AcceptedResultBuilder(session)
                .setAvailableSessionCommands(
                    MediaSession.ConnectionResult.DEFAULT_SESSION_AND_LIBRARY_COMMANDS.buildUpon().add(SLEEP).build()
                )
                .build()

        override fun onCustomCommand(
            session: MediaSession, controller: MediaSession.ControllerInfo, customCommand: SessionCommand, args: Bundle
        ): ListenableFuture<SessionResult> {
            if (customCommand.customAction != SLEEP.customAction) {
                return Futures.immediateFuture(SessionResult(SessionError.ERROR_NOT_SUPPORTED))
            }
            stepSleepTimer()
            return Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
        }

        override fun onGetLibraryRoot(
            session: MediaLibrarySession, browser: MediaSession.ControllerInfo, params: LibraryParams?
        ): ListenableFuture<LibraryResult<MediaItem>> {
            // System UI asks for a "recent" root to offer playback resumption.
            val rootId = if (params?.isRecent == true) "recent" else "root"
            val extras = Bundle().apply {
                putBoolean("android.media.browse.CONTENT_STYLE_SUPPORTED", true)
                putInt("android.media.browse.CONTENT_STYLE_BROWSABLE_HINT", 1) // list
                putInt("android.media.browse.CONTENT_STYLE_PLAYABLE_HINT", 1)
                putBoolean("android.media.browse.SEARCH_SUPPORTED", true)
            }
            val root = browsable(rootId, "Catlify")
            return Futures.immediateFuture(LibraryResult.ofItem(root, LibraryParams.Builder().setExtras(extras).build()))
        }

        override fun onGetChildren(
            session: MediaLibrarySession, browser: MediaSession.ControllerInfo, parentId: String,
            page: Int, pageSize: Int, params: LibraryParams?
        ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
            return browse.submit<LibraryResult<ImmutableList<MediaItem>>> {
                val all = nodes(parentId)
                val from = (page * pageSize).coerceAtMost(all.size)
                val to = (from + pageSize).coerceAtMost(all.size)
                LibraryResult.ofItemList(ImmutableList.copyOf(all.subList(from, to)), params)
            }
        }

        override fun onGetItem(
            session: MediaLibrarySession, browser: MediaSession.ControllerInfo, mediaId: String
        ): ListenableFuture<LibraryResult<MediaItem>> =
            Futures.immediateFuture(LibraryResult.ofItem(browsable(mediaId, mediaId), null))

        override fun onSearch(
            session: MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, params: LibraryParams?
        ): ListenableFuture<LibraryResult<Void>> {
            return browse.submit<LibraryResult<Void>> {
                val count = nodes("search:$query").size
                main.post { session.notifySearchResultChanged(browser, query, count, params) }
                LibraryResult.ofVoid()
            }
        }

        override fun onGetSearchResult(
            session: MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String,
            page: Int, pageSize: Int, params: LibraryParams?
        ): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> =
            onGetChildren(session, browser, "search:$query", page, pageSize, params)

        // Controllers send bare ids; the player resolves them through Rust, so pass them through.
        override fun onAddMediaItems(
            mediaSession: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>
        ): ListenableFuture<MutableList<MediaItem>> = Futures.immediateFuture(mediaItems)

        /** Bluetooth "play" with nothing loaded (e.g. car just started): resume recent songs. */
        override fun onPlaybackResumption(
            mediaSession: MediaSession, controller: MediaSession.ControllerInfo
        ): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
            if (System.currentTimeMillis() < player.ignorePlayUntil || !StafyNative.carAutoplay) {
                return Futures.immediateFailedFuture(UnsupportedOperationException("auto-play disabled"))
            }
            val items = nodes("recent").filter { it.mediaMetadata.isPlayable == true }
            return Futures.immediateFuture(MediaSession.MediaItemsWithStartPosition(items, 0, 0))
        }
    }

    private fun nodes(parent: String): List<MediaItem> {
        val arr = StafyNative.children(parent)
        return (0 until arr.length()).map { i ->
            val n: JSONObject = arr.getJSONObject(i)
            val id = n.optString("id")
            val playable = n.optBoolean("playable")
            // Section headings in Android Auto lists ("Genres", "Moods", "Now playing").
            val extras = n.optString("group").ifEmpty { null }?.let { group ->
                Bundle().apply { putString("android.media.browse.CONTENT_STYLE_GROUP_TITLE_HINT", group) }
            }
            val meta = MediaMetadata.Builder()
                .setTitle(n.optString("title"))
                .setSubtitle(n.optString("subtitle").ifEmpty { null })
                .setArtist(n.optString("subtitle").ifEmpty { null })
                .setArtworkUri(CoverProvider.uriFor(this, n.optString("art").ifEmpty { null }))
                .setIsPlayable(playable)
                .setIsBrowsable(n.optBoolean("browsable"))
                .setMediaType(
                    when (n.optString("kind")) {
                        "album" -> MediaMetadata.MEDIA_TYPE_ALBUM
                        "artist" -> MediaMetadata.MEDIA_TYPE_ARTIST
                        "playlist" -> MediaMetadata.MEDIA_TYPE_PLAYLIST
                        "track" -> MediaMetadata.MEDIA_TYPE_MUSIC
                        "lyric" -> null // a line of text; tapping it jumps there
                        else -> MediaMetadata.MEDIA_TYPE_FOLDER_MIXED
                    }
                )
                .setExtras(extras)
                .build()
            // Playables carry their parent so tapping one plays the surrounding list.
            MediaItem.Builder().setMediaId(if (playable) "$parent|$id" else id).setMediaMetadata(meta).build()
        }
    }

    private fun browsable(id: String, title: String) = MediaItem.Builder()
        .setMediaId(id)
        .setMediaMetadata(
            MediaMetadata.Builder().setTitle(title).setIsBrowsable(true).setIsPlayable(false)
                .setMediaType(MediaMetadata.MEDIA_TYPE_FOLDER_MIXED).build()
        )
        .build()

    companion object {
        const val ACTION_CAR_CONNECTED = "app.stafy.player.action.CAR_CONNECTED"

        private val SLEEP = SessionCommand("app.stafy.player.SLEEP_TIMER", Bundle.EMPTY)
        /** Minutes; 0 = end of the current song. */
        private val SLEEP_STEPS = listOf(15, 30, 60, 0)

        private val BLUETOOTH_OUTPUTS = buildSet {
            add(AudioDeviceInfo.TYPE_BLUETOOTH_A2DP)
            if (Build.VERSION.SDK_INT >= 31) add(AudioDeviceInfo.TYPE_BLE_HEADSET)
        }
        private val CAR_OUTPUTS = buildSet {
            add(AudioDeviceInfo.TYPE_BLUETOOTH_A2DP)
            if (Build.VERSION.SDK_INT >= 31) add(AudioDeviceInfo.TYPE_BLE_HEADSET)
            if (Build.VERSION.SDK_INT >= 33) add(AudioDeviceInfo.TYPE_BLE_BROADCAST)
            add(AudioDeviceInfo.TYPE_USB_DEVICE)
        }
    }
}
