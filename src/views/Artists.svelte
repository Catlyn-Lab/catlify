<script lang="ts">
  import { store } from "../lib/store.svelte";
  import Cover from "../components/Cover.svelte";
</script>

<div class="page fade-in">
  <div class="eyebrow">Library</div>
  <h1 class="page-title">Artists</h1>
  {#if store.artists.length}
    <div class="grid">
      {#each store.artists as a (a.name)}
        <button class="artist" onclick={() => store.go({ name: "artist", artist: a.name })}>
          <Cover src={a.cover} radius="50%" seed={a.name} />
          <span class="ellipsis n">{a.name}</span>
          <span class="c">{a.tracks} songs</span>
        </button>
      {/each}
    </div>
  {:else}
    <div class="empty"><h3>No artists yet</h3><p>Add a music folder in Settings.</p></div>
  {/if}
</div>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 28px 22px;
  }
  .artist {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    min-width: 0;
  }
  .artist :global(.cover) {
    box-shadow: 0 12px 30px rgba(0, 0, 0, 0.4);
    transition: transform 0.2s;
  }
  .artist:hover :global(.cover) {
    transform: scale(1.03);
  }
  .n {
    margin-top: 12px;
    font-weight: 600;
    max-width: 100%;
  }
  .c {
    font-size: 12px;
    color: var(--dim);
  }
</style>
