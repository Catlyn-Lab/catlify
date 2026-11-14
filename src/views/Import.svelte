<script lang="ts">
  import { api, type ImportCollection } from "../lib/api";
  import { importer, type ImportItem, type ImportJob } from "../lib/importer.svelte";
  import { ytAvailable } from "../lib/platform";
  import { MATCH_THRESHOLD } from "../lib/similarity";
  import { store } from "../lib/store.svelte";
  import { fmtTime } from "../lib/util";
  import Icon from "../components/Icon.svelte";
  import Spinner from "../components/Spinner.svelte";
  import { prepare, ytQueueItem } from "../lib/yt";
  import type { Candidate } from "../lib/importer.svelte";

  let link = $state("");
  let loading = $state(false);
  let error = $state<string | null>(null);
  let makePlaylist = $state(true);
  let fileInput: HTMLInputElement | undefined = $state();
  /** Collections found in a file, waiting for the user to pick which to import. */
  let found = $state<ImportCollection[]>([]);
  let help = $state<string | null>(null);
  const supported = ytAvailable();

  async function importLink() {
    const url = link.trim();
    if (!url) return;
    loading = true;
    error = null;
    try {
      try {
        start(await api.importLink(url));
      } catch (e) {
        if (!String(e).includes("youtube")) throw e;
        // YouTube / YouTube Music playlists: listed by yt-dlp, exact videos.
        const videos = await api.ytLookup(url, true);
        if (!videos.length) throw new Error("No songs found in that playlist");
        start({
          name: url.includes("music.youtube.com") ? "YouTube Music playlist" : "YouTube playlist",
          source: "YouTube Music",
          truncated: false,
          tracks: videos.map((v) => ({ title: v.title, artist: v.artist, album: null, duration: v.duration, videoId: v.id })),
        });
      }
      link = "";
    } catch (e) {
      error = String(e).replace(/^Error: /, "");
    } finally {
      loading = false;
    }
  }

  function start(c: ImportCollection) {
    if (!c.tracks.length) {
      error = `“${c.name}” has no songs`;
      return;
    }
    importer.add(c, makePlaylist);
    store.notify(`Importing ${c.tracks.length} songs from “${c.name}”`);
  }

  /** Text from export files: Apple Music's .txt is UTF-16. */
  async function readText(file: File) {
    const buf = new Uint8Array(await file.arrayBuffer());
    const utf16 = (buf[0] === 0xff && buf[1] === 0xfe) || (buf[0] === 0xfe && buf[1] === 0xff);
    return new TextDecoder(utf16 ? (buf[0] === 0xff ? "utf-16le" : "utf-16be") : "utf-8").decode(buf);
  }

  async function onFile(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    loading = true;
    error = null;
    try {
      const cs = await api.importFile(file.name, await readText(file));
      if (cs.length === 1) start(cs[0]);
      else found = cs;
    } catch (err) {
      error = String(err).replace(/^Error: /, "");
    } finally {
      loading = false;
      if (fileInput) fileInput.value = "";
    }
  }

  const review = $derived(
    importer.jobs.flatMap((j) => j.items.filter((i) => i.status === "review").map((i) => ({ job: j, item: i }))),
  );

  // Listen before choosing: streams the candidate without downloading it.
  const nowPath = $derived(store.player.track?.path);
  function listen(item: ImportItem, c: Candidate) {
    if (nowPath === `yt:${c.id}`) return api.control("toggle");
    store.playItems([ytQueueItem({ videoId: c.id, title: item.title, artist: item.artist, cover: c.thumbnail, duration: c.duration })], 0);
  }

  function searchManually(item: ImportItem) {
    store.go({ name: "youtube", query: `${item.artist} - ${item.title}` });
  }

  function statusText(job: ImportJob) {
    const c = importer.counts(job);
    const parts = [`${c.done} downloaded`, `${c.library} already in library`];
    if (c.review) parts.push(`${c.review} need your choice`);
    if (c.failed) parts.push(`${c.failed} failed`);
    if (c.skipped) parts.push(`${c.skipped} skipped`);
    return parts.join(" · ");
  }
</script>

