<script lang="ts">
  import type { Track } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import Hero from "../components/Hero.svelte";
  import TrackList from "../components/TrackList.svelte";

  let { liked = false }: { liked?: boolean } = $props();

  type Sort = "album" | "title" | "artist" | "added" | "plays" | "quality";
  let sort = $state<Sort>((localStorage.getItem("stafy.songSort") as Sort) ?? "album");
  let losslessOnly = $state(false);

  const sorters: Record<Sort, (a: Track, b: Track) => number> = {
    album: () => 0, // backend order: album artist → album → disc → track
    title: (a, b) => a.title.localeCompare(b.title),
    artist: (a, b) => a.artist.localeCompare(b.artist) || a.title.localeCompare(b.title),
    added: (a, b) => b.addedAt - a.addedAt,
    plays: (a, b) => b.playCount - a.playCount,
    quality: (a, b) => (b.bitDepth ?? 0) * (b.sampleRate ?? 0) - (a.bitDepth ?? 0) * (a.sampleRate ?? 0),
  };

  const list = $derived.by(() => {
    let t = store.tracks;
    if (liked) t = t.filter((x) => x.liked);
    if (losslessOnly) t = t.filter((x) => x.lossless);
    return sort === "album" ? t : [...t].sort(sorters[sort]);
  });

  $effect(() => localStorage.setItem("stafy.songSort", sort));
</script>

<div class="page fade-in">
  <Hero kicker={liked ? "Collection" : "Library"} title={liked ? "Liked Songs" : "Songs"} tracks={list} />

  <div class="filters">
    {#each [["album", "Album order"], ["title", "Title"], ["artist", "Artist"], ["added", "Recently added"], ["plays", "Most played"], ["quality", "Quality"]] as [k, label]}
      <button class="chip" class:active={sort === k} onclick={() => (sort = k as Sort)}>{label}</button>
    {/each}
    <span class="sep"></span>
    <button class="chip" class:active={losslessOnly} onclick={() => (losslessOnly = !losslessOnly)}>Lossless only</button>
  </div>

  {#if list.length}
    <TrackList tracks={list} />
  {:else}
    <div class="empty">
      <h3>{liked ? "No liked songs yet" : "Your library is empty"}</h3>
      <p>{liked ? "Tap the heart on any song to save it here." : "Add a music folder in Settings to get started."}</p>
      {#if !liked}<button class="btn primary" onclick={() => store.go({ name: "settings" })}>Open Settings</button>{/if}
    </div>
  {/if}
</div>

<style>
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 18px;
    align-items: center;
  }
  .sep {
    width: 1px;
    height: 20px;
    background: var(--line-strong);
    margin: 0 6px;
  }
</style>
