import AVFoundation
import MediaPlayer
import UIKit

/// Lock screen, Control Center, Bluetooth (AVRCP: car displays + steering-wheel buttons),
/// CarPlay Now Playing, audio session interruptions and route changes.
final class NowPlaying {
    static let shared = NowPlaying()

    private var started = false
    private var lastTrackKey = ""
    private var artwork: MPMediaItemArtwork?
    private var wasPlayingBeforeInterruption = false
    /// Many head units (Mazda Connect included) send "play" right after Bluetooth connects.
    private var ignorePlayUntil = Date.distantPast
    private var seekTimer: Timer?
    private(set) var state: [String: Any] = [:]

    func start() {
        guard !started else { return }
        started = true

        let fm = FileManager.default
        let id = Bundle.main.bundleIdentifier ?? "app.stafy.player"
        // Must match Tauri's app_data_dir / app_cache_dir so UI and CarPlay share one library.
        let data = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent(id)
        let cache = fm.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent(id)
        try? fm.createDirectory(at: data, withIntermediateDirectories: true)
        try? fm.createDirectory(at: cache, withIntermediateDirectories: true)
        guard stafy_init(data.path, cache.path) else {
            NSLog("[Catlify] engine init failed")
            return
        }

        configureAudioSession()
        registerRemoteCommands()
        stafy_set_listener { json, trackChanged in
            guard let json, let data = String(cString: json).data(using: .utf8),
                  let state = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
            DispatchQueue.main.async { NowPlaying.shared.apply(state, trackChanged: trackChanged) }
        }
    }

    // MARK: Audio session

