<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ytAvailable } from "../lib/platform";
  import { canStreamYouTube, prepare, ytId, ytQueueItem } from "../lib/yt";

  const streamFull = canStreamYouTube();
  import { api, type Categories, type Chart, type ChartEntry, type QueueItem, type Track } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { downloads } from "../lib/downloads.svelte";
  import { fmtTime, norm, toQueueItem } from "../lib/util";
  import Cover from "../components/Cover.svelte";
  import Icon from "../components/Icon.svelte";
  import Spinner from "../components/Spinner.svelte";

  const COUNTRIES: [string, string][] = [
    ["us", "United States"], ["gb", "United Kingdom"], ["id", "Indonesia"], ["jp", "Japan"], ["kr", "South Korea"],
    ["de", "Germany"], ["fr", "France"], ["br", "Brazil"], ["mx", "Mexico"], ["in", "India"], ["au", "Australia"],
    ["ca", "Canada"], ["es", "Spain"], ["it", "Italy"], ["nl", "Netherlands"], ["se", "Sweden"], ["ph", "Philippines"],
    ["sg", "Singapore"], ["my", "Malaysia"], ["th", "Thailand"], ["vn", "Vietnam"], ["tw", "Taiwan"], ["za", "South Africa"],
  ];
  const SOURCES = [
    { id: "apple-songs", label: "Top Songs" },
    { id: "apple-albums", label: "Top Albums" },
    { id: "youtube-songs", label: "YouTube Music" },
    { id: "deezer", label: "Global · Deezer" },
    { id: "categories", label: "Categories" },
  ];

  type Category = { type: "genre" | "mood"; id: string; name: string };
  let categories = $state<Categories | null>(null);
  api.categories().then((c) => (categories = c)).catch(() => {});
  let category = $state<Category | null>(JSON.parse(localStorage.getItem("catlify.category") ?? "null"));
  /** Spotify-style tile colours, stable per category. */
  const TILE_COLORS = ["#e13300", "#1e3264", "#8d67ab", "#e8115b", "#148a08", "#bc5900", "#509bf5", "#27856a", "#af2896", "#477d95", "#ba5d07", "#8c1932"];
  const tileColor = (i: number) => TILE_COLORS[i % TILE_COLORS.length];
  function pick(c: Category | null) {
    category = c;
    localStorage.setItem("catlify.category", JSON.stringify(c));
  }

  const region = navigator.language.split("-")[1]?.toLowerCase();
  let country = $state(localStorage.getItem("stafy.country") ?? (COUNTRIES.some(([c]) => c === region) ? region! : "us"));
  let source = $state(localStorage.getItem("stafy.chartSource") ?? "apple-songs");
  let chart = $state<Chart | null>(null);
  const browsing = $derived(source === "categories" && !category);
  let loading = $state(false);
  let error = $state<string | null>(null);

  // YouTube Music also has a worldwide chart; Apple doesn't.
  const countries = $derived(source.startsWith("youtube") ? [["global", "Global"] as [string, string], ...COUNTRIES] : COUNTRIES);
  $effect(() => {
    if (country === "global" && !source.startsWith("youtube")) country = "us";
  });

  /** ▲3 / ▼2 / NEW, for sources that report last week's position. */
  function movement(e: ChartEntry): { cls: string; text: string } | null {
    if (!source.startsWith("youtube")) return null;
    if (e.previousRank == null) return { cls: "new", text: "NEW" };
    const d = e.previousRank - e.rank;
    return d > 0 ? { cls: "up", text: `▲${d}` } : d < 0 ? { cls: "down", text: `▼${-d}` } : { cls: "same", text: "–" };
  }

  const cache = new Map<string, { at: number; chart: Chart }>();

  let latest = 0;
  async function load() {
    const key = `${source}:${country}:${source === "categories" ? `${category?.type}:${category?.id}` : ""}`;
    const req = ++latest; // only the newest request may update the page
    localStorage.setItem("stafy.country", country);
    if (source.startsWith("apple")) api.setSetting("chart_country", country).catch(() => {}); // car Billboard tab
    localStorage.setItem("stafy.chartSource", source);
    error = null;
    if (browsing) {
      chart = null;
      loading = false;
      return;
    }
    const hit = cache.get(key);
    if (hit && Date.now() - hit.at < 30 * 60_000) {
      chart = hit.chart;
      loading = false;
      return;
    }
    loading = true;
    const [src, kind] =
      source === "deezer" ? ["deezer", "songs"] : source === "categories" ? [category!.type, category!.id] : source.split("-");
    try {
      let c: Chart;
      try {
        c = await api.chart(src, country, kind);
      } catch {
        c = await api.chart(src, country, kind); // one retry: phone networks drop requests
      }
      cache.set(key, { at: Date.now(), chart: c });
      if (req === latest) chart = c;
    } catch (e) {
      if (req === latest) error = String(e);
    } finally {
      if (req === latest) loading = false;
    }
  }

  $effect(() => {
    source;
    country;
    category;
    load();
  });

  const isAlbums = $derived(source === "apple-albums");

  // Get the top songs ready to play the moment they're tapped.
  $effect(() => {
    if (!chart || isAlbums || !streamFull) return;
    prepare(chart.entries.slice(0, 5).map((e) => asItem(e)?.path));
  });

  // Rows scrolled into view get ready too (looking up a YouTube link takes seconds, longest on phones).
  function visible(node: HTMLElement, e: ChartEntry) {
    const io = new IntersectionObserver(([entry]) => entry.isIntersecting && warm(e), { rootMargin: "100px" });
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }
  const warm = (e: ChartEntry) => !isAlbums && streamFull && prepare([asItem(e)?.path]);
  const songIndex = $derived(new Map(store.tracks.map((t) => [norm(t.artist) + "|" + norm(t.title), t])));
  const albumIndex = $derived(new Map(store.albums.map((a) => [norm(a.artist) + "|" + norm(a.title), a])));

  function localTrack(e: ChartEntry): Track | undefined {
    return songIndex.get(norm(e.artist) + "|" + norm(e.title));
  }

  function asItem(e: ChartEntry): QueueItem | null {
    const local = localTrack(e);
    if (local) return toQueueItem(local);
    // Full songs from YouTube where yt-dlp is available (desktop, Android); 30 s previews otherwise.
    if (streamFull) {
      return ytQueueItem({
        videoId: ytId(e.videoUrl),
        title: e.title,
        artist: e.artist,
        album: e.album,
        cover: e.artwork,
        duration: e.duration,
        preview: e.preview,
      });
    }
    if (!e.preview) return null;
    return { id: null, path: e.preview, title: e.title, artist: e.artist, album: e.album ?? "", duration: e.duration ?? 30, cover: e.artwork };
  }

  function playFrom(entry: ChartEntry) {
    if (isAlbums) {
      const a = albumIndex.get(norm(entry.artist) + "|" + norm(entry.title));
      if (a) store.go({ name: "album", key: a.key });
      else if (entry.url) openUrl(entry.url);
      return;
    }
    const entries = chart?.entries ?? [];
    const items: QueueItem[] = [];
    let start = 0;
    for (const e of entries) {
      const it = asItem(e);
      if (!it) continue;
      if (e === entry) start = items.length;
      items.push(it);
    }
    if (!asItem(entry)) {
      // YouTube Music has no previews: offer the download for that exact video instead.
      if (entry.videoUrl && ytAvailable()) store.go({ name: "youtube", query: entry.videoUrl });
      else store.notify("No preview available for this song");
      return;
    }
    store.playItems(items, start);
  }

  const inLibrary = $derived(
    chart && !isAlbums ? chart.entries.filter((e) => localTrack(e)).length : 0,
  );
  const top = $derived(chart?.entries.slice(0, 3) ?? []);
  const rest = $derived(chart?.entries.slice(3) ?? []);
  const nowPath = $derived(store.player.track?.path);
  /** The chosen song is still being fetched from YouTube (a few seconds). */
  const buffering = $derived(!!store.player.loading);
