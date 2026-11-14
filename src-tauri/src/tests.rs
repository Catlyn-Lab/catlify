//! Format coverage tests. Run with STAFY_TEST_MUSIC=/path/to/sample/files cargo test -- --nocapture
use crate::audio::decoder::{self, Decoder};
use crate::library::Library;
use std::path::PathBuf;

fn sample_files() -> Vec<PathBuf> {
    let Ok(dir) = std::env::var("STAFY_TEST_MUSIC") else { return vec![] };
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    files.sort();
    files
}

#[test]
fn decodes_every_format_at_output_rate() {
    decoder::init();
    for path in sample_files() {
        let src = path.to_string_lossy().to_string();
        let mut d = Decoder::open(&src, 48_000, 2).unwrap_or_else(|e| panic!("open {src}: {e:#}"));
        let mut buf = Vec::new();
        while d.decode_next(&mut buf).unwrap() {}
        let secs = buf.len() as f64 / 2.0 / 48_000.0;
        let peak = buf.iter().fold(0f32, |m, s| m.max(s.abs()));
        println!(
            "{:<28} {:<10} {:>6} Hz {:>2}-bit lossless={:<5} decoded {:.2}s / {:.2}s peak {:.2}",
            path.file_name().unwrap().to_string_lossy(),
            d.info.codec,
            d.info.sample_rate,
            d.info.bit_depth,
            d.info.lossless,
            secs,
            d.info.duration,
            peak
        );
        assert!((secs - d.info.duration).abs() < 0.15, "{src}: decoded {secs}s vs {}s", d.info.duration);
        assert!(peak > 0.05, "{src}: silent output");
    }
}

#[test]
fn seek_is_sample_accurate() {
    decoder::init();
    for path in sample_files() {
        let src = path.to_string_lossy().to_string();
        let mut d = Decoder::open(&src, 48_000, 2).unwrap();
        d.seek(2.0).unwrap();
        let mut buf = Vec::new();
        while d.decode_next(&mut buf).unwrap() {}
        let remaining = buf.len() as f64 / 2.0 / 48_000.0;
        let expected = d.info.duration - 2.0;
        assert!((remaining - expected).abs() < 0.1, "{src}: {remaining}s left after seek, expected {expected}s");
    }
}

#[test]
fn reads_tags_for_every_format() {
    decoder::init();
    let tmp = std::env::temp_dir().join(format!("stafy-test-{}", std::process::id()));
    let lib = Library::open(&tmp, &tmp).unwrap();
    for path in sample_files() {
        let t = lib.read_track(&path).unwrap_or_else(|| panic!("no tags for {path:?}"));
        println!(
            "{:<28} title={:<20} artist={:<14} codec={:<8} {:?}-bit {:?} Hz lossless={} cover={}",
            path.file_name().unwrap().to_string_lossy(),
            t.title,
            t.artist,
            t.codec,
            t.bit_depth,
            t.sample_rate,
            t.lossless,
            t.cover.is_some()
        );
        assert_ne!(t.artist, "Unknown Artist", "{path:?}");
        assert!(t.duration > 1.0, "{path:?}");
    }
    let _ = std::fs::remove_dir_all(tmp);
}

