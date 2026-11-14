import { invoke } from "@tauri-apps/api/core";

export type Repeat = "off" | "all" | "one";

export interface QueueItem {
  id: number | null;
  path: string;
  title: string;
  artist: string;
  album: string;
  duration: number;
  cover: string | null;
}

export interface StreamInfo {
  codec: string;
  sampleRate: number;
  bitDepth: number;
  channels: number;
  lossless: boolean;
  duration: number;
  outputRate: number;
}

/** Android's bundled yt-dlp (StafyYouTube.kt); absent on desktop, iOS and store builds. */
interface YtBridge {
  status(callId: string): void;
  lookup(callId: string, query: string): void;
  lookupAll(callId: string, query: string, playlist: boolean): void;
  update(callId: string): void;
  video(callId: string, query: string): void;
  download(id: string, url: string, title: string, artist: string, format: string): void;
  cancel(id: string): void;
  takeShare(): string | null;
}
export const ytBridge = () => (window as unknown as { StafyYouTube?: YtBridge }).StafyYouTube;

const ytPending = new Map<string, { resolve: (v: unknown) => void; reject: (e: unknown) => void }>();
(window as unknown as { __stafyYt: (id: string, r: { ok?: unknown; error?: string }) => void }).__stafyYt = (id, r) => {
  const p = ytPending.get(id);
  ytPending.delete(id);
  if (r.error !== undefined) p?.reject(r.error);
  else p?.resolve(r.ok);
};
let ytSeq = 0;
function ytCall<T>(start: (id: string) => void): Promise<T> {
  const id = `yt${++ytSeq}`;
  return new Promise<T>((resolve, reject) => {
    ytPending.set(id, { resolve: resolve as (v: unknown) => void, reject });
    start(id);
  });
}

export interface ImportTrack {
  title: string;
  artist: string;
  album: string | null;
  duration: number | null;
  videoId: string | null;
}

export interface ImportCollection {
  name: string;
  source: string;
  tracks: ImportTrack[];
  truncated: boolean;
}

export interface YtStatus {
  ytDlp: string | null;
  ffmpeg: boolean;
  downloadDir: string;
}

export interface YtVideo {
  id: string;
  url: string;
  title: string;
  artist: string;
  videoTitle: string;
  channel: string;
  duration: number | null;
  thumbnail: string;
}

export interface YtProgress {
  id: string;
  stage: "downloading" | "converting" | "done" | "error" | "cancelled";
  percent: number;
  path: string | null;
  error: string | null;
}

/** "See video": a direct MP4 link with sound, valid for a few hours on this device. */
export interface VideoStream {
  id: string;
  title: string | null;
  channel: string | null;
  url: string | null;
  thumbnail: string | null;
}

export interface SleepStatus {
  active: boolean;
  /** Unix ms when playback pauses (minutes timer). */
  endsAt: number | null;
  endOfTrack: boolean;
}

export interface Categories {
  genres: { id: string; name: string }[];
  moods: { id: string; name: string }[];
}

export interface PlayerState {
  playing: boolean;
  position: number;
  duration: number;
  index: number | null;
  track: QueueItem | null;
  volume: number;
  shuffle: boolean;
  repeat: Repeat;
  stream: StreamInfo | null;
  error: string | null;
  queueVersion: number;
  ended?: boolean;
  /** The current song is still being fetched (YouTube streams take a few seconds). */
  loading?: boolean;
}

export interface Track {
  id: number;
  path: string;
  title: string;
  artist: string;
  album: string;
  albumArtist: string;
  trackNo: number | null;
  discNo: number | null;
  year: number | null;
  genre: string | null;
  duration: number;
  sampleRate: number | null;
  bitDepth: number | null;
  channels: number | null;
  codec: string;
  lossless: boolean;
  cover: string | null;
  addedAt: number;
  playCount: number;
  lastPlayed: number | null;
  liked: boolean;
}

export interface Playlist {
  id: number;
  name: string;
  trackCount: number;
  createdAt: number;
}

export interface ScanProgress {
  scanned: number;
  total: number;
  added: number;
  done: boolean;
  current: string | null;
}

export interface Lyrics {
  synced: string | null;
  plain: string | null;
  source: string;
}

export interface ChartEntry {
  rank: number;
  title: string;
  artist: string;
  album: string | null;
  artwork: string | null;
  preview: string | null;
  url: string | null;
  releaseDate: string | null;
  genre: string | null;
  explicit: boolean;
  duration: number | null;
  previousRank: number | null;
  weeks: number | null;
  /** YouTube Music charts: the exact video to download. */
  videoUrl: string | null;
}

export interface Chart {
  title: string;
  source: string;
  updated: string | null;
  entries: ChartEntry[];
}

/** Same JSON RPC the car integrations use (src-tauri/src/rpc.rs). */
export const rpc = <T>(method: string, args: Record<string, unknown> = {}) => invoke<T>("rpc", { method, args });

export interface SyncDevice {
  id: string;
  name: string;
  addr: string | null;
  online?: boolean;
  paired?: boolean;
  lastSync?: number | null;
}

export interface SyncProgress {
  active: boolean;
  peer: string | null;
  phase: "connecting" | "comparing" | "copying" | "merging" | "done" | "error" | "";
  done: number;
  total: number;
  bytes: number;
  current: string | null;
  added: number;
  failed: number;
  error: string | null;
  finishedAt: number | null;
}

