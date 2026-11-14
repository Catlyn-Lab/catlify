import { listen } from "@tauri-apps/api/event";
import { api, rpc, type PlayerState, type SleepStatus, type Playlist, type QueueItem, type ScanProgress, type Track } from "./api";
import { mobileQuery, useMobileLayout } from "./platform";
import { albumKey, toQueueItem } from "./util";

export type View =
  | { name: "home" }
  | { name: "billboard" }
  | { name: "youtube"; query?: string }
  | { name: "songs" }
  | { name: "albums" }
  | { name: "album"; key: string }
  | { name: "artists" }
  | { name: "artist"; artist: string }
  | { name: "liked" }
  | { name: "playlist"; id: number }
  | { name: "search"; query: string }
  | { name: "settings" }
  | { name: "library" }
  | { name: "sync" }
  | { name: "import" };

export interface MenuItem {
  label: string;
  action?: () => void;
  heading?: boolean;
  danger?: boolean;
}

export interface Album {
  key: string;
  title: string;
  artist: string;
  year: number | null;
  cover: string | null;
  tracks: Track[];
  lossless: boolean;
  hiRes: boolean;
}

const emptyPlayer: PlayerState = {
  playing: false,
  position: 0,
  duration: 0,
  index: null,
  track: null,
  volume: Number(localStorage.getItem("stafy.volume") ?? 0.8),
  shuffle: false,
  repeat: "off",
  stream: null,
  error: null,
  queueVersion: 0,
};

class Store {
  player = $state<PlayerState>({ ...emptyPlayer });
  /** performance.now() when `player` was last received, for smooth interpolation. */
  receivedAt = $state(performance.now());
  now = $state(performance.now());

  tracks = $state<Track[]>([]);
  playlists = $state<Playlist[]>([]);
  folders = $state<string[]>([]);
  scan = $state<ScanProgress | null>(null);

  view = $state<View>({ name: "home" });
  history = $state<View[]>([]);
  panel = $state<"none" | "queue" | "lyrics">("none");
  nowPlayingOpen = $state(false);
  /** "See video" player: what to look up on YouTube (song search, video id or link). */
  video = $state<{ query: string; title: string; artist?: string } | null>(null);
  sleep = $state<SleepStatus>({ active: false, endsAt: null, endOfTrack: false });
  /** Phone layout (bottom tabs, mini player). */
  mobile = $state(useMobileLayout());
  toast = $state<string | null>(null);
  menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
  ask = $state<{ title: string; value: string; resolve: (v: string | null) => void } | null>(null);
  confirm = $state<{ title: string; message: string; action: string; resolve: (ok: boolean) => void } | null>(null);

  albums = $derived.by(() => {
    const map = new Map<string, Album>();
    for (const t of this.tracks) {
      const key = albumKey(t);
      let a = map.get(key);
      if (!a) {
        a = { key, title: t.album, artist: t.albumArtist, year: t.year, cover: t.cover, tracks: [], lossless: true, hiRes: false };
        map.set(key, a);
      }
      a.tracks.push(t);
      a.cover ??= t.cover;
      a.year ??= t.year;
      a.lossless &&= t.lossless;
      a.hiRes ||= t.lossless && ((t.bitDepth ?? 0) > 16 || (t.sampleRate ?? 0) > 48000);
    }
    return [...map.values()];
  });

  artists = $derived.by(() => {
    const map = new Map<string, { name: string; tracks: number; cover: string | null }>();
    for (const t of this.tracks) {
      const a = map.get(t.albumArtist) ?? { name: t.albumArtist, tracks: 0, cover: null };
      a.tracks++;
      a.cover ??= t.cover;
      map.set(t.albumArtist, a);
    }
    return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
  });

  /** Interpolated playback position for smooth progress bars and lyric sync. */
  get position(): number {
    const p = this.player;
    if (!p.playing) return p.position;
    const pos = p.position + (this.now - this.receivedAt) / 1000;
    return p.duration > 0 ? Math.min(pos, p.duration) : pos;
  }

