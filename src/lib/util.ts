import { convertFileSrc } from "@tauri-apps/api/core";
import type { QueueItem, StreamInfo, Track } from "./api";

export function fmtTime(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) secs = 0;
  const s = Math.floor(secs);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

export function fmtDuration(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.round((secs % 3600) / 60);
  return h ? `${h} hr ${m} min` : `${m} min`;
}

export function coverSrc(cover: string | null | undefined): string | null {
  if (!cover) return null;
  return cover.startsWith("http") ? cover : convertFileSrc(cover);
}

export function toQueueItem(t: Track): QueueItem {
  return { id: t.id, path: t.path, title: t.title, artist: t.artist, album: t.album, duration: t.duration, cover: t.cover };
}

/** "FLAC · 24/96" or "DSD64" or "MP3" */
export function qualityLabel(codec: string, bitDepth: number | null | undefined, rate: number | null | undefined): string {
  if (codec === "DSD" || codec.startsWith("dsd")) {
    const r = rate ?? 0;
    const mult = Math.round(r / 44100);
    return mult >= 64 ? `DSD${mult}` : "DSD";
  }
  if (!bitDepth || !rate || bitDepth > 32) return codec.toUpperCase();
  const khz = rate % 1000 === 0 ? String(rate / 1000) : (rate / 1000).toFixed(1);
  return `${codec.toUpperCase()} · ${bitDepth}/${khz}`;
}

export function streamLabel(s: StreamInfo | null): string {
  if (!s) return "";
  const codec = s.codec.startsWith("pcm_") ? "PCM" : s.codec.startsWith("dsd_") ? "DSD" : s.codec.toUpperCase();
  const rate = s.codec.startsWith("dsd_") ? s.sampleRate * 8 : s.sampleRate;
  return qualityLabel(codec, s.lossless ? s.bitDepth : null, rate);
}

export function isHiRes(bitDepth: number | null | undefined, rate: number | null | undefined, lossless: boolean): boolean {
  return lossless && ((bitDepth ?? 0) > 16 || (rate ?? 0) > 48000);
}

/** Normalises titles/artists so chart entries can be matched against the local library. */
export function norm(s: string): string {
  return s
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/\s*[([].*?(feat|ft\.|with|remaster|version|edit|live).*?[)\]]/g, "")
    .replace(/&/g, "and")
    .replace(/[^a-z0-9]+/g, "");
}

export interface LyricLine {
  time: number;
  text: string;
}

export function parseLrc(lrc: string): LyricLine[] {
  let offset = 0;
  const lines: LyricLine[] = [];
  for (const raw of lrc.split(/\r?\n/)) {
    const off = raw.match(/^\[offset:\s*([+-]?\d+)\]/i);
    if (off) {
      offset = parseInt(off[1], 10) / 1000;
      continue;
    }
    const stamps = [...raw.matchAll(/\[(\d{1,3}):(\d{1,2}(?:[.:]\d{1,3})?)\]/g)];
    if (!stamps.length) continue;
    const text = raw
      .replace(/\[[^\]]*\]/g, "")
      .replace(/<\d{1,3}:\d{1,2}(?:[.:]\d{1,3})?>/g, "")
      .trim();
    for (const st of stamps) {
      const t = parseInt(st[1], 10) * 60 + parseFloat(st[2].replace(":", "."));
      lines.push({ time: Math.max(0, t - offset), text });
    }
  }
  return lines.sort((a, b) => a.time - b.time);
}

export function greeting(): string {
  const h = new Date().getHours();
  if (h < 5) return "Late night listening";
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}

export function albumKey(t: Pick<Track, "albumArtist" | "album">): string {
  return `${t.albumArtist}\u0001${t.album}`;
}