<div class="page fade-in">
  <div class="eyebrow">Library</div>
  <h1 class="page-title">Import music</h1>
  <p class="lede">
    Bring your playlists from Spotify, YouTube Music or Apple Music. Catlify downloads each song from YouTube,
    automatically when the top result matches at least {MATCH_THRESHOLD}%, and lets you choose for the rest.
  </p>

  {#if !supported}
    <div class="card">
      <b>Not available on iPhone</b>
      <p class="muted">Importing downloads songs from YouTube, which works in Catlify for Mac, Windows, Linux and Android. Imported songs reach your iPhone with Wi-Fi sync.</p>
    </div>
  {:else}
    <form class="bar" onsubmit={(e) => (e.preventDefault(), importLink())}>
      <input bind:value={link} placeholder="Playlist link from Spotify, YouTube Music or Apple Music" spellcheck="false" autocapitalize="off" />
      <button class="btn primary" type="submit" disabled={loading || !link.trim()}>
        <Icon name="download" size={15} />{loading ? "Reading…" : "Import"}
      </button>
    </form>
    <div class="options">
      <button class="btn" onclick={() => fileInput?.click()} disabled={loading}><Icon name="file" size={15} />Import a file…</button>
      <input bind:this={fileInput} type="file" accept=".json,.csv,.txt,.xml" hidden onchange={onFile} />
      <label class="check"><input type="checkbox" bind:checked={makePlaylist} /> Create a playlist with the same name</label>
    </div>
    {#if error}<p class="error">{error}</p>{/if}

    {#if found.length}
      <div class="card">
        <b>This file has {found.length} playlists</b>
        {#each found as c, i (i)}
          <div class="found">
            <span class="ellipsis">{c.name} <span class="muted">· {c.tracks.length} songs</span></span>
            <button class="btn" onclick={() => { start(c); found = found.filter((x) => x !== c); }}>Import</button>
          </div>
        {/each}
        <div class="row">
          <button class="btn primary" onclick={() => { found.forEach(start); found = []; }}>Import all</button>
          <button class="btn" onclick={() => (found = [])}>Cancel</button>
        </div>
      </div>
    {/if}

    <div class="help">
      <span class="muted">Liked Songs or a whole library?</span>
      {#each ["Spotify", "YouTube Music", "Apple Music"] as s}
        <button class="chip" class:active={help === s} onclick={() => (help = help === s ? null : s)}>{s}</button>
      {/each}
    </div>
    {#if help === "Spotify"}
      <ol class="steps">
        <li>Public playlists and albums: share → Copy link, then paste it above.</li>
        <li>Liked Songs or private playlists: open <b>exportify.app</b>, sign in with Spotify and export the playlist as CSV, then choose <b>Import a file</b>.</li>
        <li>Or request your data in Spotify → Account → Privacy → Download your data, and import <code>YourLibrary.json</code> or <code>Playlist1.json</code>.</li>
      </ol>
    {:else if help === "YouTube Music"}
      <ol class="steps">
        <li>Playlists: ⋮ → Share → Copy link (make it Public or Unlisted), then paste it above.</li>
        <li>Your whole library: <b>takeout.google.com</b> → YouTube and YouTube Music → export, then import <code>music library songs.csv</code>.</li>
      </ol>
    {:else if help === "Apple Music"}
      <ol class="steps">
        <li>Shared playlists: ⋯ → Share → Copy link, then paste it above.</li>
        <li>On a Mac: select a playlist (or Songs), then File → Library → Export Playlist… and import the .txt or .xml file.</li>
      </ol>
    {/if}

    {#if review.length}
      <h2 class="section-title">Needs your choice <span class="count">{review.length}</span></h2>
      <p class="muted">The top YouTube result didn't match well enough to download it automatically.</p>
      {#each review as { job, item } (job.id + item.key)}
        <div class="review">
          <div class="want">
            <b class="ellipsis">{item.title}</b>
            <span class="muted ellipsis">{item.artist}{item.album ? ` · ${item.album}` : ""}{item.duration ? ` · ${fmtTime(item.duration)}` : ""}</span>
            <span class="from muted">From “{job.name}”</span>
          </div>
          <div class="cands">
            {#each item.candidates ?? [] as c (c.id)}
              {@const current = nowPath === `yt:${c.id}`}
              <div class="cand" class:current>
                <button
                  class="thumb"
                  onclick={() => listen(item, c)}
                  onpointerenter={() => prepare([`yt:${c.id}`])}
                  ontouchstart={() => prepare([`yt:${c.id}`])}
                  aria-label="Listen"
                  title="Listen"
                >
                  <img src={c.thumbnail} alt="" loading="lazy" />
                  <span class="ov" class:show={current}>
                    {#if current && store.player.loading}<Spinner size={18} color="#fff" />
                    {:else}<Icon name={current && store.player.playing ? "pause" : "play"} size={16} />{/if}
                  </span>
                </button>
                <span class="ctext">
                  <span class="ellipsis">{c.videoTitle}</span>
                  <span class="muted ellipsis">
                    {current && store.player.loading ? "Loading…" : `${c.channel}${c.duration ? ` · ${fmtTime(c.duration)}` : ""}`}
                  </span>
                </span>
                <span class="score" class:good={c.score >= 75}>{c.score}%</span>
                <button class="btn small" onclick={() => importer.choose(job, item, c)} title="Download this one">
                  <Icon name="download" size={14} />Download
                </button>
              </div>
            {:else}
              <p class="muted">No results on YouTube.</p>
            {/each}
          </div>
          <div class="row">
            <button class="btn" onclick={() => searchManually(item)}><Icon name="search" size={14} />Search YouTube</button>
            <button class="btn" onclick={() => importer.skip(job, item)}>Skip</button>
          </div>
        </div>
      {/each}
    {/if}

    {#if importer.jobs.length}
      <h2 class="section-title">Imports</h2>
      {#each importer.jobs as job (job.id)}
        {@const c = importer.counts(job)}
        {@const finished = c.done + c.library + c.failed + c.skipped + c.review}
        <div class="card job">
          <div class="row-between">
            <span class="ellipsis"><b>{job.name}</b> <span class="muted">· {job.source} · {c.total} songs</span></span>
            <span class="row">
              {#if c.pending || c.working}
                <button class="btn" onclick={() => importer.setPaused(job, !job.paused)}>{job.paused ? "Resume" : "Pause"}</button>
              {/if}
              <button class="icon-btn" onclick={() => importer.remove(job)} aria-label="Remove from list" title="Remove from list"><Icon name="x" size={15} /></button>
            </span>
          </div>
          <span class="bar"><span style:width="{c.total ? (finished / c.total) * 100 : 0}%"></span></span>
          <span class="muted">
            {#if c.pending || c.working}{job.paused ? "Paused" : `Working… ${finished} of ${c.total}`} · {/if}{statusText(job)}
          </span>
          {#if job.truncated}<span class="warn">The public page only listed the first {c.total} songs. Export the full playlist as a file to get the rest.</span>{/if}
        </div>
      {/each}
    {/if}

    <p class="note">
      Only download music you have the right to, and respect YouTube's Terms of Service and artists' rights. Songs are
      saved to your library with the playlist's titles and artists.
    </p>
  {/if}
</div>

<style>
  .lede {
    color: var(--muted);
    margin: 4px 0 20px;
    max-width: 660px;
  }
  .bar {
    display: flex;
    gap: 8px;
    max-width: 760px;
  }
  .bar input {
    flex: 1;
    min-width: 0;
    height: 42px;
    padding: 0 14px;
    border-radius: var(--radius);
    border: 1px solid var(--line-strong);
    background: var(--surface);
    color: var(--text);
    outline: none;
  }
  .bar input:focus {
    border-color: var(--violet);
  }
  .options {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
    margin: 12px 0;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    font-size: 13.5px;
  }
  .check input {
    accent-color: var(--coral);
  }
  .help {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin: 18px 0 6px;
  }
  .steps {
    max-width: 660px;
    color: var(--muted);
    font-size: 13.5px;
    padding-left: 20px;
    margin: 8px 0 0;
  }
  .steps li {
    margin-bottom: 6px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 14px 16px;
    border-radius: var(--radius-lg);
    background: var(--surface);
    border: 1px solid var(--line);
    margin: 10px 0;
    max-width: 760px;
  }
  .card p {
    margin: 0;
  }
  .found {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }
  .row-between {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
  }
  .bar,
  .job .bar {
    position: relative;
  }
  .job .bar {
    height: 5px;
    border-radius: 3px;
    background: var(--line-strong);
    overflow: hidden;
  }
  .job .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
  }
  .count {
    font-family: var(--sans);
    font-size: 13px;
    background: var(--coral);
    color: #000;
    border-radius: 99px;
    padding: 1px 8px;
    margin-left: 8px;
  }
  .review {
    padding: 14px 0;
    border-bottom: 1px solid var(--line);
    max-width: 760px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .want {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .from {
    font-size: 12px;
  }
  .cands {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .cand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px;
    border-radius: 10px;
    text-align: left;
  }
  .cand:hover,
  .cand.current {
    background: var(--surface);
  }
  .thumb {
    position: relative;
    flex: none;
    border-radius: 6px;
    overflow: hidden;
  }
  .thumb img {
    width: 72px;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    display: block;
  }
  .ov {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.45);
    color: #fff;
    opacity: 0;
    transition: opacity 0.12s ease;
  }
  .thumb:hover .ov,
  .ov.show {
    opacity: 1;
  }
  :global(html.m) .ov {
    opacity: 1;
  }
  .btn.small {
    height: 32px;
    padding: 0 10px;
    font-size: 12.5px;
    flex: none;
  }
  .ctext {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    font-size: 13.5px;
  }
  .score {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--dim);
    flex: none;
  }
  .score.good {
    color: var(--gold);
  }
  .icon-btn {
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    color: var(--dim);
  }
  .muted {
    color: var(--muted);
    font-size: 13px;
  }
  .error,
  .warn {
    color: var(--coral);
    font-size: 13px;
  }
  .note {
    margin-top: 26px;
    color: var(--dim);
    font-size: 12.5px;
    max-width: 660px;
  }
  code {
    font-family: var(--mono);
    font-size: 12px;
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  :global(html.m) .bar {
    flex-wrap: wrap;
  }
  :global(html.m) .bar input {
    flex-basis: 100%;
    height: 46px;
  }
  :global(html.m) .bar .btn {
    flex: 1;
    justify-content: center;
    height: 42px;
  }
</style>
