# Catlify

A lightweight, lossless-first music player for macOS, Windows, Linux, Android and iOS.

- **Player**: gapless playback, seek, shuffle, repeat (all/one), queue, volume, OS media keys / Now Playing / Windows media overlay
- **Every format**: FLAC, ALAC, WAV, AIFF, APE, WavPack, TTA, TAK, WMA (Lossless), DSD (DSF/DFF), Opus, Vorbis, MP3, AAC, … via FFmpeg
- **Library**: folder scanning (incremental), albums, artists, songs, liked songs, playlists, search, play counts, hi-res/lossless badges
- **Lyrics**: synced (karaoke-style, click a line to seek) from `.lrc` sidecar files, embedded tags, or [LRCLIB](https://lrclib.net), cached
- **Billboard**: live Apple Music top songs/albums for 23 countries and the Deezer global chart. Songs you own play from your files in full quality; others play 30 s previews

## Stack

| Layer | Tech |
|---|---|
| Shell | Tauri 2 (system webview, so no bundled browser) |
| UI | Svelte 5 + Vite (~130 KB JS) |
| Decode | FFmpeg (`ffmpeg-next`), any codec → f32 at the device rate (swresample) |
| Output | cpal (CoreAudio / WASAPI / ALSA·Pulse / AAudio) via a lock-free ring buffer (`rtrb`) |
| Tags & art | lofty, with an FFmpeg fallback for DSD/WMA/TAK |
| Storage | SQLite (rusqlite, bundled) |
| Media keys | souvlaki (desktop) |

```
src/                      Svelte UI
  lib/api.ts              typed Tauri commands
  lib/store.svelte.ts     global state (player, library, navigation)
  views/                  Home, Billboard, Songs, Albums, Artists, Playlist, Search, Settings
  components/             PlayerBar, TrackList (virtualised), LyricsView, NowPlaying, Queue…
src-tauri/src/
  audio/decoder.rs        FFmpeg decoder + resampler + sample-accurate seek
  audio/engine.rs         gapless engine: decode thread, ring buffer, queue/shuffle/repeat
  library.rs              scanner, tag reader, SQLite, playlists, lyrics cache
  lyrics.rs               .lrc / embedded / LRCLIB lookup
  charts.rs               Apple Music + iTunes previews + Deezer
  media_controls.rs       OS media integration
```

## Develop

Requirements: Rust, Node + pnpm, FFmpeg dev libraries (`brew install ffmpeg pkg-config` on macOS).

```sh
pnpm install
pnpm tauri dev            # run the app
pnpm dev                  # UI only, in a browser, with mocked backend (try ?view=billboard&panel=lyrics)
pnpm tauri build          # release bundle
```

Tests (format coverage + live engine test):

```sh
cd src-tauri
STAFY_TEST_MUSIC=/path/to/samples cargo test --lib -- --nocapture
STAFY_TEST_MUSIC=/path/to/samples cargo test --lib engine_ -- --ignored   # plays (muted) through your speakers
```

## Keyboard

`Space` play/pause · `←/→` seek 5 s · `⌘←/⌘→` previous/next · `⌘↑/⌘↓` volume · `⌘F` search · `L` lyrics · `Q` queue · `⌘[` back

## Mobile, cars and Bluetooth

All car paths go through the OS media session, so the Rust engine runs inside a native media service
that works with no UI (a car can start it cold). UI and service share one engine (`src-tauri/src/core.rs`)
through a JSON RPC (`rpc.rs`) exposed over JNI (`jni_bridge.rs`) and a C ABI (`ffi.rs`).

| | Android (`gen/android/.../media/`) | iOS (`gen/apple/Sources/stafy/media/`) |
|---|---|---|
| Car screen | Android Auto via Media3 `MediaLibraryService`, 4 tabs: Recent · Library · Billboard · Playlists, search, voice | CarPlay `CPTabBarTemplate` with the same tabs + `CPNowPlayingTemplate` |
| Bluetooth (AVRCP) | Title/artist/album/duration/position, track n/N, artwork via `content://`, steering-wheel next/prev, long-press seek | `MPNowPlayingInfoCenter` + `MPRemoteCommandCenter` (incl. seek forward/backward) |
| Car connects | Resumes playback even when the app isn't running (`CarConnectReceiver` on Bluetooth ACL, needs "Nearby devices"), or optionally ignores head-unit auto-play (Mazda Connect sends PLAY on connect) | Optional ignore of head-unit auto-play |
| Car disconnects | Pause (`BECOMING_NOISY`) | Pause (`oldDeviceUnavailable`) |
| Calls / navigation | Audio focus: pause for calls, system ducking for nav prompts | Interruptions: pause + resume |
| Output device changes | Engine reopens the stream (AAudio disconnect on Bluetooth switch) and resumes at the same position | same (desktop polls the default device) |

Notes:
- Bluetooth to a car is lossy (SBC/AAC) regardless of source. Wired CarPlay/Android Auto is the better path.
- CarPlay distribution needs Apple's `com.apple.developer.carplay-audio` entitlement (request at developer.apple.com/carplay). It works in the simulator without it.
- Tauri's iOS runtime assigns its own delegate to every scene; `Bootstrap.swift` routes CarPlay sessions to `CarPlaySceneDelegate`.

### Building

```sh
# Minimal static FFmpeg (≈3.3 MB per target), once per platform:
scripts/build-ffmpeg.sh macos-arm64
scripts/build-ffmpeg.sh ios-arm64 && scripts/build-ffmpeg.sh ios-sim-arm64
scripts/build-ffmpeg.sh android-arm64 && scripts/build-ffmpeg.sh android-x86_64

# iOS (FFMPEG_DIR selects device vs simulator libs)
IOS_SIM=1 scripts/ios.sh dev
scripts/ios.sh build

# Android (lib/arm64 and lib/amd64 are picked per ABI)
scripts/android.sh build   # sets SDK/NDK/JDK + bindgen sysroot
```

Testing in a car simulator:
- **Android Auto**: install Android Auto on the emulator/phone, enable developer mode → "Start head unit server",
  `adb forward tcp:5277 tcp:5277`, then run `$ANDROID_HOME/extras/google/auto/desktop-head-unit`.
- **CarPlay**: Xcode → Simulator → I/O → External Displays → CarPlay.

## Notch player (macOS)

`src-tauri/src/notch.rs` + `src/notch/`. A borderless window at menu-bar level over the camera housing,
sized from `NSScreen.safeAreaInsets` / `auxiliaryTop{Left,Right}Area`. It ignores the mouse except over the visible island,
so menu-bar clicks around it keep working. Collapsed: artwork + level meter beside the camera. On song change: title peek.
On hover: full controls, seek bar and format badge. Displays without a notch get a notch-sized pill. Toggle in Settings.
Closing the main window now hides it (music and notch keep going); the Dock icon brings it back.

## YouTube (desktop, iPhone/iPad, Android)

Paste a link or search; Catlify guesses artist/title from the video, lets you edit them, then saves the song
into `~/Music/Catlify` (changeable; on iPhone: Files → On My iPhone → Catlify → YouTube) and adds it to the
library with tags and cover art. "Download" on a song streaming from a chart or playlist saves that exact video.

How yt-dlp runs (`src-tauri/src/ytrun.rs`):
- **macOS, iPhone, iPad**: [ytdlp-apple](https://github.com/Catlyn-Lab/ytdlp-apple), a Swift package that runs
  yt-dlp on an embedded Python and solves YouTube's JavaScript challenges with JavaScriptCore. iOS links the
  library (`gen/apple/project.yml`, `media/YTDLPBridge.swift`); the Mac app bundles its `ytdlp-apple` command
  (`scripts/ytdlp-apple.sh` before `pnpm tauri build`; `tauri.macos.conf.json`). Nothing to install.
- **Windows, Linux** (and `tauri dev` on a Mac): the user's own `yt-dlp`.
- **Android**: a bundled yt-dlp driven from Kotlin (`StafyYouTube.kt`).

MP3 needs ffmpeg (desktop: `brew install ffmpeg`); without it songs are saved as M4A (YouTube's own AAC audio).
Only download content you own or have permission to save.

## Wi-Fi sync (all platforms)

`src-tauri/src/sync.rs` · Library → Sync devices. No account or cloud. Each device runs a small HTTP server on the
LAN and announces itself over mDNS (`_catlify._tcp`); "Pair by address" covers routers that block discovery.
Pairing: a 6-digit code shown on one device and typed on the other; both derive a shared secret (never sent)
and HMAC-sign every request. Sync is two-way: missing songs (matched by a content fingerprint) are copied into
`Music/Catlify/Synced`, then likes (union), play counts (max) and playlists (by name) are merged. Nothing is deleted.
Meant for trusted home networks (plain HTTP on the LAN; files aren't encrypted in transit).

## Import (desktop, Android)

`src-tauri/src/importer.rs` + `src/lib/importer.svelte.ts` · Library → Import. Public Spotify / Apple Music links
are read from their public pages; YouTube Music playlists via yt-dlp; files: Spotify data export or Exportify CSV,
Apple Music "Export Playlist" (.txt/.xml), Google Takeout CSV, any CSV with title/artist. Songs already in the library
are skipped; the top YouTube result is downloaded when `matchScore` (src/lib/similarity.ts) is ≥ 90, otherwise the
user chooses. Progress is saved and resumes after a restart.

## Library safety

Schema changes are numbered migrations (`MIGRATIONS` in library.rs; never edit a shipped step). Songs a scan can't
see are hidden (`missing_since`), not deleted, and come back with likes/playlists when visible again (purged after
30 days). iOS container moves are followed by rebasing stored paths.

## Status

- macOS desktop: built and tested (format/seek/tag tests, live gapless engine test, real YouTube download test).
- Android: debug APK builds (`scripts/android.sh build --debug --apk`); launches on the emulator, media session registers. minSdk 26 (AAudio).
- iOS: compiles and links for device (unsigned). Signing needs your Apple team; the simulator needs Xcode's iOS 27 simulator SDK.
- Android library scanning uses file paths (`READ_MEDIA_AUDIO`); a Storage Access Framework folder picker is still TODO.
