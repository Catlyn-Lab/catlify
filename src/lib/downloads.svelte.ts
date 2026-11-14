import { listen } from "@tauri-apps/api/event";
import { api, rpc, ytBridge, type QueueItem, type YtProgress, type YtVideo } from "./api";
import { isYouTubePath } from "./yt";
import { store } from "./store.svelte";

export interface Download {
  video: YtVideo;
  title: string;
  artist: string;
  format: "mp3" | "original";
  progress: YtProgress;
}

/** YouTube downloads, kept outside the view so progress survives navigation. */
class Downloads {
  items = $state<Record<string, Download>>({});

  constructor() {
    const onProgress = (p: YtProgress) => {
      const d = this.items[p.id];
      if (!d) return;
      d.progress = p;
      if (p.stage === "done") {
        store.notify(`Saved “${d.title}” to your library`);
        // Android saves via MediaStore; rescan so the song shows up (desktop already rescans in Rust).
        if (ytBridge()) api.rescan();
      }
      if (p.stage === "error") store.notify(`Download failed: ${p.error}`);
    };
    listen<YtProgress>("youtube://progress", (e) => onProgress(e.payload)).catch(() => {});
    // Android bridge (StafyYouTube.kt)
    addEventListener("stafy-yt-progress", (e) => onProgress((e as CustomEvent<YtProgress>).detail));
  }

  get active() {
    return Object.values(this.items).filter((d) => d.progress.stage === "downloading" || d.progress.stage === "converting").length;
  }

  async start(video: YtVideo, title: string, artist: string, format: "mp3" | "original") {
    this.items[video.id] = { video, title, artist, format, progress: { id: video.id, stage: "downloading", percent: 0, path: null, error: null } };
    try {
      await api.ytDownload({ url: video.url, id: video.id, title, artist }, format);
    } catch (e) {
      this.items[video.id].progress = { id: video.id, stage: "error", percent: 0, path: null, error: String(e) };
    }
  }

  /** Resolves when the download finishes (done, error or cancelled). */
  wait(id: string): Promise<YtProgress> {
    return new Promise((resolve) => {
      const check = () => {
        const p = this.items[id]?.progress;
        if (p && (p.stage === "done" || p.stage === "error" || p.stage === "cancelled")) resolve($state.snapshot(p) as YtProgress);
        else setTimeout(check, 500);
      };
      check();
    });
  }

  /** Save the song that's streaming from YouTube into the library. */
  downloadCurrent() {
    const t = store.player.track;
    if (t) this.downloadItem(t);
  }

  /**
   * Saves a song from a chart, playlist or queue straight to the library: the exact YouTube
   * video it plays from (looked up first if it hasn't played yet). No search page.
   */
  async downloadItem(t: QueueItem) {
    if (!isYouTubePath(t.path)) {
      store.notify(t.id !== null ? "Already in your library" : "This song can't be downloaded");
      return;
    }
    store.notify(`Downloading “${t.title}”…`);
    let id: string;
    try {
      id = await rpc<string>("ytVideoId", { path: t.path });
    } catch (e) {
      store.notify(`Couldn't find “${t.title}” on YouTube: ${e}`);
      return;
    }
    const stage = this.items[id]?.progress.stage;
    if (stage === "downloading" || stage === "converting") return;
    const format = await this.format();
    const video: YtVideo = {
      id,
      url: `https://www.youtube.com/watch?v=${id}`,
      title: t.title,
      artist: t.artist,
      videoTitle: t.title,
      channel: t.artist,
      duration: t.duration || null,
      thumbnail: t.cover ?? `https://i.ytimg.com/vi/${id}/hqdefault.jpg`,
    };
    this.start(video, t.title, t.artist, format);
  }

  /** The chosen format, or M4A where MP3 isn't possible (no ffmpeg: iPhone, Mac without Homebrew). */
  private canMp3?: Promise<boolean>;
  private async format(): Promise<"mp3" | "original"> {
    const chosen = (localStorage.getItem("stafy.ytFormat") as "mp3" | "original") ?? "mp3";
    if (chosen === "original") return chosen;
    this.canMp3 ??= api.ytStatus().then((s) => s.ffmpeg).catch(() => true);
    return (await this.canMp3) ? "mp3" : "original";
  }

  cancel(id: string) {
    api.ytCancel(id);
  }
}

export const downloads = new Downloads();