/// Drives the real engine through the default output device (muted). Run with --ignored.
#[test]
#[ignore]
fn engine_gapless_seek_and_end_of_queue() {
    use crate::audio::{Player, QueueItem};
    use std::time::Duration;
    decoder::init();
    let dir = std::env::var("STAFY_TEST_MUSIC").expect("STAFY_TEST_MUSIC");
    let item = |f: &str, t: &str| QueueItem {
        id: None,
        path: format!("{dir}/Mixed Formats/{f}"),
        title: t.into(),
        artist: "Sampler".into(),
        album: "Formats".into(),
        duration: 6.0,
        cover: None,
    };
    let player = Player::new(std::env::temp_dir().join("stafy-previews"), Box::new(|_, _| {}));
    player.set_volume(0.0);
    player.play_queue(vec![item("wavpack.wv", "A"), item("test.tta", "B")], 0);
    std::thread::sleep(Duration::from_millis(1200));
    let s = player.state();
    println!("t+1.2s  playing={} idx={:?} pos={:.2} err={:?}", s.playing, s.index, s.position, s.error);
    assert!(s.playing && s.index == Some(0) && (0.7..1.6).contains(&s.position));

    player.seek(5.0);
    std::thread::sleep(Duration::from_millis(1800));
    let s = player.state();
    println!("seek 5 +1.8s  idx={:?} pos={:.2} codec={:?}", s.index, s.position, s.stream.as_ref().map(|x| &x.codec));
    assert_eq!(s.index, Some(1), "gapless advance to second track");
    assert!((0.4..1.4).contains(&s.position));
    assert_eq!(s.stream.as_ref().unwrap().codec, "tta");

    player.pause();
    std::thread::sleep(Duration::from_millis(600));
    let p1 = player.state().position;
    std::thread::sleep(Duration::from_millis(600));
    let p2 = player.state().position;
    println!("paused pos {p1:.3} -> {p2:.3}");
    assert!((p2 - p1).abs() < 0.01, "position must not advance while paused");

    player.previous(); // >3s? no: ~1s in, so goes to previous track
    std::thread::sleep(Duration::from_millis(700));
    let s = player.state();
    println!("previous  playing={} idx={:?} pos={:.2}", s.playing, s.index, s.position);
    assert_eq!(s.index, Some(0));
    assert!(s.playing);

    player.jump(1);
    player.seek(5.3);
    std::thread::sleep(Duration::from_millis(1600));
    let s = player.state();
    println!("end of queue  playing={} idx={:?} pos={:.2}/{:.2}", s.playing, s.index, s.position, s.duration);
    assert!(!s.playing, "stops after last track with repeat off");
}

/// Minimal 16-bit stereo WAV with a title tag lofty can read.
fn write_wav(path: &std::path::Path) {
    let samples = 4410u32; // 0.1 s
    let data_len = samples * 4;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + data_len).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(44100u32.to_le_bytes());
    b.extend((44100u32 * 4).to_le_bytes());
    b.extend(4u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data_len.to_le_bytes());
    b.resize(b.len() + data_len as usize, 0);
    std::fs::write(path, b).unwrap();
}