export interface SyncStatus {
  enabled: boolean;
  me: { id?: string; name: string; port?: number; addresses?: string[] };
  nearby: SyncDevice[];
  paired: SyncDevice[];
  incoming?: { name: string; code: string } | null;
  outgoing?: { name: string } | null;
  progress: SyncProgress | null;
  syncDir: string;
}

export const api = {
  playQueue: (items: QueueItem[], startIndex: number) => invoke<void>("player_play_queue", { items, startIndex }),
  enqueue: (items: QueueItem[], playNext: boolean) => invoke<void>("player_enqueue", { items, playNext }),
  removeFromQueue: (index: number) => invoke<void>("player_remove", { index }),
  jump: (index: number) => invoke<void>("player_jump", { index }),
  queue: () => invoke<{ index: number; item: QueueItem }[]>("player_queue"),
  state: () => invoke<PlayerState>("player_state"),
  control: (action: "play" | "pause" | "toggle" | "next" | "previous") => invoke<void>("player_control", { action }),
  seek: (position: number) => invoke<void>("player_seek", { position }),
  setVolume: (volume: number) => invoke<void>("player_set_volume", { volume }),
  setShuffle: (shuffle: boolean) => invoke<void>("player_set_shuffle", { shuffle }),
  setRepeat: (repeat: Repeat) => invoke<void>("player_set_repeat", { repeat }),

  tracks: () => invoke<Track[]>("library_tracks"),
  folders: () => invoke<string[]>("library_folders"),
  addFolder: (path: string) => invoke<void>("library_add_folder", { path }),
  removeFolder: (path: string) => invoke<void>("library_remove_folder", { path }),
  rescan: () => invoke<void>("library_rescan"),
  isScanning: () => invoke<boolean>("library_is_scanning"),
  toggleLike: (id: number) => invoke<boolean>("library_toggle_like", { id }),
  probeFiles: (paths: string[]) => invoke<Track[]>("probe_files", { paths }),
  audioExtensions: () => invoke<string[]>("audio_extensions"),

  playlists: () => invoke<Playlist[]>("playlists"),
  createPlaylist: (name: string) => invoke<number>("playlist_create", { name }),
  renamePlaylist: (id: number, name: string) => invoke<void>("playlist_rename", { id, name }),
  deletePlaylist: (id: number) => invoke<void>("playlist_delete", { id }),
  playlistTracks: (id: number) => invoke<Track[]>("playlist_tracks", { id }),
  addToPlaylist: (id: number, trackIds: number[]) => invoke<void>("playlist_add", { id, trackIds }),
  removeFromPlaylist: (id: number, position: number) => invoke<void>("playlist_remove", { id, position }),

  lyrics: (item: QueueItem, force = false) => invoke<Lyrics>("lyrics_get", { item, force }),
  chart: (source: string, country: string, kind: string) => invoke<Chart>("charts_get", { source, country, kind }),
  // Desktop: Rust commands + system yt-dlp. Android: bundled yt-dlp via window.StafyYouTube.
  ytStatus: () => (ytBridge() ? ytCall<YtStatus>((id) => ytBridge()!.status(id)) : invoke<YtStatus>("youtube_status")),
  /** `playlist`: list every song of a YouTube / YouTube Music playlist link (import). */
  ytLookup: (query: string, playlist = false) =>
    ytBridge()
      ? ytCall<YtVideo[]>((id) => (playlist ? ytBridge()!.lookupAll(id, query, true) : ytBridge()!.lookup(id, query)))
      : invoke<YtVideo[]>("youtube_lookup", { query, playlist }),
  importLink: (url: string) => invoke<ImportCollection>("import_link", { url }),
  importFile: (name: string, text: string) => invoke<ImportCollection[]>("import_file", { name, text }),
  ytDownload: async (v: { url: string; id: string; title: string; artist: string }, format: "mp3" | "original") => {
    const b = ytBridge();
    if (b) b.download(v.id, v.url, v.title, v.artist, format);
    else await invoke<void>("youtube_download", { ...v, format });
  },
  ytCancel: async (id: string) => {
    const b = ytBridge();
    if (b) b.cancel(id);
    else await invoke<void>("youtube_cancel", { id });
  },
  /** Song search ("artist title"), video id or link → playable video. Desktop and Android (yt-dlp). */
  ytVideo: (query: string) =>
    ytBridge() ? ytCall<VideoStream>((id) => ytBridge()!.video(id, query)) : invoke<VideoStream>("youtube_video", { query }),
  categories: () => rpc<Categories>("categories"),
  sleepStatus: () => rpc<SleepStatus>("sleepStatus"),
  sleepSet: (o: { minutes: number } | { endOfTrack: true }) => rpc<SleepStatus>("sleepSet", o),
  sleepCancel: () => rpc<SleepStatus>("sleepCancel"),
  ytUpdate: () => ytCall<string | null>((id) => ytBridge()!.update(id)),
  ytSetDir: (path: string) => invoke<void>("youtube_set_dir", { path }),
  notchEnabled: () => invoke<boolean>("notch_enabled"),
  setNotchEnabled: (enabled: boolean) => invoke<void>("notch_set_enabled", { enabled }),
  settings: () => invoke<{ car_autoplay: boolean; chart_country: string }>("rpc", { method: "settings", args: {} }),
  setSetting: (key: string, value: string) => invoke<null>("rpc", { method: "setSetting", args: { key, value } }),
};
