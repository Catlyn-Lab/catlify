<script lang="ts">
  import { api, type Lyrics, type QueueItem } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { parseLrc } from "../lib/util";
  import { keepScreenOn } from "../lib/platform";

  // Reading along: don't let the screen dim or lock while lyrics are visible.
  $effect(() => keepScreenOn());

  let { large = false }: { large?: boolean } = $props();

  const track = $derived(store.player.track);
  let lyrics = $state<Lyrics | null>(null);
  let loading = $state(false);
  let failed = $state<string | null>(null);
  let loadedFor = "";

  async function load(item: QueueItem, force = false) {
    loadedFor = item.path;
    loading = true;
    failed = null;
    try {
      const l = await api.lyrics($state.snapshot(item) as QueueItem, force);
      if (loadedFor === item.path) lyrics = l;
    } catch (e) {
      if (loadedFor === item.path) {
        failed = String(e);
        lyrics = null;
      }
    } finally {
      if (loadedFor === item.path) loading = false;
    }
  }

  $effect(() => {
    if (track && track.path !== loadedFor) load(track);
    if (!track) lyrics = null;
  });

  const lines = $derived(lyrics?.synced ? parseLrc(lyrics.synced) : []);
  const current = $derived.by(() => {
    const pos = store.position + 0.15;
    let lo = 0,
      hi = lines.length - 1,
      idx = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (lines[mid].time <= pos) {
        idx = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    return idx;
  });

  let box: HTMLDivElement | undefined = $state();
  let userScrollUntil = 0;
  $effect(() => {
    const i = current;
    if (!box || i < 0 || performance.now() < userScrollUntil) return;
    const el = box.querySelector<HTMLElement>(`[data-i="${i}"]`);
    if (!el) return;
    box.scrollTo({ top: el.offsetTop - box.clientHeight * 0.38, behavior: "smooth" });
  });
</script>

<div class="lyrics" class:large bind:this={box} onwheel={() => (userScrollUntil = performance.now() + 3500)}>
  {#if !track}
    <p class="msg">Play a song to see its lyrics.</p>
  {:else if loading && !lyrics}
    <p class="msg">Finding lyrics…</p>
  {:else if lines.length}
    <div class="pad"></div>
    {#each lines as line, i}
      <button
        data-i={i}
        class="line"
        class:active={i === current}
        class:past={i < current}
        onclick={() => store.seek(line.time)}
      >
        {line.text || "♪"}
      </button>
    {/each}
    <div class="pad"></div>
  {:else if lyrics?.plain}
    <div class="plain">{lyrics.plain}</div>
  {:else}
    <div class="msg">
      <p>{failed ? "Couldn't reach the lyrics service." : "No lyrics found for this song."}</p>
      <button class="btn" onclick={() => track && load(track, true)}>Try again</button>
    </div>
  {/if}
  {#if lyrics && lyrics.source !== "none"}
    <p class="source">
      {lyrics.source === "lrclib" ? "Lyrics from LRCLIB" : lyrics.source === "file" ? "Lyrics from .lrc file" : "Embedded lyrics"}
    </p>
  {/if}
</div>

<style>
  .lyrics {
    height: 100%;
    overflow-y: auto;
    padding: 0 24px;
    scrollbar-width: none;
    mask-image: linear-gradient(transparent, #000 12%, #000 82%, transparent);
  }
  .lyrics::-webkit-scrollbar {
    display: none;
  }
  .pad {
    height: 35%;
  }
  .line {
    display: block;
    width: 100%;
    text-align: left;
    padding: 7px 0;
    font-family: var(--serif);
    font-size: 21px;
    font-weight: 600;
    line-height: 1.3;
    color: rgba(255, 255, 255, 0.28);
    transition: color 0.3s, transform 0.3s;
    transform-origin: left center;
  }
  .line.past {
    color: rgba(255, 255, 255, 0.45);
  }
  .line.active {
    color: var(--text);
    transform: scale(1.03);
  }
  .line:hover {
    color: rgba(255, 255, 255, 0.75);
  }
  .large .line {
    font-size: clamp(26px, 3.1vw, 40px);
    padding: 10px 0;
    letter-spacing: -0.01em;
  }
  .plain {
    white-space: pre-wrap;
    font-family: var(--serif);
    font-size: 18px;
    line-height: 1.7;
    color: var(--muted);
    padding: 40px 0;
    user-select: text;
  }
  .large .plain {
    font-size: 24px;
  }
  .msg {
    color: var(--muted);
    padding-top: 40%;
    text-align: center;
  }
  .source {
    font-size: 11px;
    color: var(--dim);
    padding: 10px 0 30px;
  }
</style>