/// Simulates app updates: reopening, the iOS container moving, and a folder going missing
/// must never lose songs, likes, play counts or playlists.
#[test]
fn library_survives_updates() {
    let root = std::env::temp_dir().join(format!("stafy-upgrade-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let old_home = root.join("Application/OLD-UUID");
    let new_home = root.join("Application/NEW-UUID");
    let music = old_home.join("Documents/Music");
    std::fs::create_dir_all(&music).unwrap();
    for n in ["a", "b", "c"] {
        write_wav(&music.join(format!("{n}.wav")));
    }
    let data = |h: &std::path::Path| h.join("Library/Application Support");
    let cache = |h: &std::path::Path| h.join("Library/Caches");

    // v1 install: scan, like a song, make a playlist.
    let ids = {
        let lib = Library::open(&data(&old_home), &cache(&old_home)).unwrap();
        lib.adopt_home(&old_home.to_string_lossy()).unwrap();
        lib.add_folder(&music.to_string_lossy()).unwrap();
        lib.scan(|_| {}).unwrap();
        let tracks = lib.tracks().unwrap();
        assert_eq!(tracks.len(), 3);
        lib.toggle_like(tracks[0].id).unwrap();
        let pl = lib.create_playlist("Road trip").unwrap();
        lib.add_to_playlist(pl, &[tracks[1].id, tracks[2].id]).unwrap();
        tracks.iter().map(|t| t.id).collect::<Vec<_>>()
    };

    // Same container, new version: reopen runs migrations on the existing DB.
    {
        let lib = Library::open(&data(&old_home), &cache(&old_home)).unwrap();
        lib.scan(|_| {}).unwrap();
        assert_eq!(lib.tracks().unwrap().len(), 3);
    }

    // iOS update: the whole container moves to a new UUID path.
    std::fs::rename(&old_home, &new_home).unwrap();
    {
        let lib = Library::open(&data(&new_home), &cache(&new_home)).unwrap();
        lib.adopt_home(&new_home.to_string_lossy()).unwrap();
        lib.scan(|_| {}).unwrap();
        let tracks = lib.tracks().unwrap();
        assert_eq!(tracks.iter().map(|t| t.id).collect::<Vec<_>>(), ids, "same track ids after move");
        assert!(tracks.iter().all(|t| t.path.starts_with(&*new_home.to_string_lossy())), "paths rebased");
        assert!(tracks.iter().find(|t| t.id == ids[0]).unwrap().liked, "like kept");
        assert_eq!(lib.folders().unwrap(), vec![new_home.join("Documents/Music").to_string_lossy().to_string()]);
        let pl = lib.playlists().unwrap();
        assert_eq!(pl[0].track_count, 2, "playlist kept");
    }

    // Folder unavailable (permission revoked / drive unplugged): nothing is forgotten.
    let moved = new_home.join("Documents/Music-away");
    std::fs::rename(new_home.join("Documents/Music"), &moved).unwrap();
    {
        let lib = Library::open(&data(&new_home), &cache(&new_home)).unwrap();
        lib.scan(|_| {}).unwrap();
        assert_eq!(lib.tracks().unwrap().len(), 3, "tracks kept while folder is missing");
        assert_eq!(lib.playlists().unwrap()[0].track_count, 2);
    }
    // A file really deleted from an available folder is still forgotten.
    std::fs::rename(&moved, new_home.join("Documents/Music")).unwrap();
    std::fs::remove_file(new_home.join("Documents/Music/c.wav")).unwrap();
    {
        let lib = Library::open(&data(&new_home), &cache(&new_home)).unwrap();
        lib.scan(|_| {}).unwrap();
        assert_eq!(lib.tracks().unwrap().len(), 2);
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Network + speakers (muted): full songs stream from YouTube, the next one is warmed in the
/// background, and skipping to it starts almost immediately.
#[test]
#[ignore]
fn engine_streams_youtube() {
    use crate::audio::{Player, QueueItem};
    use std::time::{Duration, Instant};
    decoder::init();
    crate::audio::engine::set_stream_resolver(Box::new(crate::youtube::fetch_stream));
    let cache = std::env::temp_dir().join("stafy-yt-engine");
    let _ = std::fs::remove_dir_all(&cache);
    let item = |path: &str, t: &str| QueueItem { id: None, path: path.into(), title: t.into(), artist: "".into(), album: "".into(), duration: 0.0, cover: None };
    let player = Player::new(cache.clone(), Box::new(|_, _| {}));
    player.set_volume(0.0);
    let start = Instant::now();
    player.play_queue(vec![item("yt:YE7VzlLtp-4", "Big Buck Bunny"), item("ytq:Gloria Jessica Terbelah Jadi Dua", "Terbelah")], 0);
    std::thread::sleep(Duration::from_millis(300));
    let s = player.state();
    println!("t+0.3s loading={} track={:?}", s.loading, s.track.as_ref().map(|t| &t.title));
    assert!(s.loading, "reports loading while fetching");
    while player.state().loading && start.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(200));
    }
    println!("first song started after {:?}", start.elapsed());
    std::thread::sleep(Duration::from_secs(2));
    let s = player.state();
    println!("playing={} pos={:.1} dur={:.0} err={:?}", s.playing, s.position, s.duration, s.error);
    assert!(s.playing && s.position > 1.0 && s.duration > 500.0, "full song, not a preview");
    // The next song warms in the background; wait for it, then skip.
    let warm = Instant::now();
    while std::fs::read_dir(&cache).unwrap().count() < 2 && warm.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(250));
    }
    println!("next song warmed after {:?}", warm.elapsed());
    let skip = Instant::now();
    player.next();
    while (player.state().loading || player.state().index != Some(1)) && skip.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let s = player.state();
    println!("skip took {:?}: idx={:?} dur={:.0}", skip.elapsed(), s.index, s.duration);
    assert!(skip.elapsed() < Duration::from_secs(2), "warmed song starts quickly");
    assert!(s.duration > 200.0);
    let _ = std::fs::remove_dir_all(&cache);
}

