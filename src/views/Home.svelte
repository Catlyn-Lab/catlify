<script lang="ts">
  import { api, type ChartEntry } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { fmtDuration, greeting, norm } from "../lib/util";
  import AlbumCard from "../components/AlbumCard.svelte";
  import Cover from "../components/Cover.svelte";
  import Icon from "../components/Icon.svelte";
  import TrackList from "../components/TrackList.svelte";

  const tracks = $derived(store.tracks);
  const totalSecs = $derived(tracks.reduce((s, t) => s + t.duration, 0));
  const losslessPct = $derived(tracks.length ? Math.round((tracks.filter((t) => t.lossless).length / tracks.length) * 100) : 0);

  const recentlyPlayed = $derived(
    tracks.filter((t) => t.lastPlayed).sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0)).slice(0, 6),
  );
  const mostPlayed = $derived(tracks.filter((t) => t.playCount > 0).sort((a, b) => b.playCount - a.playCount).slice(0, 10));
  const recentAlbums = $derived(
    [...store.albums].sort((a, b) => Math.max(...b.tracks.map((t) => t.addedAt)) - Math.max(...a.tracks.map((t) => t.addedAt))).slice(0, 12),
  );

  let trending = $state<ChartEntry[]>([]);
  const country = localStorage.getItem("stafy.country") ?? "us";
  api
    .chart("apple", country, "songs")
    .then((c) => (trending = c.entries.slice(0, 8)))
    .catch(() => {});

  const libIndex = $derived(new Map(tracks.map((t) => [norm(t.artist) + "|" + norm(t.title), t])));
</script>

<div class="page fade-in">
  <div class="eyebrow">{new Date().toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" })}</div>
  <h1 class="page-title">{greeting()}</h1>

  {#if tracks.length}
    <div class="stats">
      <div><b class="serif">{tracks.length.toLocaleString()}</b><span>songs</span></div>
      <div><b class="serif">{store.albums.length.toLocaleString()}</b><span>albums</span></div>
      <div><b class="serif">{store.artists.length.toLocaleString()}</b><span>artists</span></div>
      <div><b class="serif">{fmtDuration(totalSecs)}</b><span>of music</span></div>
      <div><b class="serif grad">{losslessPct}%</b><span>lossless</span></div>
      <div class="actions">
        <button class="btn primary" onclick={() => store.shuffleTracks(tracks)}><Icon name="shuffle" size={15} />Shuffle library</button>
      </div>
    </div>
  {:else}
    <div class="welcome">
      <div>
        <h2 class="serif">Bring your music.</h2>
        <p>
          Catlify plays everything: FLAC, ALAC, WAV, AIFF, APE, WavPack, DSD, Opus, MP3 and more, up to 32-bit/384 kHz.
          Add a folder to build your library.
        </p>
      </div>
      <button class="btn primary" onclick={() => store.go({ name: "settings" })}><Icon name="folder" size={15} />Add a music folder</button>
    </div>
  {/if}

  {#if recentlyPlayed.length}
    <h2 class="section-title">Jump back in</h2>
    <div class="recent">
      {#each recentlyPlayed as t, i (t.id)}
        <button class="recent-item" onclick={() => store.playTracks(recentlyPlayed, i)}>
          <Cover src={t.cover} size="56px" radius="8px" seed={t.album} />
          <span class="ellipsis">{t.title}</span>
          <span class="play"><Icon name="play" size={14} /></span>
        </button>
      {/each}
    </div>
  {/if}

  {#if trending.length}
    <h2 class="section-title">
      Trending now
      <button onclick={() => store.go({ name: "billboard" })}>See the Billboard →</button>
    </h2>
    <div class="trending">
      {#each trending as e (e.rank)}
        {@const local = libIndex.get(norm(e.artist) + "|" + norm(e.title))}
        <button
          class="trend"
          onclick={() =>
            local
              ? store.playTracks([local])
              : e.preview && store.playItems([{ id: null, path: e.preview, title: e.title, artist: e.artist, album: e.album ?? "", duration: 30, cover: e.artwork }])}
        >
          <span class="rank serif">{e.rank}</span>
          <Cover src={e.artwork} size="48px" radius="6px" />
          <span class="tt">
            <span class="ellipsis t">{e.title}</span>
            <span class="ellipsis a">{e.artist}</span>
          </span>
          {#if local}<span class="badge local">In library</span>{/if}
        </button>
      {/each}
    </div>
  {/if}

  {#if recentAlbums.length}
    <h2 class="section-title">
      Recently added
      <button onclick={() => store.go({ name: "albums" })}>All albums →</button>
    </h2>
    <div class="grid-cards">
      {#each recentAlbums as a (a.key)}
        <AlbumCard album={a} />
      {/each}
    </div>
  {/if}

  {#if mostPlayed.length}
    <h2 class="section-title">On repeat</h2>
    <TrackList tracks={mostPlayed} />
  {/if}
</div>

<style>
  .stats {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 36px;
    padding: 22px 0 6px;
    border-top: 1px solid var(--line);
  }
  .stats div {
    display: flex;
    flex-direction: column;
  }
  .stats b {
    font-size: 30px;
    letter-spacing: -0.02em;
    line-height: 1;
  }
  .stats span {
    color: var(--dim);
    font-size: 12px;
    margin-top: 4px;
  }
  .grad {
    background: var(--accent);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  .stats .actions {
    margin-left: auto;
  }
  .welcome {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 30px;
    padding: 34px;
    border-radius: var(--radius-lg);
    background: radial-gradient(ellipse at 0% 0%, rgba(157, 134, 255, 0.22), transparent 60%),
      radial-gradient(ellipse at 100% 100%, rgba(255, 123, 103, 0.18), transparent 60%), var(--bg-raised);
    border: 1px solid var(--line);
  }
  .welcome h2 {
    font-size: 30px;
    margin: 0 0 8px;
  }
  .welcome p {
    color: var(--muted);
    max-width: 560px;
    margin: 0;
  }
  .recent {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 10px;
  }
  .recent-item {
    display: flex;
    align-items: center;
    gap: 14px;
    padding-right: 14px;
    border-radius: 10px;
    background: var(--surface);
    overflow: hidden;
    text-align: left;
    font-weight: 600;
    transition: background 0.15s;
  }
  .recent-item:hover {
    background: var(--surface-hover);
  }
  .recent-item > span:nth-child(2) {
    flex: 1;
  }
  .play {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--text);
    color: #111;
    opacity: 0;
    transition: opacity 0.15s;
  }
  .recent-item:hover .play {
    opacity: 1;
  }
  .trending {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 4px 24px;
  }
  .trend {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px;
    border-radius: 10px;
    text-align: left;
  }
  .trend:hover {
    background: var(--surface);
  }
  .rank {
    width: 26px;
    font-size: 22px;
    font-weight: 700;
    color: var(--dim);
    text-align: center;
  }
  .tt {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .t {
    font-weight: 600;
  }
  .a {
    color: var(--muted);
    font-size: 13px;
  }
  :global(html.m) .stats {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 16px 12px;
  }
  :global(html.m) .stats b {
      font-size: 24px;
  }
  :global(html.m) .stats .actions {
      grid-column: 1 / -1;
  }
  :global(html.m) .stats .actions :global(.btn) {
      width: 100%;
      justify-content: center;
  }
  :global(html.m) .recent {
      grid-template-columns: 1fr 1fr;
      gap: 8px;
  }
  :global(html.m) .trending {
      grid-template-columns: 1fr;
  }
  :global(html.m) .welcome {
      padding: 22px;
      gap: 16px;
      flex-direction: column;
      align-items: flex-start;
  }
</style>
