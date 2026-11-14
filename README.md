<div align="center">

<img src="design/icon-rounded.png" width="128" alt="Catlify icon">

# Catlify

**A lightweight, lossless-first music player for every screen you own.**<br>
Your files in full quality, live charts, synced lyrics, YouTube, and your car, in one small app.

[![Release](https://img.shields.io/github/v/release/Catlyn-Lab/catlify?label=download&color=6d28d9)](https://github.com/Catlyn-Lab/catlify/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/Catlyn-Lab/catlify/release.yml?label=release%20build)](https://github.com/Catlyn-Lab/catlify/actions/workflows/release.yml)
![Platforms](https://img.shields.io/badge/macOS%20·%20Windows%20·%20Linux%20·%20Android%20·%20iOS-111)
![Tauri 2](https://img.shields.io/badge/Tauri-2-24c8db?logo=tauri&logoColor=white)
![Svelte 5](https://img.shields.io/badge/Svelte-5-ff3e00?logo=svelte&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-engine-b7410e?logo=rust&logoColor=white)

[Download](#️-download) · [Features](#-features) · [In the car](#-in-the-car) · [YouTube](#️-youtube) · [Develop](#-develop) · [Releasing](#-releasing)

</div>

---

## ⬇️ Download

| Platform | Get it | Notes |
|---|---|---|
| **macOS** (Apple silicon) | [`.dmg` from the latest release](https://github.com/Catlyn-Lab/catlify/releases/latest) | Not notarized yet: open it the first time with right-click → **Open** |
| **Windows** 10/11 (x64) | [`-setup.exe` or `.msi`](https://github.com/Catlyn-Lab/catlify/releases/latest) | For YouTube: `winget install yt-dlp.yt-dlp` |
| **Android** | Firebase App Distribution (testers) | Android Auto included |
| **iPhone / iPad** | Build from source | CarPlay included |
| **Linux** | Build from source | |

> [!TIP]
> On macOS and Windows, Catlify updates itself: when a new version is out you'll get a popup with what's new,
> and **Settings → Updates** has a "Check for updates" button.

## ✨ Features

<table>
<tr>
<td width="50%" valign="top">

### 🎧 Sounds like the file
Gapless playback with a sample-accurate engine. FLAC, ALAC, WAV, AIFF, APE, WavPack, TTA, TAK,
WMA Lossless, **DSD** (DSF/DFF), Opus, Vorbis, MP3, AAC and more, with hi-res and lossless badges.

</td>
<td width="50%" valign="top">

### 📚 Your library
Fast incremental folder scanning, albums, artists, songs, liked songs, playlists, search and play counts.
Songs that go missing are hidden, never deleted.

</td>
</tr>
<tr>
<td valign="top">

### 🎤 Synced lyrics
Karaoke-style lyrics: tap a line to jump there. From `.lrc` files, embedded tags or
[LRCLIB](https://lrclib.net), cached for offline. Shown on CarPlay and Android Auto too.

</td>
<td valign="top">

### 📈 Charts, genres and moods
Live Apple Music top songs for 23 countries, the Deezer global chart, 18 genres and 12 moods
(chill, focus, workout, road trip…). Songs you own play from your files.

</td>
</tr>
<tr>
<td valign="top">

### ▶️ YouTube
Search or paste a link, watch with **See video** in fullscreen, or download any song, even the one
playing from a chart, with tags and cover art.

</td>
<td valign="top">

### 🚗 Made for the car
Android Auto, CarPlay and Bluetooth with title, artist and artwork, steering-wheel controls,
and playback that resumes on its own when the car connects.

</td>
</tr>
<tr>
<td valign="top">

### 🔄 Wi-Fi sync, no cloud
Pair devices with a 6-digit code and sync songs, likes, play counts and playlists both ways
over your home network.

</td>
<td valign="top">

### 📥 Bring your playlists
Import from Spotify, Apple Music, YouTube Music, Google Takeout or any CSV. Missing songs are
matched and downloaded, and progress survives a restart.

</td>
</tr>
<tr>
<td valign="top">

### 🌙 Sleep timer
15, 30 or 60 minutes, or end of track, with a gentle fade-out. Also on the car screen.

</td>
<td valign="top">

### 🏝️ Notch player (macOS)
A tiny island around the MacBook camera: artwork and a level meter, full controls on hover.

</td>
</tr>
</table>

### ⌨️ Keyboard

| Key | Action | Key | Action |
|---|---|---|---|
| `Space` | Play / pause | `⌘F` | Search |
| `←` `→` | Seek 5 s | `L` | Lyrics |
| `⌘←` `⌘→` | Previous / next | `Q` | Queue |
| `⌘↑` `⌘↓` | Volume | `⌘[` | Back |

## 🚗 In the car

Every car path goes through the OS media session, so the engine runs inside a native media service that works
without the UI. A car can start it cold.

| | Android | iPhone |
|---|---|---|
| **Car screen** | Android Auto: Recent · Library · Browse · Playlists, search, voice, lyrics, sleep timer | CarPlay: the same tabs, Now Playing with lyrics and sleep buttons |
| **Bluetooth** | Title, artist, album, artwork, duration, position, track n/N, steering-wheel next/prev | Now Playing info and remote commands, including seek |
| **Car connects** | Resumes playback even when the app isn't open (needs the "Nearby devices" permission) | Can ignore the head unit's auto-play |
| **Car disconnects** | Pauses | Pauses |
| **Calls / navigation** | Pauses for calls, ducks for directions | Pauses and resumes |

> [!TIP]
> Bluetooth to a car is always lossy (SBC/AAC). Wired CarPlay or Android Auto sounds better.

<details>
<summary><b>Testing in a car simulator</b></summary>

- **Android Auto**: install Android Auto on the emulator or phone, enable developer mode → "Start head unit server",
  run `adb forward tcp:5277 tcp:5277`, then `$ANDROID_HOME/extras/google/auto/desktop-head-unit`.
- **CarPlay**: Xcode → Simulator → I/O → External Displays → CarPlay.
- Shipping CarPlay needs Apple's `com.apple.developer.carplay-audio` entitlement. The simulator works without it.

</details>

## ▶️ YouTube

Paste a link or search. Catlify guesses the artist and title, lets you edit them, saves the song to
`~/Music/Catlify` (on iPhone: Files → On My iPhone → Catlify → YouTube) and adds it to the library with tags and cover art.

| Platform | yt-dlp | MP3 |
|---|---|---|
| macOS, iPhone, iPad | Built in through [**ytdlp-apple**](https://github.com/Catlyn-Lab/ytdlp-apple), nothing to install | Mac: with `brew install ffmpeg`, otherwise M4A. iPhone/iPad: M4A |
| Android | Built in | ✓ |
| Windows, Linux | Your own `yt-dlp` (`winget install yt-dlp.yt-dlp`) | With ffmpeg, otherwise M4A |

> [!NOTE]
> Only download content you own or have permission to save.

## 🛠 Develop

**You need** Rust, Node 22 and pnpm. For mobile: Xcode (iOS) or Android Studio's SDK and NDK (Android).

```sh
pnpm install
scripts/build-ffmpeg.sh macos-arm64   # once: a minimal static FFmpeg (~3 MB)
pnpm tauri dev                        # run the app
pnpm dev                              # UI only, in a browser, with a mocked backend (?view=billboard&panel=lyrics)
```

<details>
<summary><b>Release builds per platform</b></summary>

```sh
# macOS: bundle yt-dlp (once per ytdlp-apple version), then build
scripts/ytdlp-apple.sh
pnpm tauri build

# iOS (FFMPEG_DIR selects device or simulator libraries)
scripts/build-ffmpeg.sh ios-arm64 && scripts/build-ffmpeg.sh ios-sim-arm64
IOS_SIM=1 scripts/ios.sh dev
scripts/ios.sh build

# Android
scripts/build-ffmpeg.sh android-arm64 && scripts/build-ffmpeg.sh android-x86_64
scripts/android.sh build              # sets SDK/NDK/JDK and the bindgen sysroot
```

Windows links the LGPL FFmpeg 9 DLLs that `FFMPEG_DIR` points to (see `.github/workflows/release.yml`).

</details>

<details>
<summary><b>Tests</b></summary>

```sh
cd src-tauri
STAFY_TEST_MUSIC=/path/to/samples cargo test --lib -- --nocapture
STAFY_TEST_MUSIC=/path/to/samples cargo test --lib engine_ -- --ignored   # plays (muted) through your speakers
```

</details>

<details>
<summary><b>Architecture</b></summary>

| Layer | Tech |
|---|---|
| Shell | Tauri 2 (system webview, no bundled browser) |
| UI | Svelte 5 + Vite (~130 KB JS) |
| Decode | FFmpeg (`ffmpeg-next`): any codec → f32 at the device rate |
| Output | cpal (CoreAudio / WASAPI / ALSA·Pulse / AAudio) through a lock-free ring buffer |
| Tags & art | lofty, with an FFmpeg fallback for DSD/WMA/TAK |
| Storage | SQLite (bundled), numbered migrations |
| Mobile | One shared engine (`core.rs`) behind a JSON RPC (`rpc.rs`) over JNI (`jni_bridge.rs`) and a C ABI (`ffi.rs`) |

```
src/                      Svelte UI
  lib/                    api.ts (typed commands), store.svelte.ts (global state), importer, downloads
  views/                  Home, Billboard, Songs, Albums, Artists, Playlist, Search, YouTube, Settings
  components/             PlayerBar, TrackList, LyricsView, NowPlaying, QueuePanel, VideoPlayer…
src-tauri/src/
  audio/                  decoder (FFmpeg + resampler + seek), gapless engine
  library.rs              scanner, tags, SQLite, playlists, lyrics cache
  charts.rs · lyrics.rs   Apple Music / iTunes / Deezer charts; .lrc / embedded / LRCLIB lyrics
  youtube.rs · ytrun.rs   YouTube downloads; how yt-dlp runs on each platform
  sync.rs · importer.rs   Wi-Fi sync (mDNS + HMAC-signed HTTP), playlist import
  sleep.rs · notch.rs     sleep timer, macOS notch player
src-tauri/gen/android/    Media3 service, Android Auto, Bluetooth car receiver
src-tauri/gen/apple/      CarPlay, Now Playing, ytdlp-apple bridge
```

</details>

## 🚀 Releasing

1. Create a branch `release/<version>`, for example `release/0.1.15`.
2. Set `"version"` in `src-tauri/tauri.conf.json` to the same version.
3. Open a pull request into `main` and merge it.

GitHub Actions then builds macOS (`.dmg`, `.app.zip`) and Windows (`-setup.exe`, `.msi`), tags the merge
commit `v<version>` and publishes the release. To try the builds without releasing, use
**Actions → Release → Run workflow**.

Installed apps find the new version through `latest.json` in the release and offer it in a popup.
Update packages are signed; the app only installs ones that match the public key in `tauri.conf.json`.

<details>
<summary><b>Update signing key (one-time setup)</b></summary>

The private key lives at `~/.tauri/catlify-updater.key` (password in `catlify-updater.key.password`).
Releases need both as repository secrets:

```sh
gh secret set TAURI_SIGNING_PRIVATE_KEY -R Catlyn-Lab/catlify < ~/.tauri/catlify-updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD -R Catlyn-Lab/catlify < ~/.tauri/catlify-updater.key.password
```

Keep a backup of the key: if it's lost, installed apps can't update to newer releases.

</details>

`main` is protected: changes land through pull requests that a Catlyn-Lab admin merges.