  get currentTrack(): Track | undefined {
    const id = this.player.track?.id;
    return id == null ? undefined : this.tracks.find((t) => t.id === id);
  }

  async init() {
    mobileQuery.addEventListener("change", () => (this.mobile = useMobileLayout()));
    // System back (Android button, gestures): close overlays first, then go back a page.
    addEventListener("popstate", () => {
      if (!this.mobile) return;
      if (this.menu) this.menu = null;
      else if (this.panel !== "none") this.panel = "none";
      else if (this.nowPlayingOpen) this.nowPlayingOpen = false;
      else this.back();
    });
    await listen<PlayerState>("player://state", (e) => {
      // Play/pause can come from the sleep timer firing or a timer set in the car.
      if (e.payload.playing !== this.player.playing) this.refreshSleep();
      this.player = e.payload;
      this.receivedAt = performance.now();
    });
    await listen("player://track", () => this.refreshTracksSoon());
    await listen<ScanProgress>("library://scan", (e) => (this.scan = e.payload));
    await listen("library://changed", () => {
      this.loadLibrary();
      setTimeout(() => (this.scan = null), 2500);
    });

    const tick = () => {
      this.now = performance.now();
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);

    this.refreshSleep();
    api.setVolume(this.player.volume);
    this.player = { ...(await api.state()), volume: this.player.volume };
    await this.loadLibrary();

    if (import.meta.env.DEV) {
      // e.g. ?view=billboard&panel=lyrics&np=1, for UI work in a plain browser.
      const q = new URLSearchParams(location.search);
      const view = q.get("view");
      if (view === "album") this.view = { name: "album", key: this.albums[0]?.key ?? "" };
      else if (view) this.view = { name: view } as View;
      if (q.get("panel")) this.panel = q.get("panel") as "queue" | "lyrics";
      if (q.get("np")) this.nowPlayingOpen = true;
    }
  }

  async loadLibrary() {
    const [tracks, playlists, folders] = await Promise.all([api.tracks(), api.playlists(), api.folders()]);
    this.tracks = tracks;
    this.playlists = playlists;
    this.folders = folders;
  }

  private refreshTimer: ReturnType<typeof setTimeout> | undefined;
  /** Play counts change on every track change; refresh lazily. */
  refreshTracksSoon() {
    clearTimeout(this.refreshTimer);
    this.refreshTimer = setTimeout(async () => (this.tracks = await api.tracks()), 1500);
  }

  async refreshPlaylists() {
    this.playlists = await api.playlists();
  }

  go(view: View) {
    this.history.push($state.snapshot(this.view) as View);
    if (this.history.length > 50) this.history.shift();
    this.view = view;
    this.nowPlayingOpen = false;
    if (this.mobile) {
      this.panel = "none";
      history.pushState({ stafy: "view" }, ""); // Android back button / iOS edge swipe
    }
    document.querySelector("main")?.scrollTo({ top: 0 });
  }

  /** Tab bar: jump to a section and reset the back stack. */
  root(view: View) {
    this.history = [];
    this.view = view;
    this.nowPlayingOpen = false;
    this.panel = "none";
    document.querySelector("main")?.scrollTo({ top: 0 });
  }

  /** Full-screen player; on phones the system back button/gesture closes it. */
  openNowPlaying() {
    this.nowPlayingOpen = true;
    if (this.mobile) history.pushState({ stafy: "np" }, "");
  }

  /** Queue / lyrics: side panel on desktop, full-screen sheet on phones. */
  togglePanel(kind: "queue" | "lyrics") {
    if (this.panel === kind) {
      if (this.mobile) history.back();
      else this.panel = "none";
      return;
    }
    if (this.mobile && this.panel === "none") history.pushState({ stafy: "panel" }, "");
    this.panel = kind;
  }

  back() {
    const v = this.history.pop();
    if (v) this.view = v;
  }

