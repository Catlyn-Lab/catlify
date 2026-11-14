// Imports playlists from Spotify / Apple Music / YouTube Music: skips songs already in the library,
// downloads the top YouTube result when it matches well enough (>= MATCH_THRESHOLD), and asks the
// user to choose for the rest. Progress is saved, so a long import continues after a restart.

import { api, type ImportCollection, type ImportTrack, type YtVideo } from "./api";
import { downloads } from "./downloads.svelte";
import { clean, matchScore, MATCH_THRESHOLD } from "./similarity";
import { store } from "./store.svelte";

export type ItemStatus = "pending" | "searching" | "downloading" | "done" | "in-library" | "review" | "skipped" | "failed";

export interface Candidate extends YtVideo {
  score: number;
}

export interface ImportItem extends ImportTrack {
  key: string;
  status: ItemStatus;
  score?: number;
  candidates?: Candidate[];
  path?: string | null;
  trackId?: number | null;
  error?: string | null;
  inPlaylist?: boolean;
}

export interface ImportJob {
  id: string;
  name: string;
  source: string;
  items: ImportItem[];
  makePlaylist: boolean;
  playlistId: number | null;
  paused: boolean;
  truncated: boolean;
  createdAt: number;
}

const STORAGE = "catlify.imports";
const WORKERS = 2;

