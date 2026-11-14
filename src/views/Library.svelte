<script lang="ts">
  import { api } from "../lib/api";
  import { ytAvailable } from "../lib/platform";
  import { store, type View } from "../lib/store.svelte";
  import Icon from "../components/Icon.svelte";

  const liked = $derived(store.tracks.filter((t) => t.liked).length);
  const rows: { label: string; icon: string; view: View; count: () => number }[] = [
    { label: "Songs", icon: "note", view: { name: "songs" }, count: () => store.tracks.length },
    { label: "Albums", icon: "disc", view: { name: "albums" }, count: () => store.albums.length },
    { label: "Artists", icon: "mic", view: { name: "artists" }, count: () => store.artists.length },
    { label: "Liked Songs", icon: "heart", view: { name: "liked" }, count: () => liked },
  ];
  const showYt = ytAvailable();

  async function newPlaylist() {
    const name = await store.askText("New playlist");
    if (!name?.trim()) return;
    const id = await api.createPlaylist(name.trim());
    await store.refreshPlaylists();
    store.go({ name: "playlist", id });
  }
</script>

<div class="page fade-in">
  <div class="head">
    <h1 class="page-title">Library</h1>
    <button class="gear" onclick={() => store.go({ name: "settings" })} aria-label="Settings"><Icon name="gear" size={22} /></button>
  </div>

  {#if store.tracks.length}
    <button class="btn primary shuffle" onclick={() => store.shuffleTracks(store.tracks)}>
      <Icon name="shuffle" size={15} />Shuffle all
    </button>
  {/if}

  <div class="list">
    {#each rows as r (r.label)}
      <button class="row" onclick={() => store.go(r.view)}>
        <Icon name={r.icon} size={20} />
        <span class="label">{r.label}</span>
        <span class="count num">{r.count().toLocaleString()}</span>
        <span class="chev">›</span>
      </button>
    {/each}
    <button class="row" onclick={() => store.go({ name: "sync" })}>
      <Icon name="refresh" size={20} />
      <span class="label">Sync devices</span>
      <span class="chev">›</span>
    </button>
    {#if showYt}
      <button class="row" onclick={() => store.go({ name: "import" })}>
        <Icon name="plus" size={20} />
        <span class="label">Import from Spotify, YouTube Music, Apple Music</span>
        <span class="chev">›</span>
      </button>
      <button class="row" onclick={() => store.go({ name: "youtube" })}>
        <Icon name="download" size={20} />
        <span class="label">Download from YouTube</span>
        <span class="chev">›</span>
      </button>
    {/if}
  </div>

  <h2 class="section-title">
    Playlists
    <button onclick={newPlaylist}>+ New</button>
  </h2>
  <div class="list">
    {#each store.playlists as p (p.id)}
      <button class="row" onclick={() => store.go({ name: "playlist", id: p.id })}>
        <Icon name="list" size={20} />
        <span class="label ellipsis">{p.name}</span>
        <span class="count num">{p.trackCount}</span>
        <span class="chev">›</span>
      </button>
    {:else}
      <p class="none">No playlists yet.</p>
    {/each}
  </div>
</div>

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .gear {
    width: 44px;
    height: 44px;
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  .shuffle {
    margin-bottom: 14px;
  }
  .list {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    min-height: 54px;
    padding: 0 4px;
    border-bottom: 1px solid var(--line);
    text-align: left;
    color: var(--text);
  }
  .row :global(svg) {
    color: var(--coral);
    flex: none;
  }
  .label {
    flex: 1;
    font-size: 16px;
    min-width: 0;
  }
  .count {
    color: var(--dim);
  }
  .chev {
    color: var(--dim);
    font-size: 22px;
    line-height: 1;
  }
  .none {
    color: var(--dim);
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
