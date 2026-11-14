// Dev-only: lets the UI run in a plain browser (vite dev / screenshots) without the Rust backend.
import { mockIPC, mockConvertFileSrc } from "@tauri-apps/api/mocks";
import chart from "./mock-chart.json";

export function installMocks() {
  mockConvertFileSrc("macos");
  const albums = [
    ["Aurora Lines", "Night Drive", 2024, "FLAC", 24, 96000, true],
    ["Mara Quell", "Low Tide Hymns", 2021, "ALAC", 16, 44100, true],
    ["The Paper Kites", "Swell", 2019, "MP3", null, 44100, false],
    ["Juno Vale", "Satellites", 2023, "FLAC", 24, 192000, true],
    ["Okta Field", "Static Bloom", 2018, "DSD", 1, 2822400, true],
  ] as const;
  let id = 0;
  const tracks = albums.flatMap(([artist, album, year, codec, bits, rate, lossless], ai) =>
    ["Opening", "Harbor Lights", "Slow Orbit", "Paper Moon", "Afterglow", "Northbound"].map((t, i) => ({
      id: ++id, path: `/music/${album}/${i + 1}.flac`, title: `${t}${ai ? "" : " (Extended)"}`, artist, album, albumArtist: artist,
      trackNo: i + 1, discNo: 1, year, genre: "Electronic", duration: 170 + ((id * 37) % 120), sampleRate: rate, bitDepth: bits,
      channels: 2, codec, lossless, cover: null, addedAt: 1_700_000_000 + id * 1000, playCount: (id * 7) % 13,
      lastPlayed: id % 3 ? 1_700_000_000 + id : null, liked: id % 4 === 0,
    })),
  );
  const state = {
    playing: true, position: 74, duration: 241, index: 1, volume: 0.7, shuffle: false, repeat: "all", error: null, queueVersion: 1,
    track: { id: 2, path: tracks[1].path, title: tracks[1].title, artist: tracks[1].artist, album: tracks[1].album, duration: 241, cover: null },
    stream: { codec: "flac", sampleRate: 96000, bitDepth: 24, channels: 2, lossless: true, duration: 241, outputRate: 48000 },
  };
  const lrc = ["Neon lights across the water", "Every signal fading slow", "Hold the line until the morning", "Where the quiet rivers go", "", "Oh, the city hums in silver", "And I'm halfway out the door", "Every echo pulls me under", "Like it did the time before"]
    .map((l, i) => `[01:${String(4 + i * 4).padStart(2, "0")}.00]${l}`).join("\n");

  let sleep: { active: boolean; endsAt: number | null; endOfTrack: boolean } = { active: false, endsAt: null, endOfTrack: false };
  const rpc = (method: string, args: Record<string, unknown>) => {
    switch (method) {
      case "categories": return {
        genres: [["14", "Pop"], ["18", "Hip-Hop/Rap"], ["15", "R&B/Soul"], ["21", "Rock"], ["20", "Alternative"], ["17", "Dance"], ["7", "Electronic"], ["51", "K-Pop"]].map(([id, name]) => ({ id, name })),
        moods: [["chill", "Chill"], ["focus", "Focus"], ["workout", "Workout"], ["party", "Party"], ["sleep", "Sleep"], ["roadtrip", "Road Trip"]].map(([id, name]) => ({ id, name })),
      };
      case "sleepStatus": return sleep;
      case "sleepSet": return (sleep = args.endOfTrack ? { active: true, endsAt: null, endOfTrack: true } : { active: true, endsAt: Date.now() + Number(args.minutes) * 60_000, endOfTrack: false });
      case "sleepCancel": return (sleep = { active: false, endsAt: null, endOfTrack: false });
      default: return null;
    }
  };

  mockIPC((cmd, payload) => {
    switch (cmd) {
      case "rpc": {
        const p = payload as { method: string; args: Record<string, unknown> };
        return rpc(p.method, p.args ?? {});
      }
      case "youtube_video": return { id: "dQw4w9WgXcQ", title: "Rick Astley - Never Gonna Give You Up (Official Video)", channel: "Rick Astley", url: null, thumbnail: null };
      case "plugin:app|version": return "0.1.14";
      // ?update shows the update popup.
      case "plugin:updater|check": return new URLSearchParams(location.search).has("update")
        ? { rid: 1, currentVersion: "0.1.14", version: "0.1.15", date: "2026-11-15 10:00:00.0 +00:00:00", body: "• In-app updates for macOS and Windows\n• Sleep timer on the car screen\n• Faster chart loading", rawJson: {} }
        : null;
      case "library_tracks": return tracks;
      case "library_folders": return ["/Users/you/Music"];
      case "playlists": return [{ id: 1, name: "Late Night Lossless", trackCount: 14, createdAt: 0 }, { id: 2, name: "Focus", trackCount: 8, createdAt: 0 }];
      case "player_state": return state;
      case "player_queue": return tracks.slice(0, 12).map((t, i) => ({ index: i, item: { ...t } }));
      case "charts_get": return chart;
      case "lyrics_get": return { synced: lrc, plain: null, source: "lrclib" };
      case "audio_extensions": return ["flac", "mp3"];
      case "youtube_status": return { ytDlp: "2026.10.26", ffmpeg: true, downloadDir: "/Users/you/Music/Catlify" };
      case "youtube_lookup": return [
        { id: "d_HlPboLRL8", url: "https://www.youtube.com/watch?v=d_HlPboLRL8", title: "Runaway", artist: "AURORA", videoTitle: "AURORA - Runaway", channel: "AURORA", duration: 250, thumbnail: "https://i.ytimg.com/vi/d_HlPboLRL8/hqdefault.jpg" },
        { id: "7ifyPIoevKM", url: "https://www.youtube.com/watch?v=7ifyPIoevKM", title: "Runaway", artist: "Aurora", videoTitle: "Aurora - Runaway (lyrics)", channel: "Chilled Sad", duration: 251, thumbnail: "https://i.ytimg.com/vi/7ifyPIoevKM/hqdefault.jpg" },
      ];
      case "youtube_download": return null;
      case "plugin:event|listen": return 1;
      default: return null;
    }
  });
}
