<script lang="ts">
  import { api, type Track } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import Hero from "../components/Hero.svelte";
  import Icon from "../components/Icon.svelte";
  import TrackList from "../components/TrackList.svelte";

  let { id }: { id: number } = $props();
  let tracks = $state<Track[]>([]);
  const playlist = $derived(store.playlists.find((p) => p.id === id));
  const cover = $derived(tracks.find((t) => t.cover)?.cover ?? null);

  async function load() {
    tracks = await api.playlistTracks(id);
  }

  $effect(() => {
    id;
    store.playlists; // reload when songs are added from elsewhere
    load();
  });

  async function rename() {
    const name = await store.askText("Rename playlist", playlist?.name ?? "");
    if (!name?.trim()) return;
    await api.renamePlaylist(id, name.trim());
    await store.refreshPlaylists();
  }

  async function remove() {
    await api.deletePlaylist(id);
    await store.refreshPlaylists();
    store.go({ name: "home" });
    store.notify(`Deleted ${playlist?.name ?? "playlist"}`);
  }

  function moreMenu(e: MouseEvent) {
    store.openMenu(e, [
      { label: "Rename…", action: rename },
      { label: "Add all to queue", action: () => store.enqueue(tracks, false) },
      { label: "Delete playlist", action: remove, danger: true },
    ]);
  }
</script>

<div class="page fade-in">
  {#if playlist}
    <Hero kicker="Playlist" title={playlist.name} {cover} {tracks}>
      {#snippet actions()}
        <button class="btn" onclick={moreMenu} aria-label="More"><Icon name="more" size={18} /></button>
      {/snippet}
    </Hero>
    {#if tracks.length}
      <TrackList
        {tracks}
        extraMenu={(_, i) => [
          {
            label: "Remove from this playlist",
            danger: true,
            action: async () => {
              await api.removeFromPlaylist(id, i);
              await store.refreshPlaylists();
            },
          },
        ]}
      />
    {:else}
      <div class="empty"><h3>This playlist is empty</h3><p>Right-click any song and choose “Add to playlist”.</p></div>
    {/if}
  {/if}
</div>
