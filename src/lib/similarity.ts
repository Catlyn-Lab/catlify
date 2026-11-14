// How closely a YouTube result matches a song from an imported playlist (0–100).
// Imports download the top result automatically only when the score is >= MATCH_THRESHOLD.

export const MATCH_THRESHOLD = 90;

const NOISE =
  /\b(official|video|audio|lyrics?|lyric video|visuali[sz]er|music video|mv|m\/v|hd|4k|hq|remaster(ed)?( \d{4})?|explicit|clean|topic|vevo)\b/g;

/** Lowercase, no accents/brackets/featured artists/punctuation. */
export function clean(s: string): string {
  return s
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[([{][^)\]}]*(feat|ft\.|with|official|video|audio|lyric|visuali|remaster|explicit|mv)[^)\]}]*[)\]}]/g, " ")
    .replace(/\s(feat\.?|ft\.?|featuring)\s.*$/, " ")
    .replace(/\s[|/]{1,2}\s.*$/, " ")
    .replace(NOISE, " ")
    .replace(/&/g, " and ")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim()
    .replace(/\s+/g, " ");
}

function levenshtein(a: string, b: string): number {
  if (a === b) return 0;
  let prev = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    const cur = [i];
    for (let j = 1; j <= b.length; j++) {
      cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    }
    prev = cur;
  }
  return prev[b.length];
}

const ratio = (a: string, b: string) => (a || b ? 1 - levenshtein(a, b) / Math.max(a.length, b.length) : 1);
const sorted = (s: string) => s.split(" ").filter(Boolean).sort().join(" ");

/** Word-order-insensitive similarity; also forgives spacing ("TateMcRae" vs "Tate McRae"). */
function similar(a: string, b: string): number {
  const sa = sorted(a);
  const sb = sorted(b);
  return Math.max(ratio(sa, sb), ratio(sa.replace(/ /g, ""), sb.replace(/ /g, "")));
}

export interface Song {
  title: string;
  artist: string;
  duration?: number | null;
}

/**
 * Score a candidate against the wanted song. Title and artist are compared both as one string
 * (channels often put the artist in the title) and separately, taking the better.
 */
export function matchScore(want: Song, got: Song & { videoTitle?: string }): number {
  const wt = clean(want.title);
  const wa = clean(want.artist);
  const gt = clean(got.title);
  // "Shakira, Burna Boy" matches "Shakira": compare against each credited artist too.
  const gotArtists = [got.artist, ...got.artist.split(/,|&| x | and | feat\.? | ft\.? /i)].map(clean).filter(Boolean);
  const ga = gotArtists.reduce((best, a) => (similar(wa, a) > similar(wa, best) ? a : best), clean(got.artist));
  const combined = similar(`${wa} ${wt}`, `${ga} ${gt}`);
  const separate = 0.65 * similar(wt, gt) + 0.35 * (wa ? similar(wa, ga) : 1);
  // The full video title often has both ("Artist - Song (Official Video)").
  const raw = got.videoTitle ? similar(`${wa} ${wt}`, clean(got.videoTitle)) : 0;
  let score = Math.max(combined, separate, raw) * 100;
  // A very different length usually means a live/extended/sped-up version.
  if (want.duration && got.duration && Math.abs(want.duration - got.duration) > 25) score -= 15;
  return Math.max(0, Math.min(100, Math.round(score)));
}
