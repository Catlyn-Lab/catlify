<script lang="ts">
  import { store } from "../lib/store.svelte";
  import AlbumCard from "../components/AlbumCard.svelte";
  import Hero from "../components/Hero.svelte";
  import TrackList from "../components/TrackList.svelte";

  let { artist }: { artist: string } = $props();

  const albums = $derived(store.albums.filter((a) => a.artist === artist).sort((a, b) => (b.year ?? 0) - (a.year ?? 0)));
  // Songs where they're the album artist or a credited (track) artist.
  const tracks = $derived(store.tracks.filter((t) => t.albumArtist === artist || t.artist.includes(artist)));
  const popular = $derived([...tracks].sort((a, b) => b.playCount - a.playCount).slice(0, 5));
  const cover = $derived(albums[0]?.cover ?? tracks[0]?.cover ?? null);
</script>

<div class="page fade-in">
  <Hero kicker="Artist" title={artist} {cover} {tracks} round />
  {#if popular.some((t) => t.playCount > 0)}
    <h2 class="section-title">Popular in your library</h2>
    <TrackList tracks={popular} />
  {/if}
  {#if albums.length}
    <h2 class="section-title">Albums</h2>
    <div class="grid-cards">
      {#each albums as a (a.key)}<AlbumCard album={a} />{/each}
    </div>
  {/if}
  <h2 class="section-title">All songs</h2>
  <TrackList {tracks} />
</div>
