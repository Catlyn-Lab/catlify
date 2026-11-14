//! Lyrics lookup: sidecar .lrc file → embedded tags → LRCLIB (synced, free, no API key).

use crate::audio::QueueItem;
use anyhow::Result;
use lofty::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
    pub source: String,
}

impl Lyrics {
    pub fn is_empty(&self) -> bool {
        self.synced.is_none() && self.plain.is_none()
    }

    fn from_text(text: String, source: &str) -> Option<Self> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return None;
        }
        let synced = text.lines().any(is_timed_line);
        Some(if synced {
            Lyrics { synced: Some(text), plain: None, source: source.into() }
        } else {
            Lyrics { synced: None, plain: Some(text), source: source.into() }
        })
    }
}

fn is_timed_line(line: &str) -> bool {
    let b = line.trim_start().as_bytes();
    b.len() > 6 && b[0] == b'[' && b[1].is_ascii_digit() && line.contains(':') && line.contains(']')
}

pub fn local(item: &QueueItem) -> Option<Lyrics> {
    if item.path.starts_with("http") {
        return None;
    }
    let path = Path::new(&item.path);
    for ext in ["lrc", "LRC", "txt"] {
        if let Ok(text) = std::fs::read_to_string(path.with_extension(ext)) {
            if let Some(l) = Lyrics::from_text(text, "file") {
                return Some(l);
            }
        }
    }
    let file = lofty::read_from_path(path).ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    let text = tag.get_string(ItemKey::Lyrics).or_else(|| tag.get_string(ItemKey::UnsyncLyrics))?;
    Lyrics::from_text(text.to_string(), "embedded")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibRecord {
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

impl LrclibRecord {
    fn into_lyrics(self) -> Lyrics {
        if self.instrumental {
            return Lyrics { synced: None, plain: Some("♪ Instrumental ♪".into()), source: "lrclib".into() };
        }
        let clean = |s: Option<String>| s.filter(|s| !s.trim().is_empty());
        Lyrics { synced: clean(self.synced_lyrics), plain: clean(self.plain_lyrics), source: "lrclib".into() }
    }
}

/// Strips "(feat. X)", "- Remastered 2011", "[Live]" etc. to improve matching.
fn clean_title(title: &str) -> String {
    let mut t = title.to_string();
    for sep in [" (", " [", " - "] {
        if let Some(i) = t.find(sep) {
            let tail = t[i..].to_lowercase();
            let noisy = ["feat", "ft.", "remaster", "live", "version", "edit", "mix", "mono", "stereo", "deluxe", "bonus"];
            if noisy.iter().any(|n| tail.contains(n)) {
                t.truncate(i);
            }
        }
    }
    t.trim().to_string()
}

pub async fn online(client: &reqwest::Client, item: &QueueItem) -> Result<Option<Lyrics>> {
    let artist = item.artist.split([',', '&']).next().unwrap_or(&item.artist).trim().to_string();
    let mut query: Vec<(&str, String)> = vec![("track_name", item.title.clone()), ("artist_name", artist.clone())];
    if !item.album.is_empty() && item.album != "Unknown Album" {
        query.push(("album_name", item.album.clone()));
    }
    if item.duration > 0.0 {
        query.push(("duration", format!("{}", item.duration.round() as i64)));
    }
    let res = client.get("https://lrclib.net/api/get").query(&query).send().await?;
    if res.status().is_success() {
        let l = res.json::<LrclibRecord>().await?.into_lyrics();
        if !l.is_empty() {
            return Ok(Some(l));
        }
    }

    let records: Vec<LrclibRecord> = client
        .get("https://lrclib.net/api/search")
        .query(&[("track_name", clean_title(&item.title)), ("artist_name", artist)])
        .send()
        .await?
        .json()
        .await?;
    let best = records
        .iter()
        .position(|r| r.synced_lyrics.is_some())
        .or(if records.is_empty() { None } else { Some(0) });
    Ok(best.map(|i| records.into_iter().nth(i).unwrap().into_lyrics()).filter(|l| !l.is_empty()))
}

/// Cache → sidecar/embedded → LRCLIB, caching the answer for library songs. Shared by the UI
/// command and the car screens.
pub async fn get(library: &crate::library::Library, http: &reqwest::Client, item: &QueueItem, force: bool) -> Result<Lyrics> {
    if let (Some(id), false) = (item.id, force) {
        if let Ok(Some(cached)) = library.cached_lyrics(id) {
            return Ok(cached);
        }
    }
    let local_item = item.clone();
    let mut found = tokio::task::spawn_blocking(move || local(&local_item)).await?;
    if found.is_none() {
        found = online(http, item).await?;
    }
    let result = found.unwrap_or(Lyrics { synced: None, plain: None, source: "none".into() });
    if let Some(id) = item.id {
        let _ = library.store_lyrics(id, &result);
    }
    Ok(result)
}

/// One line per row for screens that can only show lists (CarPlay, Android Auto).
/// Synced lyrics carry the second the line starts; plain lyrics don't.
pub fn lines(l: &Lyrics) -> Vec<(Option<f64>, String)> {
    if let Some(synced) = &l.synced {
        let mut out: Vec<(Option<f64>, String)> = Vec::new();
        for raw in synced.lines() {
            // "[01:02.34][02:10.00]text": one entry per timestamp.
            let mut rest = raw.trim();
            let mut times = Vec::new();
            while let Some(stripped) = rest.strip_prefix('[') {
                let Some(end) = stripped.find(']') else { break };
                let tag = &stripped[..end];
                rest = stripped[end + 1..].trim_start();
                if let Some((m, s)) = tag.split_once(':') {
                    if let (Ok(m), Ok(s)) = (m.trim().parse::<f64>(), s.trim().parse::<f64>()) {
                        times.push(m * 60.0 + s);
                    }
                }
            }
            let text = rest.trim();
            for t in times {
                out.push((Some(t), if text.is_empty() { "♪".into() } else { text.to_string() }));
            }
        }
        out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        if !out.is_empty() {
            return out;
        }
    }
    l.plain
        .as_deref()
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| (None, t.to_string()))
        .collect()
}

#[cfg(test)]
mod line_tests {
    #[test]
    fn splits_synced_lines() {
        let l = super::Lyrics {
            synced: Some("[ar:x]\n[00:12.50]Hello\n[00:05.00][01:00.00]Chorus\n[00:20.00]".into()),
            plain: None,
            source: "t".into(),
        };
        let lines = super::lines(&l);
        assert_eq!(lines.iter().map(|(t, s)| (t.unwrap(), s.as_str())).collect::<Vec<_>>(),
            vec![(5.0, "Chorus"), (12.5, "Hello"), (20.0, "♪"), (60.0, "Chorus")]);
    }
}
