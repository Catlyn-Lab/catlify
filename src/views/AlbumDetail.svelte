<script lang="ts">
  import { store } from "../lib/store.svelte";
  import { qualityLabel } from "../lib/util";
  import Hero from "../components/Hero.svelte";
  import TrackList from "../components/TrackList.svelte";

  let { key }: { key: string } = $props();
  const album = $derived(store.albums.find((a) => a.key === key));
  const discs = $derived(new Set(album?.tracks.map((t) => t.discNo ?? 1)).size);
  const best = $derived(
    album?.tracks.reduce((b, t) => ((t.bitDepth ?? 0) * (t.sampleRate ?? 0) > (b.bitDepth ?? 0) * (b.sampleRate ?? 0) ? t : b)),
  );
</script>

<div class="page fade-in">
  {#if album}
    <Hero kicker={album.hiRes ? "Hi-Res Album" : album.lossless ? "Lossless Album" : "Album"} title={album.title} cover={album.cover} tracks={album.tracks}>
      {#snippet meta()}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <b class="link" onclick={() => store.go({ name: "artist", artist: album.artist })}>{album.artist}</b>
        {#if album.year}<span>{album.year}</span>{/if}
        {#if album.tracks[0]?.genre}<span>{album.tracks[0].genre}</span>{/if}
        {#if best}<span class="badge" class:hires={album.hiRes} class:lossless={album.lossless && !album.hiRes}>{qualityLabel(best.codec, best.lossless ? best.bitDepth : null, best.sampleRate)}</span>{/if}
        {#if discs > 1}<span>{discs} discs</span>{/if}
      {/snippet}
    </Hero>
    <TrackList tracks={album.tracks} showAlbum={false} showCover={false} numbered />
  {:else}
    <div class="empty"><h3>Album not found</h3></div>
  {/if}
</div>
