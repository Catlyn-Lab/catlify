//! YouTube lookup, audio download, playback streams and "See video", all through yt-dlp
//! (see ytrun.rs: Catlify's bundled ytdlp-apple on macOS and iOS, else the user's own yt-dlp).
//! ffmpeg (desktop, optional) is only needed for MP3: without it, YouTube's own M4A audio is saved.

use crate::core;
use crate::ytmeta::{retag, videos_from_json, Video};
use crate::ytrun;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

const SETTING_DIR: &str = "download_dir";

/// ffmpeg, for MP3 conversion and embedding cover art (desktop only).
fn ffmpeg() -> Option<PathBuf> {
    #[cfg(desktop)]
    return ytrun::find_tool("ffmpeg");
    #[cfg(not(desktop))]
    return None;
}

pub fn download_dir(app: &AppHandle) -> PathBuf {
    if let Some(dir) = core::get().and_then(|c| c.library.setting(SETTING_DIR)) {
        return PathBuf::from(dir);
    }
    // iOS: in Documents, which the library scans and the Files app shows (On My iPhone → Catlify).
    #[cfg(target_os = "ios")]
    let base = app.path().document_dir().ok().map(|d| d.join("YouTube"));
    #[cfg(not(target_os = "ios"))]
    let base = app.path().audio_dir().ok().map(|d| d.join("Catlify"));
    base.unwrap_or_else(|| PathBuf::from("Catlify"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    yt_dlp: Option<String>,
    ffmpeg: bool,
    download_dir: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    id: String,
    stage: &'static str, // downloading | converting | done | error | cancelled
    percent: f64,
    path: Option<String>,
    error: Option<String>,
}

/// Runs blocking yt-dlp work off the async runtime.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| anyhow!("{e}"))?
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ---- commands -------------------------------------------------------------------------

#[tauri::command]
pub async fn youtube_status(app: AppHandle) -> Status {
    let version = blocking(|| Ok(ytrun::version())).await.ok().flatten();
    Status { yt_dlp: version, ffmpeg: ffmpeg().is_some(), download_dir: download_dir(&app).to_string_lossy().into() }
}

/// A YouTube link → that video; anything else → search results.
#[tauri::command]
pub async fn youtube_lookup(query: String, playlist: Option<bool>) -> Result<Vec<Video>, String> {
    let q = query.trim().to_string();
    blocking(move || lookup(&q, playlist.unwrap_or(false))).await.map_err(|e| format!("{e:#}"))
}

fn lookup(query: &str, playlist: bool) -> Result<Vec<Video>> {
    if query.is_empty() {
        return Ok(vec![]);
    }
    let is_url = query.starts_with("http") || query.contains("youtu.be/") || query.contains("youtube.com/");
    let mut a = args(&["-J", "--no-warnings"]);
    if is_url && playlist {
        // Import: every song in a YouTube / YouTube Music playlist.
        a.extend(args(&["--flat-playlist", query]));
    } else if is_url {
        a.extend(args(&["--no-playlist", "--skip-download", query]));
    } else {
        a.extend(["--flat-playlist".to_string(), format!("ytsearch15:{query}")]);
    }
    let out = ytrun::run(&a, None, &mut |_, _| {})?;
    if !out.ok() {
        return Err(out.error("yt-dlp failed"));
    }
    let v: Value = serde_json::from_str(&out.stdout).context("parsing yt-dlp output")?;
    Ok(videos_from_json(&v))
}

fn safe_name(s: &str) -> String {
    let s: String = s.chars().map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_start_matches('.').to_string();
    s.chars().take(150).collect()
}

/// Downloads the audio in the background; progress arrives as `youtube://progress` events.
/// `format`: "mp3" (re-encoded, VBR ~245 kbps; needs ffmpeg) or "original" (YouTube's own audio).
#[tauri::command(async)]
pub fn youtube_download(app: AppHandle, url: String, id: String, title: String, artist: String, format: String) -> Result<(), String> {
    if !ytrun::available() {
        return Err("yt-dlp is not installed. Install it with: brew install yt-dlp ffmpeg".into());
    }
    let ffmpeg = ffmpeg();
    if format == "mp3" && ffmpeg.is_none() {
        return Err("MP3 needs ffmpeg (brew install ffmpeg). Choose M4A to save YouTube's own audio instead.".into());
    }
    let dir = download_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;

    tauri::async_runtime::spawn_blocking(move || {
        let emit = |stage, percent, path: Option<String>, error: Option<String>| {
            let _ = app.emit("youtube://progress", Progress { id: id.clone(), stage, percent, path, error });
        };
        emit("downloading", 0.0, None, None);
        match run(ffmpeg.as_deref(), &dir, &url, &id, &title, &artist, &format, &emit) {
            Ok(Some(path)) => {
                if let Some(c) = core::get() {
                    let _ = c.library.add_folder(&dir.to_string_lossy());
                    crate::spawn_scan(app.clone(), c.library.clone());
                }
                emit("done", 100.0, Some(path.to_string_lossy().into()), None);
            }
            Ok(None) => emit("cancelled", 0.0, None, None),
            Err(e) => emit("error", 0.0, None, Some(format!("{e:#}"))),
        }
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run(
    ffmpeg: Option<&Path>,
    dir: &Path,
    url: &str,
    id: &str,
    title: &str,
    artist: &str,
    format: &str,
    emit: &impl Fn(&'static str, f64, Option<String>, Option<String>),
) -> Result<Option<PathBuf>> {
    let name = safe_name(&if artist.trim().is_empty() { title.to_string() } else { format!("{artist} - {title}") });
    let template = dir.join(format!("{}.%(ext)s", name.replace('%', "%%")));
    let mut a = args(&["--no-playlist", "--newline", "--progress", "--no-warnings"]);
    a.extend(args(&["--progress-template", "download:STAFY %(progress._percent_str)s"]));
    a.extend(args(&["--print", "after_move:STAFYPATH %(filepath)s"]));
    a.extend(["-o".to_string(), template.to_string_lossy().into_owned()]);
    match ffmpeg {
        Some(ffmpeg) => {
            a.extend(args(&["-x", "--embed-thumbnail", "--embed-metadata", "--convert-thumbnails", "jpg"]));
            a.extend(["--ffmpeg-location".to_string(), ffmpeg.to_string_lossy().into_owned()]);
            if format == "mp3" {
                a.extend(args(&["-f", "bestaudio", "--audio-format", "mp3", "--audio-quality", "0"]));
            } else {
                // AAC in M4A plays everywhere (CarPlay, Android Auto, Bluetooth) and keeps cover art.
                a.extend(args(&["-f", "bestaudio[ext=m4a]/bestaudio", "--audio-format", "m4a"]));
            }
        }
        // No ffmpeg (iPhone/iPad, or a Mac without Homebrew): YouTube's M4A as is; tags come from retag().
        None => a.extend(args(&["-f", "bestaudio[ext=m4a]/bestaudio"])),
    }
    a.push(url.to_string());

    let mut path = None;
    let out = ytrun::run(&a, Some(id), &mut |line, is_err| {
        if is_err {
            return;
        }
        if let Some(p) = line.strip_prefix("STAFY ") {
            let pct: f64 = p.trim().trim_end_matches('%').trim().parse().unwrap_or(0.0);
            if pct >= 100.0 {
                emit("converting", 100.0, None, None);
            } else {
                emit("downloading", pct, None, None);
            }
        } else if let Some(p) = line.strip_prefix("STAFYPATH ") {
            path = Some(PathBuf::from(p.trim()));
        }
    })?;
    if out.code == ytrun::CANCELLED {
        return Ok(None);
    }
    if !out.ok() {
        return Err(out.error("yt-dlp failed"));
    }
    let path = path.ok_or_else(|| anyhow!("yt-dlp didn't report the output file"))?;
    // Use the (possibly edited) song title/artist instead of the video's metadata.
    retag(&path, title, artist)?;
    Ok(Some(path))
}

fn stream_target(spec: &str) -> Result<String> {
    match (spec.strip_prefix("yt:"), spec.strip_prefix("ytq:")) {
        (Some(id), _) => Ok(format!("https://www.youtube.com/watch?v={id}")),
        // YouTube Music's "songs" results are the audio uploads, not music videos with intros.
        (_, Some(q)) => Ok(format!("https://music.youtube.com/search?q={}#songs", urlencode(q))),
        _ => Err(anyhow!("not a YouTube source: {spec}")),
    }
}

/// Engine stream resolver: downloads the audio of "yt:<id>" / "ytq:<query>" as YouTube serves it
/// (no conversion, so it starts quickly) to `stem.<ext>` for playback without downloading.
pub fn fetch_stream(spec: &str, stem: &Path) -> Result<PathBuf> {
    let mut a = args(&["--no-playlist", "--no-warnings", "-q", "--playlist-items", "1", "-f", "bestaudio[ext=m4a]/bestaudio"]);
    a.extend(args(&["--print", "after_move:filepath", "-o"]));
    a.push(format!("{}.%(ext)s", stem.to_string_lossy().replace('%', "%%")));
    a.push(stream_target(spec)?);
    let out = ytrun::run(&a, None, &mut |_, _| {})?;
    let path = out.stdout.lines().rfind(|l| !l.trim().is_empty()).map(|l| PathBuf::from(l.trim()));
    match path {
        Some(p) if out.ok() && p.exists() => Ok(p),
        _ => Err(out.error("yt-dlp couldn't fetch the song")),
    }
}

/// Engine link lookup: yt-dlp's JSON for the audio of "yt:<id>" / "ytq:<query>" (no download).
pub fn extract_link(spec: &str) -> Result<String> {
    let mut a = args(&["-J", "--no-warnings", "--no-playlist", "--playlist-items", "1", "-f", "bestaudio[ext=m4a]/bestaudio"]);
    // Links are locked to the requesting address; IPv6 privacy addresses rotate, IPv4 doesn't.
    a.push("--force-ipv4".into());
    a.push(stream_target(spec)?);
    let out = ytrun::run(&a, None, &mut |_, _| {})?;
    if !out.ok() {
        return Err(out.error("yt-dlp couldn't find the song"));
    }
    Ok(out.stdout)
}

/// Muxed MP4 (picture + sound in one file), so a plain <video> element can play it.
pub const VIDEO_FORMAT: &str = "best[ext=mp4][vcodec!=none][acodec!=none]/best[vcodec!=none][acodec!=none]";

/// What "See video" asks yt-dlp for: an exact video (id or link) or the top match for a song.
pub fn video_target(query: &str) -> String {
    let q = query.trim();
    let is_id = q.len() == 11 && q.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if is_id {
        format!("https://www.youtube.com/watch?v={q}")
    } else if q.starts_with("http") {
        q.to_string()
    } else {
        format!("ytsearch1:{q} official music video")
    }
}

fn video(query: &str) -> Result<Value> {
    let mut a = args(&["-J", "--no-warnings", "--no-playlist", "--force-ipv4", "-f", VIDEO_FORMAT]);
    a.push(video_target(query));
    let out = ytrun::run(&a, None, &mut |_, _| {})?;
    if !out.ok() {
        return Err(out.error("yt-dlp couldn't find the video"));
    }
    let v: Value = serde_json::from_str(&out.stdout).context("parsing yt-dlp output")?;
    crate::ytmeta::video_stream_from_json(&v).ok_or_else(|| anyhow!("no video found"))
}

/// "See video": a direct, playable video link for a song or YouTube video.
#[tauri::command(async)]
pub async fn youtube_video(query: String) -> Result<Value, String> {
    blocking(move || video(&query)).await.map_err(|e| format!("{e:#}"))
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tauri::command(async)]
pub fn youtube_cancel(id: String) {
    ytrun::cancel(&id);
}

#[tauri::command(async)]
pub fn youtube_set_dir(path: String) -> Result<(), String> {
    let c = core::get().ok_or("engine not initialised")?;
    c.library.set_setting(SETTING_DIR, &path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ytmeta::guess;
    use lofty::prelude::*;

    #[test]
    fn guesses_artist_and_title() {
        assert_eq!(guess("AURORA - Runaway", "AURORA"), ("AURORA".into(), "Runaway".into()));
        assert_eq!(guess("Aurora - Runaway (lyrics)", "Chilled Sad"), ("Aurora".into(), "Runaway".into()));
        assert_eq!(
            guess("Aurora - Runaway | Lirik Terjemahan And I was running far away", "Indolirik"),
            ("Aurora".into(), "Runaway".into())
        );
        assert_eq!(guess("Blinding Lights (Official Video)", "TheWeekndVEVO"), ("TheWeeknd".into(), "Blinding Lights".into()));
        assert_eq!(guess("Song (feat. X) [Official Audio]", "Band - Topic"), ("Band".into(), "Song (feat. X)".into()));
        assert_eq!(safe_name("AC/DC - Back: In Black?"), "AC_DC - Back_ In Black_");
    }

    /// Network: "Download" on a song streaming from a YouTube Music search saves that exact video.
    #[test]
    #[ignore]
    fn resolves_search_to_video_id() {
        crate::audio::engine::set_link_extractor(Box::new(super::extract_link));
        let id = crate::audio::engine::video_id_for("ytq:LANY Stuck#https://example.com/preview.m4a").unwrap();
        println!("ytq:LANY Stuck -> {id}");
        assert_eq!(id.len(), 11);
        // Second time comes from the link cache, instantly.
        let t = std::time::Instant::now();
        assert_eq!(crate::audio::engine::video_id_for("ytq:LANY Stuck").unwrap(), id);
        assert!(t.elapsed() < std::time::Duration::from_millis(50));
        assert_eq!(crate::audio::engine::video_id_for("yt:dQw4w9WgXcQ").unwrap(), "dQw4w9WgXcQ");
    }

    /// Network: "See video" finds a playable MP4 for a song search and for a video id.
    #[test]
    #[ignore]
    fn finds_video_for_see_video() {
        for q in ["LANY Stuck", "dQw4w9WgXcQ"] {
            let v = super::video(q).unwrap();
            println!("{q}: {} {} {}", v["id"], v["title"], &v["url"].as_str().unwrap_or("NONE")[..60.min(v["url"].as_str().unwrap_or("NONE").len())]);
            assert!(v["url"].as_str().is_some_and(|u| u.starts_with("https://")));
        }
    }

    /// Network: streams a Creative Commons video and a YouTube Music search into the play cache.
    #[test]
    #[ignore]
    fn fetches_streams_for_playback() {
        let dir = std::env::temp_dir().join("stafy-yt-stream");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let t = std::time::Instant::now();
        let p = fetch_stream("yt:YE7VzlLtp-4", &dir.join("a")).unwrap();
        println!("yt: {} in {:?}", p.display(), t.elapsed());
        let t = std::time::Instant::now();
        let q = fetch_stream("ytq:Gloria Jessica Terbelah Jadi Dua", &dir.join("b")).unwrap();
        println!("ytq: {} in {:?}", q.display(), t.elapsed());
        for f in [&p, &q] {
            let d = crate::audio::decoder::Decoder::open(&f.to_string_lossy(), 48000, 2).unwrap();
            println!("  decodes: {:.0}s {}", d.info.duration, d.info.codec);
            assert!(d.info.duration > 60.0);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Network: downloads a Creative Commons video (Blender's Big Buck Bunny trailer).
    #[test]
    #[ignore]
    fn downloads_and_tags_mp3() {
        let dir = std::env::temp_dir().join("stafy-yt-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let videos = lookup("https://www.youtube.com/watch?v=YE7VzlLtp-4", false).unwrap();
        println!("lookup: {} / {}", videos[0].artist, videos[0].title);
        let stages = parking_lot::Mutex::new(Vec::new());
        let emit = |stage: &'static str, pct: f64, _: Option<String>, _: Option<String>| stages.lock().push((stage, pct));
        let ffmpeg = ffmpeg().unwrap();
        let path = run(Some(&ffmpeg), &dir, &videos[0].url, "t", "Trailer", "Blender Foundation", "mp3", &emit).unwrap().unwrap();
        println!("saved {} ({} progress events)", path.display(), stages.lock().len());
        let file = lofty::read_from_path(&path).unwrap();
        let tag = file.primary_tag().unwrap();
        assert_eq!(path.extension().unwrap(), "mp3");
        assert_eq!(tag.title().as_deref(), Some("Trailer"));
        assert_eq!(tag.artist().as_deref(), Some("Blender Foundation"));
        println!("cover art embedded: {}", !tag.pictures().is_empty());
    }
}