  openMenu(e: MouseEvent, items: MenuItem[]) {
    e.preventDefault();
    e.stopPropagation();
    this.menu = { x: e.clientX, y: e.clientY, items };
  }

  // ---- "See video" ---------------------------------------------------------------------

  /** Plays the song's YouTube video (pauses the music). `videoId` when it's known exactly. */
  openVideo(o: { title: string; artist?: string; videoId?: string | null }) {
    const query = o.videoId ?? [o.artist, o.title].filter(Boolean).join(" ");
    this.video = { query, title: o.title, artist: o.artist };
  }

  // ---- sleep timer ---------------------------------------------------------------------

  /** "23 min", "End of song" or null; ticks with the animation clock. */
  get sleepLabel(): string | null {
    if (!this.sleep.active) return null;
    if (this.sleep.endOfTrack) return "End of song";
    void this.now;
    const left = Math.max(0, (this.sleep.endsAt ?? 0) - Date.now());
    return left >= 60_000 ? `${Math.ceil(left / 60_000)} min` : `${Math.ceil(left / 1000)} s`;
  }

  async refreshSleep() {
    this.sleep = await api.sleepStatus().catch(() => this.sleep);
  }

  /** The sleep timer menu (also used from the car: the engine owns the timer). */
  sleepMenu(e: MouseEvent) {
    const set = async (o: { minutes: number } | { endOfTrack: true }, label: string) => {
      this.sleep = await api.sleepSet(o);
      this.notify(`Sleep timer: ${label}`);
    };
    const items: MenuItem[] = [{ label: "Sleep timer", heading: true }];
    for (const m of [5, 15, 30, 45, 60, 90]) items.push({ label: `${m} minutes`, action: () => set({ minutes: m }, `${m} minutes`) });
    items.push({ label: "End of this song", action: () => set({ endOfTrack: true }, "end of this song") });
    if (this.sleep.active) {
      items.push({
        label: "Turn off timer",
        danger: true,
        action: async () => {
          this.sleep = await api.sleepCancel();
          this.notify("Sleep timer off");
        },
      });
    }
    this.openMenu(e, items);
  }

  /** Standard actions for one or more library tracks. */
  trackMenu(tracks: Track[], extra: MenuItem[] = []): MenuItem[] {
    const first = tracks[0];
    const items: MenuItem[] = [
      { label: "Play next", action: () => this.enqueue(tracks, true) },
      { label: "Add to queue", action: () => this.enqueue(tracks, false) },
    ];
    if (first && tracks.length === 1) {
      items.push({ label: first.liked ? "Remove from Liked" : "Add to Liked", action: () => this.toggleLike(first.id) });
      items.push({ label: "Go to album", action: () => this.go({ name: "album", key: albumKey(first) }) });
      items.push({ label: "Go to artist", action: () => this.go({ name: "artist", artist: first.albumArtist }) });
      items.push({ label: "See video", action: () => this.openVideo({ title: first.title, artist: first.artist }) });
    }
    items.push({ label: "Add to playlist", heading: true });
    items.push({
      label: "+ New playlist…",
      action: async () => {
        const name = await this.askText("New playlist");
        if (!name?.trim()) return;
        const id = await api.createPlaylist(name.trim());
        await api.addToPlaylist(id, tracks.map((t) => t.id));
        await this.refreshPlaylists();
        this.notify(`Added to ${name.trim()}`);
      },
    });
    for (const p of this.playlists) {
      items.push({
        label: p.name,
        action: async () => {
          await api.addToPlaylist(p.id, tracks.map((t) => t.id));
          await this.refreshPlaylists();
          this.notify(`Added to ${p.name}`);
        },
      });
    }
    const del: MenuItem = {
      label: tracks.length === 1 ? "Delete from device…" : `Delete ${tracks.length} songs from device…`,
      danger: true,
      action: () => this.deleteTracks(tracks),
    };
    return [...items, ...extra, del];
  }

