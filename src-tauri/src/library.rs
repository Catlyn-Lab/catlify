//! Local music library: folder scanning, tag reading and SQLite storage.

use anyhow::Result;
use lofty::prelude::*;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "m4b", "mp4", "aac", "flac", "alac", "wav", "wave", "w64", "aif", "aiff", "aifc", "ogg",
    "oga", "opus", "ape", "wv", "wma", "dsf", "dff", "tta", "mpc", "tak", "mka", "caf", "spx",
];

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub duration: f64,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub channels: Option<u32>,
    pub codec: String,
    pub lossless: bool,
    pub cover: Option<String>,
    pub added_at: i64,
    pub play_count: i64,
    pub last_played: Option<i64>,
    pub liked: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub deleted: Vec<i64>,
    /// Android: files only the system can delete (it asks the user).
    pub needs_permission: Vec<DeleteBlocked>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteBlocked {
    pub id: i64,
    pub path: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub track_count: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub scanned: usize,
    pub total: usize,
    pub added: usize,
    pub done: bool,
    pub current: Option<String>,
}

pub struct Library {
    db: Mutex<Connection>,
    covers_dir: PathBuf,
    scanning: std::sync::atomic::AtomicBool,
    rescan_requested: std::sync::atomic::AtomicBool,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn hash_str(s: &str) -> String {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

const TRACK_COLUMNS: &str = "id, path, title, artist, album, album_artist, track_no, disc_no, year, genre, \
    duration, sample_rate, bit_depth, channels, codec, lossless, cover, added_at, play_count, last_played, liked";

fn row_to_track(r: &rusqlite::Row) -> rusqlite::Result<Track> {
    Ok(Track {
        id: r.get(0)?,
        path: r.get(1)?,
        title: r.get(2)?,
        artist: r.get(3)?,
        album: r.get(4)?,
        album_artist: r.get(5)?,
        track_no: r.get(6)?,
        disc_no: r.get(7)?,
        year: r.get(8)?,
        genre: r.get(9)?,
        duration: r.get(10)?,
        sample_rate: r.get(11)?,
        bit_depth: r.get(12)?,
        channels: r.get(13)?,
        codec: r.get(14)?,
        lossless: r.get(15)?,
        cover: r.get(16)?,
        added_at: r.get(17)?,
        play_count: r.get(18)?,
        last_played: r.get(19)?,
        liked: r.get(20)?,
    })
}

impl Library {
    pub fn open(data_dir: &Path, cache_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let covers_dir = cache_dir.join("covers");
        std::fs::create_dir_all(&covers_dir)?;
        let conn = Connection::open(data_dir.join("stafy.db"))?;
        migrate(&conn)?;
        Ok(Self { db: Mutex::new(conn), covers_dir, scanning: Default::default(), rescan_requested: Default::default() })
    }

    /// iOS gives the app a new container path on every update (…/Application/<UUID>/), and
    /// macOS home folders can be renamed. Rewrite stored absolute paths so the library survives.
    pub fn follow_moved_home(&self) -> Result<()> {
        let Some(home) = std::env::var_os("HOME").map(|h| h.to_string_lossy().trim_end_matches('/').to_string()) else {
            return Ok(());
        };
        self.adopt_home(&home)
    }

    pub(crate) fn adopt_home(&self, home: &str) -> Result<()> {
        if home.is_empty() || home == "/" {
            return Ok(());
        }
        match self.setting("home") {
            Some(old) if old != home => {
                log::info!("home moved {old} -> {home}; rebasing library paths");
                self.rebase_paths(&old, home)?;
            }
            _ => {}
        }
        self.set_setting("home", home)
    }

    /// Replaces the `old` path prefix with `new` everywhere a path is stored.
    pub fn rebase_paths(&self, old: &str, new: &str) -> Result<()> {
        let db = self.db.lock();
        let tx = db.unchecked_transaction()?;
        let like = format!("{}/%", old.replace('%', "\\%").replace('_', "\\_"));
        let n = old.chars().count() as i64 + 1;
        for (table, col) in [("folders", "path"), ("tracks", "path"), ("tracks", "folder"), ("tracks", "cover")] {
            // Paths that would collide (already re-added under the new home) are dropped, not duplicated.
            tx.execute(
                &format!(
                    "UPDATE OR IGNORE {table} SET {col} = ?1 || substr({col}, ?2) WHERE {col} = ?3 OR {col} LIKE ?4 ESCAPE '\\'"
                ),
                params![new, n, old, like],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// Schema migrations. Never edit a shipped step; append a new one so existing libraries upgrade in place.
const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    "CREATE TABLE IF NOT EXISTS folders (path TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS tracks (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                folder TEXT NOT NULL,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT NOT NULL,
                album_artist TEXT NOT NULL,
                track_no INTEGER, disc_no INTEGER, year INTEGER, genre TEXT,
                duration REAL NOT NULL DEFAULT 0,
                sample_rate INTEGER, bit_depth INTEGER, channels INTEGER,
                codec TEXT NOT NULL DEFAULT '',
                lossless INTEGER NOT NULL DEFAULT 0,
                cover TEXT,
                mtime INTEGER NOT NULL DEFAULT 0,
                added_at INTEGER NOT NULL,
                play_count INTEGER NOT NULL DEFAULT 0,
                last_played INTEGER,
                liked INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS tracks_album ON tracks(album_artist, album);
             CREATE TABLE IF NOT EXISTS playlists (
                id INTEGER PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS playlist_tracks (
                playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                position INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS lyrics (
                track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
                synced TEXT, plain TEXT, source TEXT NOT NULL, fetched_at INTEGER NOT NULL
             );",
    // 2: Wi-Fi sync (sync.rs): content fingerprints to match songs across devices, and paired devices.
    "ALTER TABLE tracks ADD COLUMN fp TEXT;
     CREATE INDEX IF NOT EXISTS tracks_fp ON tracks(fp);
     CREATE TABLE IF NOT EXISTS peers (
        id TEXT PRIMARY KEY, name TEXT NOT NULL, secret TEXT NOT NULL,
        addr TEXT, last_sync INTEGER, paired_at INTEGER NOT NULL
     );",
    // 3: songs that vanish from a scan are hidden, not deleted, so a revoked permission or unplugged
    //    drive never loses likes, play counts or playlist entries. Purged after 30 days.
    "ALTER TABLE tracks ADD COLUMN missing_since INTEGER;",
    // 4: songs the user deleted from the device (by fingerprint), so Wi-Fi sync doesn't copy them back.
    "CREATE TABLE IF NOT EXISTS deleted (fp TEXT PRIMARY KEY, deleted_at INTEGER NOT NULL);",
];

/// Songs not seen for this long are finally removed from the library.
const MISSING_PURGE_SECS: i64 = 30 * 24 * 3600;

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    let version: usize = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
    if version > MIGRATIONS.len() {
        // Opened by a newer Catlify (e.g. after a downgrade): leave it alone rather than damage it.
        log::warn!("library schema v{version} is newer than this build (v{})", MIGRATIONS.len());
        return Ok(());
    }
    for (i, step) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(step)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

impl Library {
    // ---- folders & scanning -----------------------------------------------------------

    pub fn folders(&self) -> Result<Vec<String>> {
        let db = self.db.lock();
        let mut stmt = db.prepare("SELECT path FROM folders ORDER BY path")?;
        let rows = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn add_folder(&self, path: &str) -> Result<()> {
        self.db.lock().execute("INSERT OR IGNORE INTO folders(path) VALUES (?1)", [path])?;
        Ok(())
    }

    pub fn remove_folder(&self, path: &str) -> Result<()> {
        let db = self.db.lock();
        db.execute("DELETE FROM folders WHERE path = ?1", [path])?;
        db.execute("DELETE FROM tracks WHERE folder = ?1", [path])?;
        Ok(())
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Incremental scan: only files whose mtime changed are re-read.
    pub fn scan(&self, progress: impl Fn(ScanProgress)) -> Result<()> {
        use std::sync::atomic::Ordering;
        if self.scanning.swap(true, Ordering::SeqCst) {
            // A scan is running: make it go round once more so new files (e.g. a download) aren't missed.
            self.rescan_requested.store(true, Ordering::SeqCst);
            return Ok(());
        }
        let result = loop {
            self.rescan_requested.store(false, Ordering::SeqCst);
            let r = self.scan_inner(&progress);
            if r.is_err() || !self.rescan_requested.load(Ordering::SeqCst) {
                break r;
            }
        };
        self.scanning.store(false, Ordering::SeqCst);
        result
    }

    fn scan_inner(&self, progress: &impl Fn(ScanProgress)) -> Result<()> {
        let folders = self.folders()?;
        let mut files: Vec<(String, PathBuf, i64)> = Vec::new();
        // Folders we couldn't read (unplugged drive, revoked permission, moved app container):
        // their songs are kept, not forgotten, so likes/plays/playlists survive.
        let mut unavailable: Vec<String> = Vec::new();
        for folder in &folders {
            if std::fs::read_dir(folder).is_err() {
                log::warn!("library folder unavailable, keeping its tracks: {folder}");
                unavailable.push(folder.clone());
                continue;
            }
            let before = files.len();
            for entry in WalkDir::new(folder).follow_links(true).into_iter().filter_map(|e| e.ok()) {
                if !entry.file_type().is_file() {
                    continue;
                }
                let ext = entry.path().extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
                if !ext.is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.as_str())) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy();
                if name.starts_with("._") || name.contains(crate::sync::PARTIAL) {
                    continue; // macOS resource forks, songs still being copied by Wi-Fi sync
                }
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                files.push((folder.clone(), entry.into_path(), mtime));
            }
            if files.len() == before {
                // Readable but empty: on Android that's what a missing media permission looks like.
                unavailable.push(folder.clone());
            }
        }

        let known: std::collections::HashMap<String, (i64, String, bool, bool)> = {
            let db = self.db.lock();
            let mut stmt = db.prepare("SELECT path, mtime, folder, cover, missing_since IS NOT NULL FROM tracks")?;
            let rows = stmt
                .query_map([], |r| {
                    let cover: Option<String> = r.get(3)?;
                    // Cover cache can be purged by the OS (iOS Caches); re-extract when missing.
                    let cover_ok = cover.is_none_or(|c| c.starts_with("http") || Path::new(&c).exists());
                    Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)?, r.get::<_, String>(2)?, cover_ok, r.get::<_, bool>(4)?)))
                })?
                .collect::<rusqlite::Result<_>>()?;
            rows
        };

        let total = files.len();
        let mut added = 0;
        let mut seen = HashSet::with_capacity(total);
        for (i, (folder, path, mtime)) in files.iter().enumerate() {
            let path_str = path.to_string_lossy().to_string();
            seen.insert(path_str.clone());
            if !known.get(&path_str).is_some_and(|(m, _, cover_ok, _)| m == mtime && *cover_ok) {
                if let Some(track) = self.read_track(path) {
                    self.upsert(folder, &track, *mtime)?;
                    added += 1;
                }
            }
            if i % 25 == 0 {
                progress(ScanProgress { scanned: i, total, added, done: false, current: Some(path_str) });
            }
        }

        // Songs that disappeared are hidden, not deleted (they may just be unreadable right now);
        // ones that came back are shown again with their likes and playlists intact.
        {
            let db = self.db.lock();
            let tx = db.unchecked_transaction()?;
            let ts = now();
            for (path, (_, folder, _, missing)) in &known {
                if seen.contains(path) {
                    if *missing {
                        tx.execute("UPDATE tracks SET missing_since = NULL WHERE path = ?1", [path])?;
                    }
                } else if !*missing && !unavailable.contains(folder) {
                    tx.execute("UPDATE tracks SET missing_since = ?2 WHERE path = ?1", params![path, ts])?;
                }
            }
            tx.execute("DELETE FROM tracks WHERE missing_since < ?1", [ts - MISSING_PURGE_SECS])?;
            tx.commit()?;
        }
        progress(ScanProgress { scanned: total, total, added, done: true, current: None });
        Ok(())
    }

    fn upsert(&self, folder: &str, t: &Track, mtime: i64) -> Result<()> {
        self.db.lock().execute(
            "INSERT INTO tracks (path, folder, title, artist, album, album_artist, track_no, disc_no, year, genre,
                duration, sample_rate, bit_depth, channels, codec, lossless, cover, mtime, added_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
             ON CONFLICT(path) DO UPDATE SET folder=excluded.folder, title=excluded.title, artist=excluded.artist,
                album=excluded.album, album_artist=excluded.album_artist, track_no=excluded.track_no,
                disc_no=excluded.disc_no, year=excluded.year, genre=excluded.genre, duration=excluded.duration,
                sample_rate=excluded.sample_rate, bit_depth=excluded.bit_depth, channels=excluded.channels,
                codec=excluded.codec, lossless=excluded.lossless, cover=excluded.cover, mtime=excluded.mtime, fp=NULL, missing_since=NULL",
            params![
                t.path, folder, t.title, t.artist, t.album, t.album_artist, t.track_no, t.disc_no, t.year, t.genre,
                t.duration, t.sample_rate, t.bit_depth, t.channels, t.codec, t.lossless, t.cover, mtime, now()
            ],
        )?;
        Ok(())
    }

    /// Reads tags with lofty; falls back to FFmpeg for formats lofty doesn't know (DSD, WMA, TAK, ...).
    pub fn read_track(&self, path: &Path) -> Option<Track> {
        let fallback_title = path.file_stem()?.to_string_lossy().to_string();
        let mut t = Track { path: path.to_string_lossy().to_string(), title: fallback_title, ..Default::default() };

        match lofty::read_from_path(path) {
            Ok(file) => {
                let props = file.properties();
                t.duration = props.duration().as_secs_f64();
                t.sample_rate = props.sample_rate();
                t.bit_depth = props.bit_depth().map(u32::from);
                t.channels = props.channels().map(u32::from);
                let ft = file.file_type();
                t.codec = codec_label(ft, t.bit_depth.is_some(), path);
                t.lossless = is_lossless_type(ft, t.bit_depth.is_some(), path);
                if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
                    if let Some(v) = tag.title().filter(|s| !s.trim().is_empty()) {
                        t.title = v.to_string();
                    }
                    t.artist = tag.artist().map(|s| s.to_string()).unwrap_or_default();
                    t.album = tag.album().map(|s| s.to_string()).unwrap_or_default();
                    t.album_artist = tag.get_string(ItemKey::AlbumArtist).unwrap_or_default().to_string();
                    t.genre = tag.genre().map(|s| s.to_string());
                    t.track_no = tag.track();
                    t.disc_no = tag.disk();
                    t.year = tag.date().map(|d| d.year as u32);
                    if let Some(pic) = tag.pictures().first() {
                        t.cover = self.save_cover(&t, pic.data());
                    }
                }
            }
            Err(_) => {
                if !self.probe_with_ffmpeg(path, &mut t) {
                    return None;
                }
            }
        }

        if t.artist.is_empty() {
            t.artist = "Unknown Artist".into();
        }
        if t.album.is_empty() {
            t.album = "Unknown Album".into();
        }
        if t.album_artist.is_empty() {
            t.album_artist = t.artist.clone();
        }
        if t.cover.is_none() {
            t.cover = self.folder_cover(path);
        }
        Some(t)
    }

    fn probe_with_ffmpeg(&self, path: &Path, t: &mut Track) -> bool {
        use ffmpeg_next as ffmpeg;
        let Ok(input) = ffmpeg::format::input(&path) else { return false };
        let Some(stream) = input.streams().best(ffmpeg::media::Type::Audio) else { return false };
        if input.duration() > 0 {
            t.duration = input.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64;
        }
        let params = stream.parameters();
        let codec = params.id().name().to_string();
        unsafe {
            let p = params.as_ptr();
            t.sample_rate = Some((*p).sample_rate as u32).filter(|r| *r > 0);
            t.channels = Some((*p).ch_layout.nb_channels as u32).filter(|c| *c > 0);
            t.bit_depth = Some((*p).bits_per_raw_sample as u32).filter(|b| *b > 0);
        }
        if codec.starts_with("dsd_") {
            t.bit_depth = Some(1);
            // FFmpeg reports DSD rate / 8; show the real DSD rate (e.g. 2.8224 MHz for DSD64).
            t.sample_rate = t.sample_rate.map(|r| r * 8);
        }
        t.lossless = crate::audio::decoder::is_lossless_codec(&codec);
        t.codec = pretty_codec(&codec);
        let meta = input.metadata();
        let stream_meta = stream.metadata();
        let get = |k: &str| meta.get(k).or_else(|| stream_meta.get(k)).map(|s| s.trim().to_string());
        if let Some(v) = get("title").filter(|s| !s.is_empty()) {
            t.title = v;
        }
        t.artist = get("artist").unwrap_or_default();
        t.album = get("album").unwrap_or_default();
        t.album_artist = get("album_artist").unwrap_or_default();
        t.genre = get("genre");
        t.track_no = get("track").and_then(|s| s.split('/').next()?.parse().ok());
        t.disc_no = get("disc").and_then(|s| s.split('/').next()?.parse().ok());
        t.year = get("date").and_then(|s| s.get(..4)?.parse().ok());
        true
    }

    fn save_cover(&self, t: &Track, data: &[u8]) -> Option<String> {
        let key = if t.album.is_empty() { t.path.clone() } else { format!("{}\u{1}{}", t.album_artist, t.album) };
        let ext = if data.starts_with(&[0x89, b'P', b'N', b'G']) { "png" } else { "jpg" };
        let file = self.covers_dir.join(format!("{}.{ext}", hash_str(&key)));
        if !file.exists() {
            std::fs::write(&file, data).ok()?;
        }
        Some(file.to_string_lossy().to_string())
    }

    /// Copies folder.jpg / cover.jpg into the cache, so the webview only ever reads from the cache dir.
    fn folder_cover(&self, track_path: &Path) -> Option<String> {
        let dir = track_path.parent()?;
        let names = ["cover", "folder", "front", "album", "Cover", "Folder", "Front", "AlbumArt"];
        for name in names {
            for ext in ["jpg", "jpeg", "png"] {
                let candidate = dir.join(format!("{name}.{ext}"));
                if candidate.is_file() {
                    let dest = self.covers_dir.join(format!("{}.{ext}", hash_str(&dir.to_string_lossy())));
                    if !dest.exists() {
                        std::fs::copy(&candidate, &dest).ok()?;
                    }
                    return Some(dest.to_string_lossy().to_string());
                }
            }
        }
        None
    }

    // ---- queries ------------------------------------------------------------------------

    pub fn tracks(&self) -> Result<Vec<Track>> {
        let db = self.db.lock();
        let mut stmt = db.prepare(&format!(
            "SELECT {TRACK_COLUMNS} FROM tracks WHERE missing_since IS NULL
             ORDER BY album_artist COLLATE NOCASE, album COLLATE NOCASE, disc_no, track_no, title COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map([], row_to_track)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn toggle_like(&self, id: i64) -> Result<bool> {
        let db = self.db.lock();
        db.execute("UPDATE tracks SET liked = 1 - liked WHERE id = ?1", [id])?;
        Ok(db.query_row("SELECT liked FROM tracks WHERE id = ?1", [id], |r| r.get(0))?)
    }

    pub fn mark_played(&self, id: i64) -> Result<()> {
        self.db
            .lock()
            .execute("UPDATE tracks SET play_count = play_count + 1, last_played = ?2 WHERE id = ?1", params![id, now()])?;
        Ok(())
    }

    // ---- playlists ----------------------------------------------------------------------

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        let db = self.db.lock();
        let mut stmt = db.prepare(
            "SELECT p.id, p.name, COUNT(t.id), p.created_at FROM playlists p
             LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
             LEFT JOIN tracks t ON t.id = pt.track_id AND t.missing_since IS NULL
             GROUP BY p.id ORDER BY p.name COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([], |r| Ok(Playlist { id: r.get(0)?, name: r.get(1)?, track_count: r.get(2)?, created_at: r.get(3)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn create_playlist(&self, name: &str) -> Result<i64> {
        let db = self.db.lock();
        db.execute("INSERT INTO playlists (name, created_at) VALUES (?1, ?2)", params![name, now()])?;
        Ok(db.last_insert_rowid())
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> Result<()> {
        self.db.lock().execute("UPDATE playlists SET name = ?2 WHERE id = ?1", params![id, name])?;
        Ok(())
    }

    pub fn delete_playlist(&self, id: i64) -> Result<()> {
        self.db.lock().execute("DELETE FROM playlists WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn playlist_tracks(&self, id: i64) -> Result<Vec<Track>> {
        let db = self.db.lock();
        let cols = TRACK_COLUMNS.split(", ").map(|c| format!("t.{c}")).collect::<Vec<_>>().join(", ");
        let mut stmt = db.prepare(&format!(
            "SELECT {cols} FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
             WHERE pt.playlist_id = ?1 AND t.missing_since IS NULL ORDER BY pt.position"
        ))?;
        let rows = stmt.query_map([id], row_to_track)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn add_to_playlist(&self, id: i64, track_ids: &[i64]) -> Result<()> {
        let db = self.db.lock();
        let tx = db.unchecked_transaction()?;
        let mut pos: i64 =
            tx.query_row("SELECT COALESCE(MAX(position), -1) FROM playlist_tracks WHERE playlist_id = ?1", [id], |r| r.get(0))?;
        for tid in track_ids {
            pos += 1;
            tx.execute("INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)", params![id, tid, pos])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_from_playlist(&self, id: i64, position: usize) -> Result<()> {
        let db = self.db.lock();
        db.execute(
            "DELETE FROM playlist_tracks WHERE rowid = (
                SELECT rowid FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position LIMIT 1 OFFSET ?2)",
            params![id, position as i64],
        )?;
        Ok(())
    }

    /// Permanently deletes songs: the file from storage and the song from the library and playlists.
    /// Files the OS won't let us delete (Android: other apps' files) are returned for the system prompt.
    pub fn delete_tracks(&self, ids: &[i64]) -> Result<DeleteResult> {
        let folders = self.folders()?;
        let mut res = DeleteResult::default();
        for &id in ids {
            let row: Option<(String, String, Option<String>)> = self
                .db
                .lock()
                .query_row("SELECT path, title, fp FROM tracks WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?;
            let Some((path, title, fp)) = row else { continue };
            let fp = fp.or_else(|| crate::sync::fingerprint(Path::new(&path)));
            let removed = std::fs::remove_file(&path);
            // Android's storage layer can report success (or "not found") without deleting a file
            // another app owns, so check the file is really gone before forgetting the song.
            if Path::new(&path).exists() {
                match removed {
                    Err(e) if e.kind() != std::io::ErrorKind::PermissionDenied && !cfg!(target_os = "android") => {
                        res.failed.push(format!("{title}: {e}"));
                    }
                    _ => res.needs_permission.push(DeleteBlocked { id, path, title }),
                }
                continue;
            }
            {
                let db = self.db.lock();
                db.execute("DELETE FROM tracks WHERE id = ?1", [id])?; // playlist entries and lyrics cascade
                if let Some(fp) = fp {
                    db.execute("INSERT OR REPLACE INTO deleted (fp, deleted_at) VALUES (?1, ?2)", params![fp, now()])?;
                }
            }
            // Tidy up folders left empty (never a library folder itself).
            let mut dir = Path::new(&path).parent();
            while let Some(d) = dir {
                let ds = d.to_string_lossy();
                if folders.iter().any(|f| f.trim_end_matches('/') == ds.trim_end_matches('/')) || std::fs::remove_dir(d).is_err() {
                    break;
                }
                dir = d.parent();
            }
            res.deleted.push(id);
        }
        Ok(res)
    }

    /// Direct database access for modules with their own tables/queries (sync.rs).
    pub(crate) fn with_db<R>(&self, f: impl FnOnce(&Connection) -> R) -> R {
        f(&self.db.lock())
    }

    // ---- settings ---------------------------------------------------------------------

    pub fn setting(&self, key: &str) -> Option<String> {
        self.db.lock().query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.db.lock().execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    // ---- lyrics cache -------------------------------------------------------------------

    pub fn cached_lyrics(&self, track_id: i64) -> Result<Option<crate::lyrics::Lyrics>> {
        let db = self.db.lock();
        Ok(db
            .query_row("SELECT synced, plain, source FROM lyrics WHERE track_id = ?1", [track_id], |r| {
                Ok(crate::lyrics::Lyrics { synced: r.get(0)?, plain: r.get(1)?, source: r.get(2)? })
            })
            .optional()?)
    }

    pub fn store_lyrics(&self, track_id: i64, l: &crate::lyrics::Lyrics) -> Result<()> {
        self.db.lock().execute(
            "INSERT OR REPLACE INTO lyrics (track_id, synced, plain, source, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![track_id, l.synced, l.plain, l.source, now()],
        )?;
        Ok(())
    }
}

fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn is_lossless_type(ft: lofty::file::FileType, has_bit_depth: bool, path: &Path) -> bool {
    use lofty::file::FileType::*;
    match ft {
        Flac | Ape | WavPack | Wav | Aiff => true,
        Mp4 => has_bit_depth, // ALAC reports a bit depth, AAC doesn't
        _ => matches!(ext_of(path).as_str(), "tta" | "tak"),
    }
}

fn codec_label(ft: lofty::file::FileType, has_bit_depth: bool, path: &Path) -> String {
    use lofty::file::FileType::*;
    match ft {
        Flac => "FLAC",
        Ape => "APE",
        WavPack => "WavPack",
        Wav => "WAV",
        Aiff => "AIFF",
        Mp4 if has_bit_depth => "ALAC",
        Mp4 | Aac => "AAC",
        Mpeg => "MP3",
        Opus => "Opus",
        Vorbis => "Vorbis",
        Speex => "Speex",
        Mpc => "Musepack",
        _ => return ext_of(path).to_ascii_uppercase(),
    }
    .to_string()
}

fn pretty_codec(name: &str) -> String {
    match name {
        n if n.starts_with("dsd_") => "DSD".into(),
        n if n.starts_with("pcm_") => "PCM".into(),
        "wmalossless" => "WMA Lossless".into(),
        "wmav1" | "wmav2" | "wmapro" => "WMA".into(),
        other => other.to_ascii_uppercase(),
    }
}
