<script lang="ts">
  import { store } from "../lib/store.svelte";
  import AlbumCard from "../components/AlbumCard.svelte";
  import Cover from "../components/Cover.svelte";
  import TrackList from "../components/TrackList.svelte";

  import Icon from "../components/Icon.svelte";
  import { ytAvailable } from "../lib/platform";

  let { query }: { query: string } = $props();
  // Phones type here (no sidebar search box); the desktop passes the query in.
  // svelte-ignore state_referenced_locally
  let typed = $state(query);
  const q = $derived(store.mobile ? typed : query);

  const terms = $derived(q.toLowerCase().split(/\s+/).filter(Boolean));
  const matches = (s: string) => terms.length > 0 && terms.every((t) => s.toLowerCase().includes(t));

  const songs = $derived(store.tracks.filter((t) => matches(`${t.title} ${t.artist} ${t.album}`)).slice(0, 200));
  const albums = $derived(store.albums.filter((a) => matches(`${a.title} ${a.artist}`)).slice(0, 12));
  const artists = $derived(store.artists.filter((a) => matches(a.name)).slice(0, 8));
</script>

<div class="page fade-in">
  {#if store.mobile}
    <h1 class="page-title">Search</h1>
    <label class="msearch">
      <Icon name="search" size={18} />
      <input bind:value={typed} placeholder="Songs, artists, albums" type="search" enterkeyhint="search" autocomplete="off" />
    </label>
  {:else}
    <div class="eyebrow">Search</div>
    <h1 class="page-title">“{query}”</h1>
  {/if}

  {#if store.mobile && terms.length && ytAvailable()}
    <button class="ytsearch" onclick={() => store.go({ name: "youtube", query: q.trim() })}>
      <Icon name="download" size={17} />Search YouTube for “{q.trim()}”
    </button>
  {/if}

  {#if !terms.length}
    <!-- nothing typed yet -->
  {:else if !songs.length && !albums.length && !artists.length}
    <div class="empty"><h3>No results</h3><p>Try a different spelling, or search by artist or album.</p></div>
  {/if}

  {#if artists.length}
    <h2 class="section-title">Artists</h2>
    <div class="artists">
      {#each artists as a (a.name)}
        <button class="artist" onclick={() => store.go({ name: "artist", artist: a.name })}>
          <Cover src={a.cover} size="72px" radius="50%" seed={a.name} />
          <span class="ellipsis">{a.name}</span>
        </button>
      {/each}
    </div>
  {/if}

  {#if albums.length}
    <h2 class="section-title">Albums</h2>
    <div class="grid-cards">
      {#each albums as a (a.key)}<AlbumCard album={a} />{/each}
    </div>
  {/if}

  {#if songs.length}
    <h2 class="section-title">Songs</h2>
    <TrackList tracks={songs} />
  {/if}
</div>

<style>
  .msearch {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 46px;
    padding: 0 14px;
    border-radius: 12px;
    background: var(--surface);
    color: var(--dim);
    margin-bottom: 8px;
  }
  .ytsearch {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 12px 4px;
    color: var(--coral);
    font-weight: 600;
    border-bottom: 1px solid var(--line);
  }
  .msearch input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    outline: none;
    font-size: 16px; /* 16px stops iOS zooming on focus */
    color: var(--text);
  }
  .artists {
    display: flex;
    gap: 22px;
    flex-wrap: wrap;
  }
  .artist {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 96px;
    font-weight: 600;
    font-size: 13px;
  }
  .artist span {
    max-width: 100%;
  }
</style>
