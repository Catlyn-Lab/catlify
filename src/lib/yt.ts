import { rpc, type QueueItem } from "./api";
import { ytAvailable } from "./platform";

/** Queue paths the engine fetches from YouTube (see audio/engine.rs "remote sources"). */
export const isYouTubePath = (path: string | null | undefined) => !!path && (path.startsWith("yt:") || path.startsWith("ytq:"));

/** Video id from a youtube.com / music.youtube.com / youtu.be link. */
export function ytId(url: string | null | undefined): string | null {
  return url?.match(/[?&]v=([\w-]{11})/)?.[1] ?? url?.match(/youtu\.be\/([\w-]{11})/)?.[1] ?? null;
}

/**
 * A full song streamed from YouTube: an exact video (`videoId`) or the best YouTube Music match
 * for "artist title". Falls back to `preview` (30 s) if YouTube can't be reached.
 */
export function ytQueueItem(o: {
  videoId?: string | null;
  title: string;
  artist: string;
  album?: string | null;
  cover?: string | null;
  duration?: number | null;
  preview?: string | null;
}): QueueItem {
  const source = o.videoId ? `yt:${o.videoId}` : `ytq:${o.artist} ${o.title}`;
  return {
    id: null,
    path: o.preview ? `${source}#${o.preview}` : source,
    title: o.title,
    artist: o.artist,
    album: o.album ?? "",
    duration: o.duration ?? 0,
    cover: o.cover ?? null,
  };
}

/**
 * Look up YouTube links early for songs the user is likely to play (top results, rows on screen,
 * hovered/touched rows). Only the link is fetched, not the audio, so it barely uses data; a tapped
 * song then starts in about a second.
 */
const prepared = new Set<string>();
export function prepare(paths: (string | null | undefined)[]) {
  const specs = paths.filter((p): p is string => isYouTubePath(p)).map((p) => p.split("#")[0]).filter((s) => !prepared.has(s));
  if (!specs.length) return;
  specs.forEach((s) => prepared.add(s));
  rpc("ytPrepare", { specs }).catch(() => specs.forEach((s) => prepared.delete(s)));
}

/** Full songs need yt-dlp (desktop, Android); iOS keeps 30 s previews. */
export const canStreamYouTube = ytAvailable;
