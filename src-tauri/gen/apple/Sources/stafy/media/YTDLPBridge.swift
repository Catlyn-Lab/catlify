import Foundation
import YTDLP

/// Gives the Rust engine yt-dlp on iPhone/iPad: the ytdlp-apple package (github.com/Catlyn-Lab/ytdlp-apple)
/// runs it in-process on an embedded Python, because iOS apps can't start programs.
/// Rust side: src-tauri/src/ytrun.rs (lookups, downloads, full-song streams, "See video").
enum YTDLPBridge {
    static func register() {
        stafy_set_ytdlp(run, release)
        // Python takes about a second to start; do it now, not when the first song is tapped.
        DispatchQueue.global(qos: .utility).async {
            do { try YTDLP.shared.start() } catch { NSLog("[Catlify] yt-dlp unavailable: %@", "\(error)") }
            if ProcessInfo.processInfo.environment["CATLIFY_YTDLP_SELFTEST"] != nil { selfTest() }
        }
    }

    /// Diagnostics (simulator / Xcode): `CATLIFY_YTDLP_SELFTEST=1` looks up a song on launch and logs it.
    private static func selfTest() {
        let started = Date()
        do {
            let v = try YTDLP.shared.versions()
            let out = try YTDLP.shared.run(["-J", "--no-warnings", "--no-playlist", "-v", "-f", "bestaudio[ext=m4a]/bestaudio",
                                            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"])
            let info = (try? JSONSerialization.jsonObject(with: Data(out.stdout.utf8))) as? [String: Any]
            let jsc = out.stderr.contains("javascriptcore")
            NSLog("[Catlify] yt-dlp self-test: code=%d title=%@ hasURL=%d jsc=%d versions=%@ in %.1fs %@",
                  out.code, (info?["title"] as? String) ?? "-", (info?["url"] as? String) != nil ? 1 : 0, jsc ? 1 : 0,
                  "\(v)", Date().timeIntervalSince(started), out.code == 0 ? "" : (out.errorMessage ?? ""))
        } catch {
            NSLog("[Catlify] yt-dlp self-test failed: %@", "\(error)")
        }
        // The app's own path: Rust engine → ytrun.rs → this bridge (a chart song, matched by search).
        let t = Date()
        let id = StafyEngine.call("ytVideoId", ["path": "ytq:LANY Stuck"]) as? String
        NSLog("[Catlify] yt-dlp via engine: ytq:LANY Stuck -> %@ in %.1fs", id ?? "FAILED", Date().timeIntervalSince(t))
    }

    private static func reply(_ object: [String: Any]) -> UnsafeMutablePointer<CChar>? {
        let data = (try? JSONSerialization.data(withJSONObject: object)) ?? Data("{}".utf8)
        return strdup(String(decoding: data, as: UTF8.self))
    }

    /// `{"args": [...], "id"?}` → `{"code", "stdout", "stderr"}`; `{"cancel": id}` stops a run.
    private static let run: @convention(c) (UnsafePointer<CChar>?, stafy_yt_line?, UnsafeMutableRawPointer?) -> UnsafeMutablePointer<CChar>? = { request, line, context in
        guard let request,
              let object = try? JSONSerialization.jsonObject(with: Data(String(cString: request).utf8)) as? [String: Any]
        else { return reply(["error": "bad request"]) }
        if let id = object["cancel"] as? String {
            YTDLP.shared.cancel(id: id)
            return reply(["ok": true])
        }
        var onLine: ((String, Bool) -> Void)?
        if let line {
            onLine = { text, isError in text.withCString { line(context, $0, isError ? 1 : 0) } }
        }
        do {
            let out = try YTDLP.shared.run(object["args"] as? [String] ?? [], id: object["id"] as? String, onLine: onLine)
            return reply(["code": Int(out.code), "stdout": out.stdout, "stderr": out.stderr])
        } catch {
            return reply(["error": "\(error)"])
        }
    }

    private static let release: @convention(c) (UnsafeMutablePointer<CChar>?) -> Void = { p in free(p) }
}
