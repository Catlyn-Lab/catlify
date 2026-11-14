<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Track } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { fmtDuration } from "../lib/util";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";

  let {
    kicker,
    title,
    cover,
    tracks,
    round = false,
    meta,
    actions,
  }: {
    kicker: string;
    title: string;
    cover?: string | null;
    tracks: Track[];
    round?: boolean;
    meta?: Snippet;
    actions?: Snippet;
  } = $props();

  const total = $derived(tracks.reduce((s, t) => s + t.duration, 0));
</script>

<header class="hero">
  {#if cover !== undefined}
    <div class="art"><Cover src={cover} radius={round ? "50%" : "12px"} seed={title} /></div>
  {/if}
  <div class="info">
    <div class="eyebrow">{kicker}</div>
    <h1 class="page-title">{title}</h1>
    <div class="meta">
      {@render meta?.()}
      <span>{tracks.length} songs, {fmtDuration(total)}</span>
    </div>
    <div class="actions">
      <button class="btn primary" onclick={() => store.playTracks(tracks)} disabled={!tracks.length}>
        <Icon name="play" size={14} />Play
      </button>
      <button class="btn" onclick={() => store.shuffleTracks(tracks)} disabled={!tracks.length}>
        <Icon name="shuffle" size={15} />Shuffle
      </button>
      {@render actions?.()}
    </div>
  </div>
</header>

<style>
  .hero {
    display: flex;
    align-items: flex-end;
    gap: 32px;
    margin-bottom: 30px;
  }
  .art {
    width: 220px;
    flex-shrink: 0;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
    border-radius: 12px;
  }
  .info {
    min-width: 0;
  }
  .page-title {
    margin-bottom: 10px;
    overflow-wrap: anywhere;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: center;
    color: var(--muted);
    font-size: 13.5px;
  }
  .actions {
    display: flex;
    gap: 10px;
    margin-top: 20px;
  }
  :global(html.m) .hero {
      flex-direction: column;
      align-items: center;
      text-align: center;
      gap: 18px;
  }
  :global(html.m) .art {
      width: min(64vw, 260px);
  }
  :global(html.m) .meta, :global(html.m) .actions {
      justify-content: center;
  }
</style>