    private func configureAudioSession() {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.playback, mode: .default, options: [.allowAirPlay, .allowBluetoothA2DP])
            try session.setActive(true)
        } catch {
            NSLog("[Catlify] audio session: %@", error.localizedDescription)
        }
        let nc = NotificationCenter.default
        nc.addObserver(self, selector: #selector(interrupted(_:)), name: AVAudioSession.interruptionNotification, object: session)
        nc.addObserver(self, selector: #selector(routeChanged(_:)), name: AVAudioSession.routeChangeNotification, object: session)
        nc.addObserver(self, selector: #selector(mediaServicesReset), name: AVAudioSession.mediaServicesWereResetNotification, object: session)
    }

    @objc private func interrupted(_ note: Notification) {
        guard let raw = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
              let type = AVAudioSession.InterruptionType(rawValue: raw) else { return }
        switch type {
        case .began: // phone call, Siri, alarm
            wasPlayingBeforeInterruption = state["playing"] as? Bool ?? false
            StafyEngine.control("pause")
        case .ended:
            try? AVAudioSession.sharedInstance().setActive(true)
            let opts = AVAudioSession.InterruptionOptions(rawValue: note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0)
            if wasPlayingBeforeInterruption && opts.contains(.shouldResume) {
                StafyEngine.control("play")
            }
        @unknown default: break
        }
    }

    @objc private func routeChanged(_ note: Notification) {
        guard let raw = note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
              let reason = AVAudioSession.RouteChangeReason(rawValue: raw) else { return }
        switch reason {
        case .oldDeviceUnavailable:
            // Car/headphones disconnected: never fall back to blasting the phone speaker.
            StafyEngine.control("pause")
        case .newDeviceAvailable:
            let carish: Set<AVAudioSession.Port> = [.bluetoothA2DP, .bluetoothHFP, .carAudio, .bluetoothLE]
            let isCar = AVAudioSession.sharedInstance().currentRoute.outputs.contains { carish.contains($0.portType) }
            if isCar && !StafyEngine.carAutoplay {
                ignorePlayUntil = Date().addingTimeInterval(5)
            }
        default: break
        }
    }

    @objc private func mediaServicesReset() {
        configureAudioSession()
    }

    // MARK: Remote commands (lock screen, AVRCP, CarPlay, Siri)

    private func registerRemoteCommands() {
        let c = MPRemoteCommandCenter.shared()
        c.playCommand.addTarget { [weak self] _ in
            guard let self, Date() >= self.ignorePlayUntil else { return .commandFailed }
            try? AVAudioSession.sharedInstance().setActive(true)
            StafyEngine.control("play")
            return .success
        }
        c.pauseCommand.addTarget { _ in StafyEngine.control("pause"); return .success }
        c.stopCommand.addTarget { _ in StafyEngine.control("pause"); return .success }
        c.togglePlayPauseCommand.addTarget { _ in StafyEngine.control("toggle"); return .success }
        c.nextTrackCommand.addTarget { _ in StafyEngine.control("next"); return .success }
        c.previousTrackCommand.addTarget { _ in StafyEngine.control("previous"); return .success }
        c.changePlaybackPositionCommand.addTarget { e in
            guard let e = e as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            StafyEngine.call("seek", ["position": e.positionTime])
            return .success
        }
        // Long-press on steering-wheel next/previous arrives as seek forward/backward.
        c.seekForwardCommand.addTarget { [weak self] e in self?.continuousSeek(e, delta: 5) ?? .commandFailed }
        c.seekBackwardCommand.addTarget { [weak self] e in self?.continuousSeek(e, delta: -5) ?? .commandFailed }
        c.changeShuffleModeCommand.addTarget { e in
            guard let e = e as? MPChangeShuffleModeCommandEvent else { return .commandFailed }
            StafyEngine.call("shuffle", ["on": e.shuffleType != .off])
            return .success
        }
        c.changeRepeatModeCommand.addTarget { e in
            guard let e = e as? MPChangeRepeatModeCommandEvent else { return .commandFailed }
            StafyEngine.call("repeat", ["mode": e.repeatType == .one ? "one" : e.repeatType == .all ? "all" : "off"])
            return .success
        }
        UIApplication.shared.beginReceivingRemoteControlEvents()
    }

    private func continuousSeek(_ e: MPRemoteCommandEvent, delta: Double) -> MPRemoteCommandHandlerStatus {
        guard let e = e as? MPSeekCommandEvent else { return .commandFailed }
        seekTimer?.invalidate()
        if e.type == .beginSeeking {
            seekTimer = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: true) { _ in
                StafyEngine.call("seekBy", ["delta": delta])
            }
            seekTimer?.fire()
        }
        return .success
    }

    // MARK: Now Playing info

    private func apply(_ s: [String: Any], trackChanged: Bool) {
        state = s
        let center = MPNowPlayingInfoCenter.default()
        guard let track = s["track"] as? [String: Any] else {
            center.nowPlayingInfo = nil
            center.playbackState = .stopped
            return
        }
        let playing = s["playing"] as? Bool ?? false
        // Head units show blanks or stale text for missing fields, so always send something.
        let title = (track["title"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? "Unknown Title"
        let artist = (track["artist"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? "Unknown Artist"
        let album = (track["album"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? "Unknown Album"
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: title,
            MPMediaItemPropertyArtist: artist,
            MPMediaItemPropertyAlbumTitle: album,
            MPMediaItemPropertyPlaybackDuration: s["duration"] as? Double ?? 0,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: s["position"] as? Double ?? 0,
            MPNowPlayingInfoPropertyPlaybackRate: playing ? 1.0 : 0.0,
            MPNowPlayingInfoPropertyDefaultPlaybackRate: 1.0,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue,
        ]
        if let index = s["index"] as? Int {
            info[MPNowPlayingInfoPropertyPlaybackQueueIndex] = index
        }

        let key = "\(track["path"] ?? "")"
        if key != lastTrackKey {
            lastTrackKey = key
            artwork = nil
            loadArtwork(track["cover"] as? String, for: key)
        }
        if let artwork { info[MPMediaItemPropertyArtwork] = artwork }
        center.nowPlayingInfo = info
        center.playbackState = playing ? .playing : .paused

        let c = MPRemoteCommandCenter.shared()
        c.changeShuffleModeCommand.currentShuffleType = (s["shuffle"] as? Bool ?? false) ? .items : .off
        let repeatMode = s["repeat"] as? String ?? "off"
        c.changeRepeatModeCommand.currentRepeatType = repeatMode == "one" ? .one : repeatMode == "all" ? .all : .off
    }

    private func loadArtwork(_ cover: String?, for key: String) {
        guard let cover else { return }
        let set: (UIImage) -> Void = { [weak self] image in
            DispatchQueue.main.async {
                guard let self, self.lastTrackKey == key else { return }
                self.artwork = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
                var info = MPNowPlayingInfoCenter.default().nowPlayingInfo ?? [:]
                info[MPMediaItemPropertyArtwork] = self.artwork
                MPNowPlayingInfoCenter.default().nowPlayingInfo = info
            }
        }
        if cover.hasPrefix("http"), let url = URL(string: cover) {
            URLSession.shared.dataTask(with: url) { data, _, _ in
                if let data, let image = UIImage(data: data) { set(image) }
            }.resume()
        } else if let image = UIImage(contentsOfFile: cover) {
            set(image)
        }
    }
}
