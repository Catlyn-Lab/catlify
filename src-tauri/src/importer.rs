//! Import playlists/libraries from Spotify, Apple Music and YouTube Music.
//!
//! Links: public Spotify and Apple Music playlists/albums are read from their public web pages (no
//! sign-in). YouTube / YouTube Music links are listed by yt-dlp in the app (exact video ids).
//! Files, for what can't be shared as a link (Liked Songs, whole libraries):
//!   Spotify data download (YourLibrary.json, Playlist*.json) · Exportify CSV
//!   Apple Music / iTunes "Export Playlist" (.txt or .xml)
//!   YouTube Music Google Takeout CSV · any CSV with title + artist columns
//! The app then finds each song on YouTube (see src/lib/importer.svelte.ts).

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportTrack {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// Seconds.
    pub duration: Option<f64>,
    /// Known YouTube video (YouTube Music playlists, Takeout), so no search is needed.
    pub video_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub name: String,
    pub source: String,
    pub tracks: Vec<ImportTrack>,
    /// The source only showed part of the list (e.g. Spotify's public page caps long playlists).
    pub truncated: bool,
}

// ---- links ----------------------------------------------------------------------------------

const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

pub async fn from_link(http: &reqwest::Client, url: &str) -> Result<Collection> {
    let url = url.trim();
    if let Some((kind, id)) = spotify_ref(url) {
        return spotify(http, kind, &id).await;
    }
    if url.contains("music.apple.com/") {
        return apple(http, url).await;
    }
    if url.contains("youtube.com/") || url.contains("youtu.be/") {
        bail!("youtube"); // handled by the app's yt-dlp (playlist listing)
    }
    bail!("Paste a Spotify, Apple Music or YouTube Music playlist link")
}

