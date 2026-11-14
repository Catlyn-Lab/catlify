//! YouTube metadata helpers shared by desktop (youtube.rs, yt-dlp CLI) and Android
//! (StafyYouTube.kt, bundled yt-dlp): song title/artist guessing and retagging downloads.

use anyhow::Result;
use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::Tag;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    pub id: String,
    pub url: String,
    /// Song title and artist guessed from the video (editable before download).
    pub title: String,
    pub artist: String,
    pub video_title: String,
    pub channel: String,
    pub duration: Option<f64>,
    pub thumbnail: String,
}

// ---- title parsing ---------------------------------------------------------------------

const NOISE: &[&str] = &[
    "official", "video", "audio", "lyric", "lyrics", "visualizer", "visualiser", "mv", "m/v", "hd", "4k", "hq", "music video",
    "performance video", "color coded", "eng sub", "sub indo", "terjemahan", "lirik",
];

/// Drops "(Official Video)", "[Lyrics]", "| Lirik Terjemahan" and similar from a video title.
fn clean(title: &str) -> String {
    let mut out = String::new();
    let mut rest = title;
    while let Some(start) = rest.find(['(', '[']) {
        let close = if rest.as_bytes()[start] == b'(' { ')' } else { ']' };
        let Some(len) = rest[start..].find(close) else { break };
        let inner = rest[start + 1..start + len].to_lowercase();
        out.push_str(&rest[..start]);
        if !NOISE.iter().any(|n| inner.contains(n)) {
            out.push_str(&rest[start..=start + len]);
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    // "Song | Lyrics", "Song // Official Audio"
    for sep in [" | ", " // "] {
        if let Some(i) = out.find(sep) {
            if NOISE.iter().any(|n| out[i..].to_lowercase().contains(n)) {
                out.truncate(i);
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_channel(channel: &str) -> String {
    let c = channel.trim_end_matches(" - Topic").trim_end_matches("VEVO").trim_end_matches("Official").trim();
    c.to_string()
}

/// Splits "Artist - Title" (also en/em dashes); otherwise the channel is the artist.
pub(crate) fn guess(video_title: &str, channel: &str) -> (String, String) {
    let t = clean(video_title);
    for sep in [" - ", " – ", " — ", " ~ "] {
        if let Some((a, b)) = t.split_once(sep) {
            if !a.trim().is_empty() && !b.trim().is_empty() {
                return (a.trim().to_string(), b.trim().trim_matches('"').to_string());
            }
        }
    }
    (clean_channel(channel), t.trim_matches('"').to_string())
}

pub fn video_from(v: &Value) -> Option<Video> {
    let id = v.get("id")?.as_str()?.to_string();
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let video_title = s("title")?;
    let channel = s("channel").or_else(|| s("uploader")).unwrap_or_default();
    // Music uploads carry real tags; prefer them over guessing.
    let (artist, title) = match (s("artist").or_else(|| s("creator")), s("track")) {
        (Some(a), Some(t)) => (a.split(',').next().unwrap_or(&a).trim().to_string(), t),
        _ => guess(&video_title, &channel),
    };
    Some(Video {
        url: s("webpage_url").or_else(|| s("url")).unwrap_or_else(|| format!("https://www.youtube.com/watch?v={id}")),
        thumbnail: format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"),
        duration: v.get("duration").and_then(Value::as_f64),
        id,
        title,
        artist,
        video_title,
        channel,
    })
}

/// Search results (`entries`) or a single video, from yt-dlp `-J` output.
pub fn videos_from_json(v: &Value) -> Vec<Video> {
    match v.get("entries").and_then(Value::as_array) {
        Some(entries) => entries.iter().filter_map(video_from).collect(),
        None => video_from(v).into_iter().collect(),
    }
}

pub fn retag(path: &Path, title: &str, artist: &str) -> Result<()> {
    let mut file = lofty::read_from_path(path)?;
    if file.primary_tag().is_none() {
        let kind = file.primary_tag_type();
        file.insert_tag(Tag::new(kind));
    }
    let tag = file.primary_tag_mut().expect("tag inserted above");
    tag.set_title(title.to_string());
    if !artist.trim().is_empty() {
        tag.set_artist(artist.to_string());
    }
    // yt-dlp puts the channel/description in these; they'd group singles into odd "albums".
    tag.remove_key(ItemKey::AlbumArtist);
    tag.remove_key(ItemKey::Comment);
    tag.remove_key(ItemKey::Description);
    if tag.album().is_none() {
        tag.set_album(title.to_string());
    }
    file.save_to_path(path, WriteOptions::default())?;
    Ok(())
}


/// The playable video from yt-dlp's `-J` output (a video, or a one-result search), for the
/// "See video" player: `{ id, title, channel, url, thumbnail }`. `url` is a direct MP4 link
/// with sound (it only works from this device's address for a few hours).
pub fn video_stream_from_json(v: &Value) -> Option<Value> {
    let v = v.get("entries").and_then(|e| e.as_array()).and_then(|e| e.first()).unwrap_or(v);
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let id = s("id")?;
    // A single selected format is flattened into the top level; otherwise take the best
    // listed format that has both picture and sound.
    let url = s("url").filter(|_| s("vcodec").as_deref() != Some("none") && s("acodec").as_deref() != Some("none")).or_else(|| {
        v.get("formats")?
            .as_array()?
            .iter()
            .filter(|f| {
                let has = |k: &str| f.get(k).and_then(|x| x.as_str()).is_some_and(|c| c != "none");
                has("vcodec") && has("acodec") && f.get("protocol").and_then(|p| p.as_str()).is_some_and(|p| p.starts_with("http"))
            })
            .max_by_key(|f| f.get("height").and_then(|h| h.as_u64()).unwrap_or(0))
            .and_then(|f| f.get("url")?.as_str().map(str::to_string))
    });
    Some(serde_json::json!({
        "id": id,
        "title": s("title"),
        "channel": s("channel").or_else(|| s("uploader")),
        "url": url,
        "thumbnail": s("thumbnail"),
    }))
}
