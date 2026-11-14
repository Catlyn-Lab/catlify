package app.stafy.player.media

import org.json.JSONArray
import org.json.JSONObject

/** JNI surface of the Rust core (src-tauri/src/jni_bridge.rs). Shared by the UI and the media service. */
object StafyNative {
    init {
        System.loadLibrary("stafy_lib")
    }

    interface Listener {
        /** Called on a Rust thread whenever playback state changes. */
        fun onState(json: String, trackChanged: Boolean)
    }

    @JvmStatic external fun init(dataDir: String, cacheDir: String): Boolean
    @JvmStatic external fun call(method: String, args: String): String
    @JvmStatic external fun setListener(listener: Listener): Boolean

    /** Fetches YouTube audio ("yt:<id>" / "ytq:<query>") to `stem.<ext>`; returns the path or "ERR:<msg>". */
    interface StreamResolver {
        fun fetch(spec: String, stem: String): String
        /** yt-dlp's JSON (-J) for the audio link only; Rust downloads it. "ERR:<msg>" on failure. */
        fun extract(spec: String): String
    }

    @JvmStatic external fun setStreamResolver(resolver: StreamResolver): Boolean

    fun rpc(method: String, args: JSONObject = JSONObject()): Any? {
        val out = JSONObject(call(method, args.toString()))
        if (out.has("error")) {
            android.util.Log.w("Catlify", "$method failed: ${out.getString("error")}")
            return null
        }
        return out.opt("ok")
    }

    fun control(action: String) = rpc("control", JSONObject().put("action", action))

    fun children(parent: String): JSONArray = rpc("children", JSONObject().put("parent", parent)) as? JSONArray ?: JSONArray()

    val carAutoplay: Boolean
        get() = (rpc("settings") as? JSONObject)?.optBoolean("car_autoplay", true) ?: true
}