/// Like `write_wav`, but with distinct audio so each file has its own fingerprint.
fn write_wav_seeded(path: &std::path::Path, seed: u8) {
    write_wav(path);
    let mut b = std::fs::read(path).unwrap();
    let n = b.len();
    for (i, x) in b[44..n].iter_mut().enumerate() {
        *x = seed.wrapping_add((i % 251) as u8);
    }
    std::fs::write(path, b).unwrap();
}

/// Two devices pair with a code and sync both ways over the network (localhost here):
/// songs are copied, likes and playlists merged, and a second sync copies nothing new.
#[test]
fn wifi_sync_two_devices() {
    use crate::sync::Sync;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    let root = std::env::temp_dir().join(format!("catlify-sync-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let device = |name: &str, songs: &[(&str, u8)]| {
        let dir = root.join(name);
        let music = dir.join("Music");
        std::fs::create_dir_all(&music).unwrap();
        for (file, seed) in songs {
            write_wav_seeded(&music.join(file), *seed);
        }
        let lib = Arc::new(Library::open(&dir.join("data"), &dir.join("cache")).unwrap());
        lib.set_setting("sync_dir", &dir.join("Synced").to_string_lossy()).unwrap();
        lib.set_setting("device_name", name).unwrap();
        lib.add_folder(&music.to_string_lossy()).unwrap();
        lib.scan(|_| {}).unwrap();
        lib
    };
    let lib_a = device("Mac", &[("a1.wav", 10), ("a2.wav", 20)]);
    let lib_b = device("Phone", &[("b1.wav", 30)]);
    let a1 = lib_a.tracks().unwrap().into_iter().find(|t| t.path.ends_with("a1.wav")).unwrap();
    lib_a.toggle_like(a1.id).unwrap();
    let pa = lib_a.create_playlist("Road").unwrap();
    lib_a.add_to_playlist(pa, &lib_a.tracks().unwrap().iter().map(|t| t.id).collect::<Vec<_>>()).unwrap();
    let pb = lib_b.create_playlist("Road").unwrap();
    lib_b.add_to_playlist(pb, &[lib_b.tracks().unwrap()[0].id]).unwrap();

    let sa = Sync::start(lib_a.clone()).unwrap();
    let sb = Sync::start(lib_b.clone()).unwrap();

    // Phone pairs with the Mac: the Mac shows a code, the phone types it.
    sb.pair_begin(&format!("127.0.0.1:{}", sa.port())).unwrap();
    let code = sa.pending_code().expect("Mac shows a code");
    assert!(sb.pair_complete("000000").is_err() || code == "000000", "wrong code rejected");
    let a_info = sb.pair_complete(&code).expect("right code pairs");
    let a_id = a_info["id"].as_str().unwrap().to_string();
    println!("paired with {} (code {code})", a_info["name"]);

    let wait = |s: &Arc<Sync>, what: &str| {
        let t = Instant::now();
        loop {
            let p = s.progress();
            if !p.active && (p.phase == "done" || p.phase == "error") {
                println!("{what}: {} copied={} err={:?}", p.phase, p.added, p.error);
                assert_eq!(p.phase, "done", "{what} failed: {:?}", p.error);
                return p;
            }
            assert!(t.elapsed() < Duration::from_secs(30), "{what} timed out");
            std::thread::sleep(Duration::from_millis(50));
        }
    };

    sb.sync_now(&a_id).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let p = wait(&sb, "phone <- mac");
    assert_eq!(p.added, 2);
    // The Mac was asked to pull from the phone in turn.
    std::thread::sleep(Duration::from_millis(300));
    let p = wait(&sa, "mac <- phone");
    assert_eq!(p.added, 1);

    for (name, lib) in [("Mac", &lib_a), ("Phone", &lib_b)] {
        let tracks = lib.tracks().unwrap();
        let road = lib.playlists().unwrap().into_iter().find(|p| p.name == "Road").unwrap();
        println!("{name}: {} songs, Road has {}", tracks.len(), road.track_count);
        assert_eq!(tracks.len(), 3, "{name} has every song");
        assert_eq!(road.track_count, 3, "{name}'s playlist merged");
    }
    let liked_on_phone = lib_b.tracks().unwrap().into_iter().filter(|t| t.liked).count();
    assert_eq!(liked_on_phone, 1, "like synced to the phone");

    // Syncing again finds nothing new and makes no duplicates.
    sb.sync_now(&a_id).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(wait(&sb, "second sync").added, 0);
    std::thread::sleep(Duration::from_millis(300));
    wait(&sa, "second sync back");
    assert_eq!(lib_a.tracks().unwrap().len(), 3);
    assert_eq!(lib_b.tracks().unwrap().len(), 3);
    let _ = std::fs::remove_dir_all(&root);
}

/// Manual cross-device check: a headless Catlify "device" with the sample library. Writes its
/// port and pairing code to /tmp/catlify-peer.json and keeps serving for a few minutes.
#[test]
#[ignore]
fn sync_peer_server() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    let dir = std::env::temp_dir().join("catlify-peer"); // kept between runs so pairings survive
    let lib = Arc::new(Library::open(&dir.join("data"), &dir.join("cache")).unwrap());
    lib.set_setting("device_name", "Test Mac").unwrap();
    lib.set_setting("sync_dir", &dir.join("Synced").to_string_lossy()).unwrap();
    let music = std::env::var("STAFY_TEST_MUSIC").expect("STAFY_TEST_MUSIC");
    lib.add_folder(&format!("{music}/Aurora Lines")).unwrap();
    lib.add_folder(&format!("{music}/Mixed Formats")).unwrap();
    lib.scan(|_| {}).unwrap();
    let s = crate::sync::Sync::start(lib.clone()).unwrap();
    println!("peer on port {} with {} songs", s.port(), lib.tracks().unwrap().len());
    let start = Instant::now();
    let mut last = String::new();
    while start.elapsed() < Duration::from_secs(420) {
        let p = s.progress();
        let state = serde_json::json!({ "port": s.port(), "code": s.pending_code(), "songs": lib.tracks().unwrap().len(),
            "phase": p.phase, "added": p.added, "error": p.error }).to_string();
        if state != last {
            std::fs::write("/tmp/catlify-peer.json", &state).unwrap();
            println!("{state}");
            last = state;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// The Android "permission revoked" case: a folder still lists some files but others are hidden.
/// Hidden songs disappear from the library but keep their like and playlist spot, and come back.
#[test]
fn hidden_songs_are_kept_not_deleted() {
    let root = std::env::temp_dir().join(format!("catlify-hidden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let music = root.join("Music");
    std::fs::create_dir_all(&music).unwrap();
    write_wav_seeded(&music.join("mine.wav"), 1);
    write_wav_seeded(&music.join("theirs.wav"), 2);
    let lib = Library::open(&root.join("data"), &root.join("cache")).unwrap();
    lib.add_folder(&music.to_string_lossy()).unwrap();
    lib.scan(|_| {}).unwrap();
    let theirs = lib.tracks().unwrap().into_iter().find(|t| t.path.ends_with("theirs.wav")).unwrap();
    lib.toggle_like(theirs.id).unwrap();
    let pl = lib.create_playlist("Mix").unwrap();
    lib.add_to_playlist(pl, &[theirs.id]).unwrap();

    // The file becomes invisible while the folder still has other songs.
    std::fs::rename(music.join("theirs.wav"), root.join("away.wav")).unwrap();
    lib.scan(|_| {}).unwrap();
    assert_eq!(lib.tracks().unwrap().len(), 1, "hidden song not shown");
    assert_eq!(lib.playlists().unwrap()[0].track_count, 0);

    // It comes back: same song, like and playlist intact.
    std::fs::rename(root.join("away.wav"), music.join("theirs.wav")).unwrap();
    lib.scan(|_| {}).unwrap();
    let back = lib.tracks().unwrap().into_iter().find(|t| t.path.ends_with("theirs.wav")).unwrap();
    assert_eq!(back.id, theirs.id, "same library entry");
    assert!(back.liked, "like kept");
    assert_eq!(lib.playlists().unwrap()[0].track_count, 1, "playlist spot kept");
    let _ = std::fs::remove_dir_all(&root);
}

/// Network: picking another song while a YouTube song is still loading switches right away
/// (the old fetch no longer blocks), and the state reports loading for the chosen song.
#[test]
#[ignore]
fn switching_while_loading_is_instant() {
    use crate::audio::{Player, QueueItem};
    use std::time::{Duration, Instant};
    decoder::init();
    crate::audio::engine::set_stream_resolver(Box::new(crate::youtube::fetch_stream));
    let cache = std::env::temp_dir().join("stafy-yt-switch");
    let _ = std::fs::remove_dir_all(&cache);
    let dir = std::env::var("STAFY_TEST_MUSIC").expect("STAFY_TEST_MUSIC");
    let item = |path: String, t: &str| QueueItem { id: None, path, title: t.into(), artist: "".into(), album: "".into(), duration: 0.0, cover: None };
    let player = Player::new(cache.clone(), Box::new(|_, _| {}));
    player.set_volume(0.0);
    player.play_queue(vec![item("yt:YE7VzlLtp-4".into(), "Bunny (YouTube)"), item(format!("{dir}/Mixed Formats/test.mp3"), "Local MP3")], 0);
    std::thread::sleep(Duration::from_millis(400));
    let s = player.state();
    println!("t+0.4s loading={} track={:?} pos={}", s.loading, s.track.as_ref().map(|t| &t.title), s.position);
    assert!(s.loading && s.track.as_ref().unwrap().title == "Bunny (YouTube)");
    let t = Instant::now();
    player.next();
    loop {
        let s = player.state();
        if !s.loading && s.index == Some(1) && s.position > 0.2 {
            println!("local song playing {:?} after next()", t.elapsed());
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(3), "stuck behind the YouTube fetch: {:?}", s.track.map(|t| t.title));
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&cache);
}

/// Deleting a song removes the file and its playlist entries, and Wi-Fi sync doesn't bring it back.
#[test]
fn deleted_songs_stay_deleted() {
    use crate::sync::Sync;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    let root = std::env::temp_dir().join(format!("catlify-delete-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let device = |name: &str, songs: &[(&str, u8)]| {
        let dir = root.join(name);
        let music = dir.join("Music").join("Album");
        std::fs::create_dir_all(&music).unwrap();
        for (file, seed) in songs {
            write_wav_seeded(&music.join(file), *seed);
        }
        let lib = Arc::new(Library::open(&dir.join("data"), &dir.join("cache")).unwrap());
        lib.set_setting("sync_dir", &dir.join("Synced").to_string_lossy()).unwrap();
        lib.add_folder(&dir.join("Music").to_string_lossy()).unwrap();
        lib.scan(|_| {}).unwrap();
        lib
    };
    let lib_a = device("A", &[("keep.wav", 1), ("gone.wav", 2)]);
    let lib_b = device("B", &[]);
    let sa = Sync::start(lib_a.clone()).unwrap();
    let sb = Sync::start(lib_b.clone()).unwrap();
    sb.pair_begin(&format!("127.0.0.1:{}", sa.port())).unwrap();
    let a_id = sb.pair_complete(&sa.pending_code().unwrap()).unwrap()["id"].as_str().unwrap().to_string();
    let wait = |s: &Arc<Sync>| {
        let t = Instant::now();
        while s.progress().active || s.progress().phase.is_empty() {
            assert!(t.elapsed() < Duration::from_secs(20));
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(300)); // reverse pull
    };
    sb.run(&a_id, false);
    wait(&sb);
    assert_eq!(lib_b.tracks().unwrap().len(), 2);

    // Delete one song on B (it's in a playlist there).
    let gone = lib_b.tracks().unwrap().into_iter().find(|t| t.path.ends_with("gone.wav")).unwrap();
    let pl = lib_b.create_playlist("Mix").unwrap();
    lib_b.add_to_playlist(pl, &[gone.id]).unwrap();
    let r = lib_b.delete_tracks(&[gone.id]).unwrap();
    assert_eq!(r.deleted, vec![gone.id]);
    assert!(!std::path::Path::new(&gone.path).exists(), "file deleted from storage");
    assert!(std::path::Path::new(&gone.path).parent().unwrap().exists(), "folder kept: the other song is still in it");
    // Deleting the last song in a folder removes the now-empty folder too.
    let keep = lib_b.tracks().unwrap().into_iter().next().unwrap();
    let album_dir = std::path::Path::new(&keep.path).parent().unwrap().to_path_buf();
    lib_b.delete_tracks(&[keep.id]).unwrap();
    assert!(!album_dir.exists(), "empty album folder removed");
    sb.run(&a_id, false);
    assert_eq!(lib_b.tracks().unwrap().len(), 0, "neither deleted song comes back");
    assert_eq!(lib_b.playlists().unwrap()[0].track_count, 0, "removed from playlists");

    assert_eq!(lib_a.tracks().unwrap().len(), 2, "other device keeps its copy");
    let _ = std::fs::remove_dir_all(&root);
}

/// Network: a song whose link was looked up early (`prepare`) starts in about a second; without
/// it the lookup adds ~4 s. Both use the direct fetch (no yt-dlp download).
#[test]
#[ignore]
fn prepared_songs_start_fast() {
    use crate::audio::{engine, Player, QueueItem};
    use std::time::{Duration, Instant};
    decoder::init();
    engine::set_stream_resolver(Box::new(crate::youtube::fetch_stream));
    engine::set_link_extractor(Box::new(crate::youtube::extract_link));
    let cache = std::env::temp_dir().join("stafy-yt-prepare");
    let _ = std::fs::remove_dir_all(&cache);
    let item = |p: &str| QueueItem { id: None, path: p.into(), title: p.into(), artist: "".into(), album: "".into(), duration: 0.0, cover: None };
    let player = Player::new(cache.clone(), Box::new(|_, _| {}));
    player.set_volume(0.0);
    let time_to_play = |path: &str| {
        let t = Instant::now();
        player.play_queue(vec![item(path)], 0);
        loop {
            let s = player.state();
            if !s.loading && s.position > 0.05 {
                return t.elapsed();
            }
            assert!(t.elapsed() < Duration::from_secs(45), "never started: {:?}", s.error);
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    let cold = time_to_play("yt:pA8puuLjw4U");
    println!("cold start (lookup + fetch): {cold:?}");
    engine::prepare(&["ytq:Ashe Stop The Wedding!".into()]);
    std::thread::sleep(Duration::from_secs(9)); // user browsing while the link is looked up
    let warm = time_to_play("ytq:Ashe Stop The Wedding!");
    println!("prepared start (fetch only): {warm:?}");
    let s = player.state();
    println!("playing {:.0}s song, error={:?}", s.duration, s.error);
    assert!(warm < Duration::from_secs(3), "prepared song should start quickly");
    assert!(s.duration > 120.0, "full song");
    let _ = std::fs::remove_dir_all(&cache);
}

