<script lang="ts">
  import { store } from "../lib/store.svelte";
  import AlbumCard from "../components/AlbumCard.svelte";

  type Sort = "artist" | "title" | "year" | "added";
  let sort = $state<Sort>("artist");
  let hiResOnly = $state(false);

  const albums = $derived.by(() => {
    let list = store.albums;
    if (hiResOnly) list = list.filter((a) => a.hiRes);
    const s = [...list];
    if (sort === "title") s.sort((a, b) => a.title.localeCompare(b.title));
    if (sort === "artist") s.sort((a, b) => a.artist.localeCompare(b.artist) || (a.year ?? 0) - (b.year ?? 0));
    if (sort === "year") s.sort((a, b) => (b.year ?? 0) - (a.year ?? 0));
    if (sort === "added") s.sort((a, b) => Math.max(...b.tracks.map((t) => t.addedAt)) - Math.max(...a.tracks.map((t) => t.addedAt)));
    return s;
  });
</script>

<div class="page fade-in">
  <div class="eyebrow">Library</div>
  <h1 class="page-title">Albums</h1>
  <div class="filters">
    {#each [["artist", "Artist"], ["title", "Title"], ["year", "Year"], ["added", "Recently added"]] as [k, label]}
      <button class="chip" class:active={sort === k} onclick={() => (sort = k as Sort)}>{label}</button>
    {/each}
    <button class="chip" class:active={hiResOnly} onclick={() => (hiResOnly = !hiResOnly)}>Hi-Res only</button>
  </div>
  {#if albums.length}
    <div class="grid-cards">
      {#each albums as a (a.key)}<AlbumCard album={a} />{/each}
    </div>
  {:else}
    <div class="empty"><h3>No albums</h3><p>Add a music folder in Settings.</p></div>
  {/if}
</div>

<style>
  .filters {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 24px;
  }
</style>