</script>

<div class="page fade-in">
  <div class="masthead">
    <div>
      <div class="eyebrow">The Catlify</div>
      <h1 class="page-title">Billboard</h1>
    </div>
    <div class="filters">
      {#each SOURCES as s}
        <button class="chip" class:active={source === s.id} onclick={() => (source = s.id)}>{s.label}</button>
      {/each}
      {#if source !== "deezer" && !(source === "categories" && category?.type === "mood")}
        <select bind:value={country} aria-label="Country">
          {#each countries as [code, name]}<option value={code}>{name}</option>{/each}
        </select>
      {/if}
    </div>
  </div>

  {#if source === "categories"}
    {#if browsing}
      {#if categories}
        <h2 class="section-title cat-head">Genres</h2>
        <div class="tiles">
          {#each categories.genres as g, i (g.id)}
            <button class="tile" style:background={tileColor(i)} onclick={() => pick({ type: "genre", id: g.id, name: g.name })}>{g.name}</button>
          {/each}
        </div>
        <h2 class="section-title cat-head">Moods &amp; activities</h2>
        <div class="tiles">
          {#each categories.moods as m, i (m.id)}
            <button class="tile" style:background={tileColor(i + 5)} onclick={() => pick({ type: "mood", id: m.id, name: m.name })}>{m.name}</button>
          {/each}
        </div>
      {:else}
        <div class="empty"><p>Loading categories…</p></div>
      {/if}
    {:else if category}
      <div class="crumb">
        <button class="chip" onclick={() => pick(null)}><Icon name="back" size={14} />All categories</button>
        <h2 class="serif">{category.name}</h2>
      </div>
    {/if}
  {/if}

  {#if chart && !browsing}
    <div class="sub">
      <span>{chart.title}</span>
      <span class="dot">·</span>
      <span>{chart.source}</span>
      {#if chart.updated}<span class="dot">·</span><span>Updated {new Date(chart.updated).toLocaleDateString()}</span>{/if}
      {#if !isAlbums}
        {@const first = chart.entries.find((e) => asItem(e))}
        <span class="dot">·</span>
        <span>{inLibrary} in your library</span>
        {#if first}
        <button class="btn primary play-all" onclick={() => playFrom(first)}>
          <Icon name="play" size={14} />Play chart
        </button>
        {/if}
      {/if}
    </div>
  {/if}

  {#if error}
    <div class="empty">
      <h3>Can't load the chart</h3>
      <p>{error}</p>
      <button class="btn" onclick={load}><Icon name="refresh" size={15} />Retry</button>
    </div>
  {:else if loading && !chart && !browsing}
    <div class="empty"><p>Loading the chart…</p></div>
  {:else if chart && !browsing}
    <div class="podium" class:loading>
      {#each top as e (e.rank + e.title)}
        {@const local = !isAlbums && localTrack(e)}
        {@const current = !!nowPath && nowPath === asItem(e)?.path}
        <button class="hero" class:playing={current} onclick={() => playFrom(e)} onpointerenter={() => warm(e)} ontouchstart={() => warm(e)}>
          <Cover src={e.artwork} radius="14px" />
          {#if current && buffering}<span class="hero-load"><Spinner size={30} color="#fff" /><span>Loading…</span></span>{/if}
          <span class="hero-rank serif">{e.rank}</span>
          <span class="hero-meta">
            <span class="ellipsis ht serif">{e.title}</span>
            <span class="ellipsis ha">{e.artist}</span>
            <span class="tags">
              {#if local}<span class="badge local">In library</span>{/if}
              {#if e.genre}<span class="badge">{e.genre}</span>{/if}
            </span>
          </span>
        </button>
      {/each}
    </div>

    <div class="rows" class:loading>
      {#each rest as e (e.rank + e.title)}
        {@const local = !isAlbums && localTrack(e)}
        {@const playable = isAlbums || !!local || !!e.preview || !!e.videoUrl}
        {@const mv = movement(e)}
        {@const current = !!nowPath && nowPath === asItem(e)?.path}
        <div
          class="row"
          use:visible={e}
          class:playing={current}
          role="button"
          tabindex="0"
          onpointerenter={() => warm(e)}
          ontouchstart={() => warm(e)}
          ondblclick={() => playFrom(e)}
          onclick={(ev) => store.mobile && !(ev.target as HTMLElement).closest("button") && playFrom(e)}
          onkeydown={(k) => k.key === "Enter" && playFrom(e)}
        >
          <span class="rank serif">{e.rank}{#if mv}<span class="mv {mv.cls}">{mv.text}</span>{/if}</span>
          <button class="art" onclick={() => playFrom(e)} disabled={!playable} aria-label="Play">
            <Cover src={e.artwork} size="48px" radius="6px" />
            {#if current && buffering}<span class="ov show"><Spinner size={18} color="#fff" /></span>
            {:else if playable}<span class="ov"><Icon name="play" size={16} /></span>{/if}
          </button>
          <span class="tt">
            <span class="ellipsis t">{e.title}{#if e.explicit}<span class="e">E</span>{/if}</span>
            <span class="ellipsis a" class:wait={current && buffering}>{current && buffering ? "Loading…" : e.artist}</span>
          </span>
          <span class="ellipsis album">{e.album ?? e.genre ?? ""}</span>
          <span class="flags">
            {#if local}<span class="badge local">In library</span>{:else if !isAlbums && streamFull}<span class="badge yt">YouTube</span>{:else if !isAlbums && e.preview}<span class="badge">Preview</span>{/if}
          </span>
          <span class="num dur">{e.duration ? fmtTime(e.duration) : ""}</span>
          {#if !isAlbums}
            <button class="vid" onclick={() => store.openVideo({ title: e.title, artist: e.artist })} aria-label="See video" title="See video">
              <Icon name="video" size={16} />
            </button>
          {:else}<span></span>{/if}
          {#if !isAlbums && !local && ytAvailable()}
            <button
              class="ext yt"
              onclick={() => {
                const it = asItem(e);
                if (it) downloads.downloadItem(it);
                else store.go({ name: "youtube", query: e.videoUrl ?? `${e.artist} - ${e.title}` });
              }}
              aria-label="Download to library"
              title="Download to library"
            >
              <Icon name="download" size={15} />
            </button>
          {:else if e.url}
            <button class="ext" onclick={() => openUrl(e.url!)} aria-label="Open in browser"><Icon name="external" size={15} /></button>
          {:else}<span></span>{/if}
        </div>
      {/each}
    </div>
    <p class="note">
      {#if streamFull}
        Songs in your library play from your own files in full quality. Everything else plays the full song from
        YouTube (it takes a few seconds to start). Tap the download icon to keep it.
      {:else}
        Songs in your library play from your own files in full quality. Everything else plays a 30-second preview.
      {/if}
    </p>
  {/if}
</div>

<style>
  .masthead {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 20px;
    flex-wrap: wrap;
    border-bottom: 3px double var(--line-strong);
    padding-bottom: 12px;
  }
  .masthead .page-title {
    font-size: 64px;
    margin-bottom: 0;
    font-style: italic;
    letter-spacing: -0.03em;
  }
  .filters {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
    padding-bottom: 8px;
  }
  select {
    height: 30px;
    padding: 0 10px;
    border-radius: 999px;
    background: var(--surface);
    border: 1px solid var(--line);
    font-size: 13px;
    outline: none;
  }
  .sub {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    color: var(--muted);
    font-size: 13px;
    padding: 14px 0 22px;
  }
  .dot {
    color: var(--dim);
  }
  .play-all {
    margin-left: auto;
  }
  .podium {
    display: grid;
    grid-template-columns: 1.25fr 1fr 1fr;
    gap: 20px;
    margin-bottom: 30px;
    align-items: end;
  }
  .hero {
    position: relative;
    text-align: left;
    border-radius: 14px;
    overflow: hidden;
    box-shadow: 0 20px 50px rgba(0, 0, 0, 0.45);
    transition: transform 0.2s;
  }
  .hero:hover {
    transform: translateY(-4px);
  }
  .hero::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(transparent 40%, rgba(0, 0, 0, 0.85));
    pointer-events: none;
  }
  .badge.yt {
    color: var(--coral);
  }
  .mv {
    display: block;
    font-family: var(--sans);
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.02em;
    margin-top: 2px;
  }
  .mv.up {
    color: #6fd39b;
  }
  .mv.down {
    color: var(--coral);
  }
  .mv.new {
    color: var(--gold);
  }
  .mv.same {
    color: var(--dim);
  }
  .hero-rank {
    position: absolute;
    top: 8px;
    left: 16px;
    z-index: 1;
    font-size: 84px;
    font-weight: 800;
    font-style: italic;
    line-height: 1;
    color: #fff;
    text-shadow: 0 6px 30px rgba(0, 0, 0, 0.5);
  }
  .hero-meta {
    position: absolute;
    left: 18px;
    right: 18px;
    bottom: 16px;
    z-index: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ht {
    font-size: 24px;
    font-weight: 700;
  }
  .ha {
    color: rgba(255, 255, 255, 0.75);
  }
  .tags {
    display: flex;
    gap: 6px;
    margin-top: 8px;
  }
  .tags .badge {
    background: rgba(0, 0, 0, 0.45);
  }
  .hero.playing {
    outline: 2px solid var(--coral);
  }
  .loading {
    opacity: 0.5;
    transition: opacity 0.2s;
  }
  .row {
    display: grid;
    grid-template-columns: 52px 48px minmax(180px, 2.4fr) minmax(100px, 1.6fr) 90px 50px 32px 32px;
    align-items: center;
    gap: 16px;
    padding: 8px 12px;
    border-radius: 10px;
    border-bottom: 1px solid var(--line);
  }
  .row:hover {
    background: var(--surface);
  }
  .row.playing .t {
    color: var(--coral);
  }
  .rank {
    font-size: 26px;
    font-weight: 700;
    font-style: italic;
    color: var(--dim);
    text-align: right;
  }
  .art {
    position: relative;
    border-radius: 6px;
    overflow: hidden;
  }
  .ov {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.5);
    opacity: 0;
    transition: opacity 0.12s;
  }
  .art:hover .ov,
  .ov.show {
    opacity: 1;
  }
  .a.wait {
    color: var(--coral);
  }
  .hero-load {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    border-radius: 14px;
    background: rgba(0, 0, 0, 0.5);
    color: #fff;
    font-weight: 600;
    font-size: 13px;
  }
  .tt {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .t {
    font-weight: 600;
    font-size: 15px;
  }
  .e {
    display: inline-grid;
    place-items: center;
    width: 15px;
    height: 15px;
    margin-left: 6px;
    border-radius: 3px;
    background: rgba(255, 255, 255, 0.18);
    font-size: 9px;
    font-weight: 800;
    vertical-align: 2px;
  }
  .a,
  .album {
    color: var(--muted);
    font-size: 13px;
  }
  .dur {
    color: var(--dim);
    text-align: right;
  }
  .ext {
    color: var(--dim);
    display: grid;
    place-items: center;
  }
  .ext:hover {
    color: var(--text);
  }
  .vid {
    color: var(--dim);
    display: grid;
    place-items: center;
    height: 32px;
  }
  .vid:hover {
    color: var(--text);
  }
  :global(html.m) .row .vid {
    height: 44px;
  }
  .cat-head {
    margin: 22px 0 12px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: 14px;
  }
  .tile {
    position: relative;
    aspect-ratio: 16 / 10;
    border-radius: 10px;
    padding: 14px;
    color: #fff;
    font-size: 19px;
    font-weight: 800;
    text-align: left;
    align-content: start;
    display: grid;
    overflow: hidden;
    letter-spacing: -0.01em;
    transition: transform 0.12s, filter 0.12s;
  }
  .tile::after {
    /* a tilted "record" in the corner, like a cover peeking out */
    content: "";
    position: absolute;
    right: -18px;
    bottom: -18px;
    width: 76px;
    height: 76px;
    border-radius: 50%;
    background: radial-gradient(circle, rgba(255, 255, 255, 0.35) 0 10%, rgba(0, 0, 0, 0.35) 11% 100%);
    transform: rotate(25deg);
  }
  .tile:hover {
    filter: brightness(1.1);
    transform: translateY(-2px);
  }
  :global(html.m) .tiles {
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  :global(html.m) .tile {
    font-size: 16px;
  }
  .crumb {
    display: flex;
    align-items: center;
    gap: 14px;
    padding-top: 16px;
  }
  .crumb h2 {
    font-size: 28px;
    margin: 0;
  }
  .crumb .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .note {
    color: var(--dim);
    font-size: 12px;
    margin-top: 24px;
  }
  :global(html.m) .podium {
      gap: 10px;
  }
  :global(html.m) .row .yt {
    width: 40px;
    height: 44px;
    color: var(--coral);
  }
  :global(html.m) .row {
      grid-template-columns: 30px 48px minmax(0, 1fr) 40px 40px;
      gap: 10px;
      padding: 8px 2px;
  }
  :global(html.m) .row .album, :global(html.m) .row .ext:not(.yt), :global(html.m) .row .flags, :global(html.m) .row .dur {
      display: none;
  }
  :global(html.m) .rank {
      font-size: 20px;
  }
</style>