  /** In-app replacement for window.prompt (not available in every webview). */
  /** A yes/no dialog for destructive actions. */
  confirmAction(title: string, message: string, action: string): Promise<boolean> {
    return new Promise((resolve) => (this.confirm = { title, message, action, resolve }));
  }

  /** Permanently deletes songs from this device: files and library entries. */
  async deleteTracks(tracks: Track[]) {
    const n = tracks.length;
    const what = n === 1 ? `“${tracks[0].title}”` : `${n} songs`;
    const ok = await this.confirmAction(
      `Delete ${what}?`,
      `${n === 1 ? "The file is" : "The files are"} permanently deleted from this device and removed from your library and playlists. This can't be undone.`,
      "Delete",
    );
    if (!ok) return;
    const ids = tracks.map((t) => t.id);
    const playing = this.player.track?.id;
    let r = await rpc<{ deleted: number[]; needsPermission: { id: number; path: string }[]; failed: string[] }>("deleteTracks", { ids });
    // Android: files from other apps need the system's own confirmation.
    const files = (window as unknown as { StafyFiles?: { requestDelete(id: string, paths: string): void } }).StafyFiles;
    if (r.needsPermission.length && files) {
      const allowed = await new Promise<boolean>((resolve) => {
        const callId = `del${Date.now()}`;
        (window as unknown as { __stafyDeleted: (id: string, ok: boolean) => void }).__stafyDeleted = (id, ok) => id === callId && resolve(ok);
        files.requestDelete(callId, JSON.stringify(r.needsPermission.map((b) => b.path)));
      });
      if (allowed) {
        const again = await rpc<typeof r>("deleteTracks", { ids: r.needsPermission.map((b) => b.id) });
        r = { deleted: [...r.deleted, ...again.deleted], needsPermission: again.needsPermission, failed: [...r.failed, ...again.failed] };
      }
    }
    if (playing != null && r.deleted.includes(playing)) api.control("next");
    await this.loadLibrary();
    const left = r.failed.length + r.needsPermission.length;
    this.notify(
      left
        ? `Deleted ${r.deleted.length} of ${n}. ${r.failed[0] ?? "Android didn't allow deleting the rest."}`
        : `Deleted ${n === 1 ? tracks[0].title : `${n} songs`}`,
    );
  }

  askText(title: string, value = ""): Promise<string | null> {
    return new Promise((resolve) => (this.ask = { title, value, resolve }));
  }

  notify(msg: string) {
    this.toast = msg;
    setTimeout(() => {
      if (this.toast === msg) this.toast = null;
    }, 2600);
  }

  // ---- playback helpers ----------------------------------------------------------------

  playTracks(tracks: Track[], start = 0) {
    if (!tracks.length) return;
    api.playQueue(tracks.map(toQueueItem), start);
  }

  playItems(items: QueueItem[], start = 0) {
    if (!items.length) return;
    api.playQueue(items, start);
  }

  shuffleTracks(tracks: Track[]) {
    if (!tracks.length) return;
    api.setShuffle(true).then(() => api.playQueue(tracks.map(toQueueItem), Math.floor(Math.random() * tracks.length)));
  }

  async enqueue(tracks: Track[] | QueueItem[], next: boolean) {
    const items = tracks.map((t) => ("addedAt" in t ? toQueueItem(t as Track) : (t as QueueItem)));
    await api.enqueue(items, next);
    this.notify(next ? `Playing next: ${items[0]?.title ?? ""}` : `Added ${items.length} to queue`);
  }

  async toggleLike(id: number) {
    const liked = await api.toggleLike(id);
    const t = this.tracks.find((t) => t.id === id);
    if (t) t.liked = liked;
  }

  setVolume(v: number) {
    v = Math.min(1, Math.max(0, v));
    this.player.volume = v;
    localStorage.setItem("stafy.volume", String(v));
    api.setVolume(v);
  }

  seek(pos: number) {
    this.player.position = pos;
    this.receivedAt = performance.now();
    api.seek(pos);
  }
}

export const store = new Store();