function load(): ImportJob[] {
  try {
    // "nadacat.imports": saved before the rename to Catlify.
    const jobs: ImportJob[] = JSON.parse(localStorage.getItem(STORAGE) ?? localStorage.getItem("nadacat.imports") ?? "[]");
    // Anything interrupted by a restart starts over.
    for (const j of jobs) for (const i of j.items) if (i.status === "searching" || i.status === "downloading") i.status = "pending";
    return jobs;
  } catch {
    return [];
  }
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

class Importer {
  jobs = $state<ImportJob[]>(load());
  private active = 0;
  private finishing = new Set<string>();

  save() {
    localStorage.setItem(STORAGE, JSON.stringify($state.snapshot(this.jobs)));
  }

  counts(job: ImportJob) {
    const c = { total: job.items.length, done: 0, library: 0, review: 0, failed: 0, skipped: 0, working: 0, pending: 0 };
    for (const i of job.items) {
      if (i.status === "done") c.done++;
      else if (i.status === "in-library") c.library++;
      else if (i.status === "review") c.review++;
      else if (i.status === "failed") c.failed++;
      else if (i.status === "skipped") c.skipped++;
      else if (i.status === "pending") c.pending++;
      else c.working++;
    }
    return c;
  }

  add(c: ImportCollection, makePlaylist = true) {
    const items: ImportItem[] = c.tracks.map((t, n) => ({ ...t, key: `${n}`, status: "pending" }));
    this.jobs.unshift({
      id: `${Date.now()}`,
      name: c.name,
      source: c.source,
      items,
      makePlaylist,
      playlistId: null,
      paused: false,
      truncated: c.truncated,
      createdAt: Date.now(),
    });
    this.save();
    this.kick();
  }

  remove(job: ImportJob) {
    this.jobs = this.jobs.filter((j) => j.id !== job.id);
    this.save();
  }

  setPaused(job: ImportJob, paused: boolean) {
    job.paused = paused;
    this.save();
    if (!paused) this.kick();
  }

  skip(job: ImportJob, item: ImportItem) {
    item.status = "skipped";
    this.save();
    this.finishIfDone(job);
  }

  /** Manual choice for a song that didn't match well enough. */
  async choose(job: ImportJob, item: ImportItem, video: YtVideo) {
    await this.download(job, item, video);
    this.finishIfDone(job);
  }

  /** Starts workers for any pending songs. */
  kick() {
    while (this.active < WORKERS) {
      const next = this.next();
      if (!next) break;
      this.active++;
      const [job, item] = next;
      item.status = "searching";
      this.process(job, item)
        .catch((e) => {
          item.status = "failed";
          item.error = String(e);
        })
        .finally(() => {
          this.active--;
          this.save();
          this.finishIfDone(job);
          this.kick();
        });
    }
  }

  private next(): [ImportJob, ImportItem] | null {
    for (const j of this.jobs) {
      if (j.paused) continue;
      const i = j.items.find((x) => x.status === "pending");
      if (i) return [j, i];
    }
    return null;
  }

  private findLocal(item: ImportItem) {
    const t = clean(item.title);
    const a = clean(item.artist);
    if (!t) return undefined;
    return store.tracks.find((x) => {
      if (clean(x.title) !== t) return false;
      const xa = clean(x.artist);
      return !a || xa.includes(a) || a.includes(xa);
    });
  }

  private async process(job: ImportJob, item: ImportItem) {
    const local = this.findLocal(item);
    if (local) {
      item.status = "in-library";
      item.trackId = local.id;
      return;
    }
    // Known video (YouTube Music playlists, Takeout): download that exact one.
    if (item.videoId) {
      let video: YtVideo | undefined;
      if (!item.title) {
        video = (await api.ytLookup(`https://www.youtube.com/watch?v=${item.videoId}`))[0];
        if (video) Object.assign(item, { title: video.title, artist: video.artist });
      }
      video ??= {
        id: item.videoId,
        url: `https://www.youtube.com/watch?v=${item.videoId}`,
        title: item.title,
        artist: item.artist,
        videoTitle: item.title,
        channel: item.artist,
        duration: item.duration,
        thumbnail: `https://i.ytimg.com/vi/${item.videoId}/hqdefault.jpg`,
      };
      return this.download(job, item, video);
    }
    const results = await api.ytLookup(`${item.artist} ${item.title}`.trim());
    const candidates: Candidate[] = results.slice(0, 6).map((v) => ({ ...v, score: matchScore(item, v) }));
    item.candidates = candidates;
    const top = candidates[0];
    item.score = top?.score ?? 0;
    // Always the top result, and only when it clearly is the same song.
    if (top && top.score >= MATCH_THRESHOLD) return this.download(job, item, top);
    item.status = "review";
  }

  private async download(job: ImportJob, item: ImportItem, video: YtVideo) {
    item.status = "downloading";
    item.videoId = video.id;
    item.error = null;
    const format = (localStorage.getItem("stafy.ytFormat") as "mp3" | "original") ?? "mp3";
    // Tag with the playlist's clean title/artist, not the video's.
    await downloads.start(video, item.title || video.title, item.artist || video.artist, format);
    const p = await downloads.wait(video.id);
    if (p.stage === "done") {
      item.status = "done";
      item.path = p.path;
    } else {
      item.status = "failed";
      item.error = p.error ?? p.stage;
    }
    this.save();
  }

  /** Once nothing is left to do, put the songs into a playlist named like the source. */
  private async finishIfDone(job: ImportJob) {
    if (!job.makePlaylist || job.items.some((i) => i.status === "pending" || i.status === "searching" || i.status === "downloading")) return;
    if (this.finishing.has(job.id)) return;
    this.finishing.add(job.id);
    try {
      await this.fillPlaylist(job);
    } finally {
      this.finishing.delete(job.id);
    }
  }

  private async fillPlaylist(job: ImportJob) {
    const want = job.items.filter((i) => (i.status === "done" || i.status === "in-library") && !i.inPlaylist);
    if (!want.length) return;
    // Downloads are scanned in the background; wait until they're in the library.
    for (let attempt = 0; attempt < 12; attempt++) {
      await store.loadLibrary();
      if (want.every((i) => i.trackId || store.tracks.some((t) => t.path === i.path))) break;
      await sleep(1500);
    }
    const ids: number[] = [];
    for (const i of want) {
      const id = i.trackId ?? store.tracks.find((t) => t.path === i.path)?.id;
      if (id != null) {
        ids.push(id);
        i.inPlaylist = true;
      }
    }
    if (!ids.length) return;
    if (job.playlistId == null || !store.playlists.some((p) => p.id === job.playlistId)) {
      job.playlistId = await api.createPlaylist(job.name);
    }
    await api.addToPlaylist(job.playlistId, ids);
    await store.refreshPlaylists();
    this.save();
    const c = this.counts(job);
    store.notify(c.review ? `“${job.name}”: ${c.review} song${c.review === 1 ? "" : "s"} need your choice` : `Imported “${job.name}”`);
  }
}

export const importer = new Importer();