/// open.spotify.com/{playlist,album,track}/<id> (also /intl-xx/ and spotify:playlist:<id>).
fn spotify_ref(url: &str) -> Option<(&'static str, String)> {
    for kind in ["playlist", "album", "track"] {
        for prefix in [format!("/{kind}/"), format!("spotify:{kind}:")] {
            if let Some(i) = url.find(&prefix) {
                if url.contains("spotify") {
                    let id: String = url[i + prefix.len()..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                    if !id.is_empty() {
                        return Some((kind, id));
                    }
                }
            }
        }
    }
    None
}

async fn spotify(http: &reqwest::Client, kind: &str, id: &str) -> Result<Collection> {
    // The embed page carries the track list as JSON and works without signing in.
    let html = http
        .get(format!("https://open.spotify.com/embed/{kind}/{id}"))
        .header("User-Agent", UA)
        .send()
        .await?
        .error_for_status()
        .context("Spotify didn't share this playlist (is it public?)")?
        .text()
        .await?;
    let json = between(&html, r#"<script id="__NEXT_DATA__" type="application/json">"#, "</script>")
        .ok_or_else(|| anyhow!("Spotify changed its page format"))?;
    let v: Value = serde_json::from_str(json)?;
    let ent = v.pointer("/props/pageProps/state/data/entity").ok_or_else(|| anyhow!("playlist not found"))?;
    let name = ent.get("name").or_else(|| ent.get("title")).and_then(Value::as_str).unwrap_or("Spotify playlist").to_string();
    let album = (kind == "album").then(|| name.clone());
    let mut tracks: Vec<ImportTrack> = ent
        .get("trackList")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|t| {
                    Some(ImportTrack {
                        title: t.get("title")?.as_str()?.to_string(),
                        artist: first_artist(t.get("subtitle").and_then(Value::as_str).unwrap_or("")),
                        album: album.clone(),
                        duration: t.get("duration").and_then(Value::as_f64).map(|ms| ms / 1000.0),
                        video_id: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if kind == "track" && tracks.is_empty() {
        let artist = ent.get("artists").and_then(Value::as_array).and_then(|a| a.first()).and_then(|a| a.get("name")).and_then(Value::as_str);
        tracks.push(ImportTrack { title: name.clone(), artist: artist.unwrap_or("").into(), ..Default::default() });
    }
    // The public page lists at most 100 songs.
    let truncated = tracks.len() >= 100;
    Ok(Collection { name, source: "Spotify".into(), tracks, truncated })
}

async fn apple(http: &reqwest::Client, url: &str) -> Result<Collection> {
    let html = http.get(url).header("User-Agent", UA).send().await?.error_for_status()?.text().await?;
    let json = between_tag(&html, "serialized-server-data").ok_or_else(|| anyhow!("Apple Music changed its page format"))?;
    let v: Value = serde_json::from_str(json)?;
    let mut tracks = vec![];
    collect_apple(&v, &mut tracks);
    let name = between(&html, r#"<meta property="og:title" content=""#, "\"")
        .map(|t| html_unescape(t).replace(" on Apple Music", ""))
        .unwrap_or_else(|| "Apple Music playlist".into());
    if tracks.is_empty() {
        bail!("no songs found on that page (is the playlist public?)");
    }
    Ok(Collection { name, source: "Apple Music".into(), truncated: tracks.len() >= 100, tracks })
}

fn collect_apple(v: &Value, out: &mut Vec<ImportTrack>) {
    match v {
        Value::Object(m) => {
            if let (Some(Value::String(title)), Some(Value::String(artist)), Some(dur)) = (m.get("title"), m.get("artistName"), m.get("duration")) {
                let album = m
                    .get("tertiaryLinks")
                    .and_then(Value::as_array)
                    .and_then(|l| l.first())
                    .and_then(|l| l.get("title"))
                    .and_then(Value::as_str)
                    .map(|a| a.trim_end_matches(" - Single").trim_end_matches(" - EP").to_string());
                let t = ImportTrack { title: title.clone(), artist: first_artist(artist), album, duration: dur.as_f64().map(|ms| ms / 1000.0), video_id: None };
                if !out.contains(&t) {
                    out.push(t);
                }
                return;
            }
            m.values().for_each(|x| collect_apple(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| collect_apple(x, out)),
        _ => {}
    }
}

// ---- files ----------------------------------------------------------------------------------

/// Parses an export file (already decoded to text). May contain several playlists.
pub fn from_file(name: &str, text: &str) -> Result<Vec<Collection>> {
    let lower = name.to_lowercase();
    let stem = name.rsplit(['/', '\\']).next().unwrap_or(name).rsplit_once('.').map(|(s, _)| s).unwrap_or(name).to_string();
    let text = text.trim_start_matches('\u{feff}');
    let out = if lower.ends_with(".json") {
        spotify_json(&stem, text)?
    } else if lower.ends_with(".xml") || text.trim_start().starts_with("<?xml") {
        vec![itunes_xml(&stem, text)?]
    } else {
        // Apple Music's .txt export is tab-separated; everything else is CSV.
        let delim = if lower.ends_with(".txt") || text.lines().next().is_some_and(|l| l.contains('\t')) { '\t' } else { ',' };
        vec![table(&stem, text, delim)?]
    };
    let out: Vec<Collection> = out.into_iter().filter(|c| !c.tracks.is_empty()).collect();
    if out.is_empty() {
        bail!("no songs found in {name}");
    }
    Ok(out)
}

/// Spotify "Download your data": YourLibrary.json (liked songs) and Playlist1.json.
fn spotify_json(stem: &str, text: &str) -> Result<Vec<Collection>> {
    let v: Value = serde_json::from_str(text).context("not a Spotify export")?;
    let s = |o: &Value, k: &str| o.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut out = vec![];
    if let Some(tracks) = v.get("tracks").and_then(Value::as_array) {
        let tracks = tracks
            .iter()
            .map(|t| ImportTrack { title: s(t, "track"), artist: first_artist(&s(t, "artist")), album: Some(s(t, "album")).filter(|a| !a.is_empty()), ..Default::default() })
            .filter(|t| !t.title.is_empty())
            .collect();
        out.push(Collection { name: "Spotify Liked Songs".into(), source: "Spotify".into(), tracks, truncated: false });
    }
    if let Some(pls) = v.get("playlists").and_then(Value::as_array) {
        for p in pls {
            let tracks = p
                .get("items")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| i.get("track").filter(|t| !t.is_null()))
                        .map(|t| ImportTrack {
                            title: s(t, "trackName"),
                            artist: first_artist(&s(t, "artistName")),
                            album: Some(s(t, "albumName")).filter(|a| !a.is_empty()),
                            ..Default::default()
                        })
                        .filter(|t| !t.title.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            out.push(Collection { name: s(p, "name"), source: "Spotify".into(), tracks, truncated: false });
        }
    }
    if out.is_empty() {
        bail!("{stem}: not a Spotify library or playlist export");
    }
    Ok(out)
}

/// iTunes / Apple Music XML (plist): every track dict with Name + Artist.
fn itunes_xml(stem: &str, text: &str) -> Result<Collection> {
    let mut tracks = vec![];
    let mut cur: Vec<(String, String)> = vec![];
    // Tiny plist reader: <key>K</key><string|integer>V</...> pairs; a <dict> closes a track.
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        rest = &rest[i..];
        if rest.starts_with("<key>") {
            let Some(k) = between(rest, "<key>", "</key>") else { break };
            rest = &rest[5 + k.len() + 6..];
            let val = rest.trim_start();
            let v = ["string", "integer"].iter().find_map(|t| {
                val.starts_with(&format!("<{t}>")).then(|| between(val, &format!("<{t}>"), &format!("</{t}>")).unwrap_or(""))
            });
            if let Some(v) = v {
                cur.push((html_unescape(k), html_unescape(v)));
            }
        } else if rest.starts_with("</dict>") {
            let get = |k: &str| cur.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone());
            if let (Some(title), Some(artist)) = (get("Name"), get("Artist")) {
                tracks.push(ImportTrack {
                    title,
                    artist: first_artist(&artist),
                    album: get("Album"),
                    duration: get("Total Time").and_then(|t| t.parse::<f64>().ok()).map(|ms| ms / 1000.0),
                    video_id: None,
                });
            }
            cur.clear();
            rest = &rest[7..];
        } else {
            rest = &rest[1..];
        }
    }
    Ok(Collection { name: stem.to_string(), source: "Apple Music".into(), tracks, truncated: false })
}

/// CSV/TSV with a header row: Exportify, Apple Music .txt, Google Takeout, or anything with
/// title + artist columns.
fn table(stem: &str, text: &str, delim: char) -> Result<Collection> {
    let rows = parse_csv(text, delim);
    let Some(header) = rows.first() else { bail!("empty file") };
    let h: Vec<String> = header.iter().map(|c| c.trim().to_lowercase()).collect();
    let col = |names: &[&str]| h.iter().position(|c| names.iter().any(|n| c == n));
    let title = col(&["track name", "song title", "title", "name", "track", "song"]);
    let artist = col(&["artist name(s)", "artist names", "artist name 1", "artist", "artists", "artist name"]);
    let album = col(&["album name", "album title", "album"]);
    let dur_ms = col(&["duration (ms)", "track duration (ms)"]);
    let dur_s = col(&["time", "duration"]);
    let video = col(&["video id", "videoid"]);
    if title.is_none() && video.is_none() {
        bail!("{stem}: couldn't find a title column");
    }
    let source = if h.iter().any(|c| c == "spotify id" || c == "track uri") {
        "Spotify"
    } else if video.is_some() || h.iter().any(|c| c.starts_with("artist name 1")) {
        "YouTube Music"
    } else if delim == '\t' {
        "Apple Music"
    } else {
        "CSV"
    };
    let cell = |r: &Vec<String>, i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let tracks = rows[1..]
        .iter()
        .filter_map(|r| {
            let t = ImportTrack {
                title: cell(r, title).unwrap_or_default(),
                artist: first_artist(&cell(r, artist).unwrap_or_default()),
                album: cell(r, album),
                duration: cell(r, dur_ms).and_then(|d| d.parse::<f64>().ok()).map(|ms| ms / 1000.0).or_else(|| cell(r, dur_s).and_then(|d| parse_time(&d))),
                video_id: cell(r, video).filter(|v| v.len() == 11),
            };
            (!t.title.is_empty() || t.video_id.is_some()).then_some(t)
        })
        .collect();
    Ok(Collection { name: stem.to_string(), source: source.into(), tracks, truncated: false })
}

fn parse_time(s: &str) -> Option<f64> {
    let parts: Vec<f64> = s.split(':').map(|p| p.trim().parse::<f64>().ok()).collect::<Option<_>>()?;
    Some(parts.iter().fold(0.0, |acc, p| acc * 60.0 + p))
}

/// RFC 4180-ish: quoted fields, doubled quotes, newlines inside quotes.
fn parse_csv(text: &str, delim: char) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut field, mut quoted) = (vec![], vec![], String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == delim && !quoted => row.push(std::mem::take(&mut field)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                }
                row.clear();
            }
            c => field.push(c),
        }
    }
    row.push(field);
    if row.iter().any(|f| !f.is_empty()) {
        rows.push(row);
    }
    rows
}

// ---- helpers --------------------------------------------------------------------------------

/// "Artist A, Artist B" / "A & B" / "A; B" → "Artist A" (searches match the lead artist best).
fn first_artist(s: &str) -> String {
    let s = s.trim();
    for sep in [", ", ";", " & ", " feat. ", " ft. ", " x "] {
        if let Some((a, _)) = s.split_once(sep) {
            if !a.trim().is_empty() {
                return a.trim().to_string();
            }
        }
    }
    s.to_string()
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(&s[i..j])
}

fn between_tag<'a>(html: &'a str, id: &str) -> Option<&'a str> {
    let i = html.find(&format!(r#"id="{id}""#))?;
    let start = html[i..].find('>')? + i + 1;
    let end = html[start..].find("</script>")? + start;
    Some(&html[start..end])
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_export_files() {
        let exportify = "\"Track URI\",\"Track Name\",\"Artist Name(s)\",\"Album Name\",\"Track Duration (ms)\"\n\
            \"spotify:track:1\",\"Runaway\",\"AURORA\",\"All My Demons Greeting Me as a Friend\",\"248000\"\n\
            \"spotify:track:2\",\"Dai Dai\",\"Shakira, Burna Boy\",\"Dai Dai\",\"222000\"\n";
        let c = &from_file("Road Trip.csv", exportify).unwrap()[0];
        assert_eq!((c.name.as_str(), c.source.as_str(), c.tracks.len()), ("Road Trip", "Spotify", 2));
        assert_eq!(c.tracks[1].artist, "Shakira");
        assert_eq!(c.tracks[0].duration, Some(248.0));

        let apple_txt = "Name\tArtist\tComposer\tAlbum\tGrouping\tTime\nSesi Potret\teñau\t\tSesi Potret\t\t243\n";
        let c = &from_file("Favourites.txt", apple_txt).unwrap()[0];
        assert_eq!((c.source.as_str(), c.tracks[0].artist.as_str(), c.tracks[0].duration), ("Apple Music", "eñau", Some(243.0)));

        let takeout = "Video ID,Song Title,Album Title,Artist Name 1\nfcnDmrtj6Sk,Dai Dai,Dai Dai,Shakira\n";
        let c = &from_file("music library songs.csv", takeout).unwrap()[0];
        assert_eq!((c.source.as_str(), c.tracks[0].video_id.as_deref()), ("YouTube Music", Some("fcnDmrtj6Sk")));

        let spotify = r#"{"tracks":[{"artist":"Tame Impala","album":"Deadbeat","track":"Loser","uri":"spotify:track:x"}],
            "playlists":[{"name":"Gym","items":[{"track":{"trackName":"Housewife","artistName":"Tate McRae","albumName":"So Close"}}]}]}"#;
        let cs = from_file("YourLibrary.json", spotify).unwrap();
        assert_eq!(cs.iter().map(|c| (c.name.as_str(), c.tracks.len())).collect::<Vec<_>>(), [("Spotify Liked Songs", 1), ("Gym", 1)]);

        let xml = "<?xml version=\"1.0\"?><plist><dict><key>Tracks</key><dict><key>1</key><dict>\
            <key>Name</key><string>Terbelah Jadi Dua</string><key>Artist</key><string>Gloria Jessica</string>\
            <key>Total Time</key><integer>255000</integer></dict></dict></dict></plist>";
        let c = &from_file("Indo.xml", xml).unwrap()[0];
        assert_eq!((c.tracks[0].title.as_str(), c.tracks[0].duration), ("Terbelah Jadi Dua", Some(255.0)));

        assert_eq!(spotify_ref("https://open.spotify.com/intl-id/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc"), Some(("playlist", "37i9dQZF1DXcBWIGoYBM5M".into())));
    }

    /// Network: public Spotify and Apple Music playlists load without signing in.
    #[tokio::test]
    #[ignore]
    async fn loads_public_playlists() {
        let http = reqwest::Client::new();
        for url in [
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            "https://music.apple.com/us/playlist/todays-hits/pl.f4d106fed2bd41149aaacabb233eb5eb",
        ] {
            let c = from_link(&http, url).await.unwrap();
            println!("{} ({}): {} songs, first: {} - {}", c.name, c.source, c.tracks.len(), c.tracks[0].artist, c.tracks[0].title);
            assert!(c.tracks.len() >= 20);
        }
    }
}
