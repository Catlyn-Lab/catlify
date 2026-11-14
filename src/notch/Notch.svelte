<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { api, type PlayerState } from "../lib/api";
  import { fmtTime, isHiRes, streamLabel } from "../lib/util";
  import Cover from "../components/Cover.svelte";
  import Icon from "../components/Icon.svelte";

  interface Geometry {
    notchWidth: number;
    notchHeight: number;
    hasNotch: boolean;
  }

  let geo = $state<Geometry>({ notchWidth: 190, notchHeight: 32, hasNotch: true });
  let player = $state<PlayerState | null>(null);
  let receivedAt = 0;
  let open = $state(false);
  let peek = $state(false);
  let now = $state(performance.now());

  const track = $derived(player?.track ?? null);
  const playing = $derived(!!player?.playing);
  const mode = $derived(open ? "open" : peek && track ? "peek" : track ? "live" : "idle");

  // Island sizes, all derived from the real notch so the collapsed shape lines up with it.
  const size = $derived.by(() => {
    const N = geo.notchWidth;
    const H = geo.notchHeight;
    const ear = Math.round(H * 1.25);
    switch (mode) {
      case "open":
        return track ? { w: Math.max(N + 300, 500), h: H + 132 } : { w: Math.max(N + 120, 330), h: H + 58 };
      case "peek":
        return { w: N + 2 * ear + 70, h: H + 38 };
      case "live":
        return { w: N + 2 * ear, h: H };
      default:
        return { w: N, h: H };
    }
  });

  // Tell Rust where the island is, so only that area takes the mouse.
  $effect(() => {
    invoke("notch_shape", { width: size.w, height: size.h }).catch(() => {});
  });

  const position = $derived.by(() => {
    if (!player) return 0;
    const p = player.position + (player.playing ? (now - receivedAt) / 1000 : 0);
    return Math.min(Math.max(p, 0), player.duration || p);
  });
  const progress = $derived(player?.duration ? position / player.duration : 0);
  const stream = $derived(player?.stream ?? null);
  const hires = $derived(stream ? isHiRes(stream.bitDepth, stream.sampleRate, stream.lossless) : false);

  function apply(state: PlayerState) {
    const prev = player?.track?.path;
    player = state;
    receivedAt = performance.now();
    now = receivedAt;
    // Briefly show the new title when the song changes on its own.
    if (prev !== undefined && state.track && state.track.path !== prev && !open) {
      peek = true;
      clearTimeout(peekTimer);
      peekTimer = setTimeout(() => (peek = false), 3200);
    }
  }

  let peekTimer: ReturnType<typeof setTimeout> | undefined;
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const offs: Promise<() => void>[] = [
      listen<PlayerState>("player://state", (e) => apply(e.payload)),
      listen<Geometry>("notch://geometry", (e) => (geo = e.payload)),
      listen<boolean>("notch://hover", (e) => {
        clearTimeout(hoverTimer);
        // Small hover intent on the way in so passing over the notch doesn't pop it open.
        hoverTimer = setTimeout(
          () => {
            open = e.payload;
            if (open) peek = false;
          },
          e.payload ? 90 : 220,
        );
      }),
    ];
    api.state().then((s) => player ?? apply(s)).catch(() => {});
    invoke<Geometry | null>("notch_geometry").then((g) => g && (geo = g)).catch(() => {});
    return () => offs.forEach((p) => p.then((off) => off()));
  });

  // Animate the progress bar only while it's visible.
  $effect(() => {
    if (!open || !playing) return;
    let raf = requestAnimationFrame(function tick() {
      now = performance.now();
      raf = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(raf);
  });

  function seek(e: MouseEvent) {
    const bar = e.currentTarget as HTMLElement;
    const r = bar.getBoundingClientRect();
    const frac = Math.min(Math.max((e.clientX - r.left) / r.width, 0), 1);
    if (player?.duration) api.seek(frac * player.duration);
  }

  const openApp = () => invoke("notch_open_app");
</script>

<div
  class="island {mode}"
  class:playing
  class:fake={!geo.hasNotch}
  style:width="{size.w}px"
  style:height="{size.h}px"
  style:--h="{geo.notchHeight}px"
  style:--ear="{Math.round(geo.notchHeight * 1.25)}px"
  style:--notch="{geo.notchWidth}px"
>
  {#if track && mode !== "open"}
    <!-- Collapsed: art on the left ear, a level meter on the right. -->
    <div class="ears">
      <div class="ear-art"><Cover src={track.cover} radius="6px" seed={track.album} /></div>
      <div class="bars" aria-hidden="true"><i></i><i></i><i></i><i></i></div>
    </div>
  {/if}

  {#if mode === "peek" && track}
    <div class="peek-text">
      <span class="ellipsis t">{track.title}</span>
      <span class="ellipsis a">{track.artist}</span>
    </div>
  {/if}

  {#if mode === "open"}
    <div class="panel">
      <div class="top">
        <span class="label">{track ? (playing ? "Now Playing" : "Paused") : "Catlify"}</span>
        <!-- Nothing in the middle: that's where the camera housing is. -->
        <span class="camera"></span>
        <span class="right">
        {#if stream}
          <span class="badge" class:hires class:lossless={stream.lossless && !hires} title={hires ? "Hi-Res" : undefined}>
            {streamLabel(stream)}
          </span>
        {/if}
        <button class="icon-btn small" onclick={openApp} aria-label="Open Catlify" title="Open Catlify">
          <Icon name="external" size={13} />
        </button>
        </span>
      </div>

      {#if track}
        <div class="body">
          <button class="art" onclick={openApp} aria-label="Open Catlify">
            <Cover src={track.cover} radius="12px" seed={track.album} />
          </button>
          <div class="info">
            <div class="ellipsis title">{track.title}</div>
            <div class="ellipsis artist">{track.artist}{track.album ? ` — ${track.album}` : ""}</div>
            <div class="progress">
              <span class="time">{fmtTime(position)}</span>
              <button class="bar" onclick={seek} aria-label="Seek">
                <span class="fill" style:width="{progress * 100}%"></span>
              </button>
              <span class="time">-{fmtTime(Math.max((player?.duration ?? 0) - position, 0))}</span>
            </div>
            <div class="controls">
              <button
                class="icon-btn"
                class:on={player?.shuffle}
                onclick={() => api.setShuffle(!player?.shuffle)}
                aria-label="Shuffle"
              >
                <Icon name="shuffle" size={15} />
              </button>
              <button class="icon-btn" onclick={() => api.control("previous")} aria-label="Previous">
                <Icon name="prev" size={19} />
              </button>
              <button class="icon-btn main" onclick={() => api.control("toggle")} aria-label={playing ? "Pause" : "Play"}>
                <Icon name={playing ? "pause" : "play"} size={22} />
              </button>
              <button class="icon-btn" onclick={() => api.control("next")} aria-label="Next">
                <Icon name="next" size={19} />
              </button>
              <button
                class="icon-btn"
                class:on={player?.repeat !== "off"}
                onclick={() => api.setRepeat(player?.repeat === "off" ? "all" : player?.repeat === "all" ? "one" : "off")}
                aria-label="Repeat"
              >
                <Icon name="repeat" size={15} />
                {#if player?.repeat === "one"}<span class="one">1</span>{/if}
              </button>
            </div>
          </div>
        </div>
      {:else}
        <div class="empty">
          <span>Nothing playing</span>
          <button class="pill" onclick={openApp}>Open Catlify</button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    background: transparent;
    overflow: hidden;
    user-select: none;
    -webkit-user-select: none;
    cursor: default;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif;
    color: #f4f0f7;
    -webkit-font-smoothing: antialiased;
  }

  .island {
    /* Damped spring, ~2% overshoot. */
    --spring: linear(0, 0.0814, 0.2551, 0.449, 0.6246, 0.7656, 0.8691, 0.9391, 0.9824, 1.0062, 1.0171, 1.0199, 1.0184,
        1.015, 1.0111, 1.0076, 1.0048, 1.0027, 1.0013, 1.0004, 0.9999, 0.9997, 0.9996, 0.9996, 1);
    --r: calc(var(--h) * 0.32);
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    background: #000;
    border-radius: 0 0 var(--r) var(--r);
    transition:
      width 0.52s var(--spring),
      height 0.52s var(--spring),
      border-radius 0.52s var(--spring),
      box-shadow 0.4s ease,
      opacity 0.25s ease;
    overflow: hidden;
  }
  .island.idle {
    /* Same size as the camera housing: invisible on a real notch. */
    opacity: 0;
  }
  .island.idle.fake {
    opacity: 1;
  }
  .island.peek {
    --r: 18px;
  }
  .island.open {
    --r: 26px;
    box-shadow:
      0 14px 40px rgba(0, 0, 0, 0.55),
      0 0 0 1px rgba(255, 255, 255, 0.04);
  }

  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ---- collapsed ---- */
  .ears {
    position: absolute;
    inset: 0 0 auto 0;
    height: var(--h);
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 calc((var(--ear) - var(--h) * 0.68) / 2 + 2px);
    animation: fade 0.35s ease 0.12s both;
  }
  .ear-art {
    width: calc(var(--h) * 0.68);
  }
  .bars {
    width: calc(var(--h) * 0.6);
    height: calc(var(--h) * 0.44);
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
  }
  .bars i {
    width: 18%;
    height: 100%;
    border-radius: 2px;
    background: linear-gradient(180deg, #ff7b67, #9d86ff);
    transform-origin: bottom;
    transform: scaleY(0.25);
    animation: level 0.9s ease-in-out infinite alternate paused;
  }
  .bars i:nth-child(2) {
    animation-duration: 0.62s;
    animation-delay: -0.3s;
  }
  .bars i:nth-child(3) {
    animation-duration: 0.78s;
    animation-delay: -0.55s;
  }
  .bars i:nth-child(4) {
    animation-duration: 0.55s;
    animation-delay: -0.15s;
  }
  .playing .bars i {
    animation-play-state: running;
  }
  @keyframes level {
    0% {
      transform: scaleY(0.2);
    }
    50% {
      transform: scaleY(0.95);
    }
    100% {
      transform: scaleY(0.45);
    }
  }

  .peek-text {
    position: absolute;
    top: var(--h);
    inset-inline: 22px;
    display: flex;
    gap: 8px;
    align-items: baseline;
    justify-content: center;
    font-size: 13px;
    animation: fade 0.3s ease 0.18s both;
  }
  .peek-text .t {
    font-weight: 600;
    max-width: 62%;
  }
  .peek-text .a {
    color: #a39cae;
  }

  /* ---- open ---- */
  .panel {
    position: absolute;
    inset: 0;
    padding: 0 20px 16px;
    display: flex;
    flex-direction: column;
    animation: fade 0.3s ease 0.14s both;
    min-width: 330px;
  }
  .top {
    height: var(--h);
    display: grid;
    grid-template-columns: 1fr calc(var(--notch) + 16px) 1fr;
    align-items: center;
    font-size: 11px;
  }
  .right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
  }
  .label {
    color: #6d6778;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-weight: 600;
  }
  .badge {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 99px;
    background: rgba(255, 255, 255, 0.08);
    color: #a39cae;
  }
  .badge.lossless {
    color: #9d86ff;
  }
  .badge.hires {
    color: #e9c46a;
  }
  .body {
    flex: 1;
    display: flex;
    gap: 16px;
    align-items: center;
    padding-top: 6px;
  }
  .art {
    width: 92px;
    flex: none;
    padding: 0;
    border: 0;
    background: none;
    border-radius: 12px;
    cursor: pointer;
    transition: transform 0.2s ease;
  }
  .art:active {
    transform: scale(0.96);
  }
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .title {
    font-size: 15px;
    font-weight: 650;
  }
  .artist {
    font-size: 12.5px;
    color: #a39cae;
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }
  .time {
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
    color: #6d6778;
    min-width: 34px;
  }
  .time:last-child {
    text-align: right;
  }
  .bar {
    flex: 1;
    height: 14px;
    padding: 0;
    border: 0;
    background: none;
    cursor: pointer;
    position: relative;
  }
  .bar::before,
  .fill {
    content: "";
    position: absolute;
    left: 0;
    top: 5px;
    height: 4px;
    border-radius: 2px;
  }
  .bar::before {
    right: 0;
    background: rgba(255, 255, 255, 0.12);
  }
  .fill {
    background: linear-gradient(90deg, #9d86ff, #ff7b67);
  }
  .bar:hover .fill,
  .bar:hover::before {
    top: 4px;
    height: 6px;
    border-radius: 3px;
  }
  .controls {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 14px;
    margin-top: 4px;
  }
  .icon-btn {
    position: relative;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    color: #f4f0f7;
    cursor: pointer;
    transition:
      background 0.15s ease,
      transform 0.15s ease,
      color 0.15s ease;
  }
  .icon-btn:hover {
    background: rgba(255, 255, 255, 0.09);
  }
  .icon-btn:active {
    transform: scale(0.9);
  }
  .icon-btn.small {
    width: 22px;
    height: 22px;
    color: #a39cae;
  }
  .icon-btn.main {
    width: 40px;
    height: 40px;
    background: #f4f0f7;
    color: #000;
  }
  .controls .icon-btn:first-child,
  .controls .icon-btn:last-child {
    color: #6d6778;
  }
  .controls .icon-btn.on {
    color: #ff7b67;
  }
  .one {
    position: absolute;
    font-size: 8px;
    font-weight: 800;
    bottom: 5px;
    right: 6px;
  }
  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 13px;
    color: #a39cae;
  }
  .pill {
    border: 0;
    border-radius: 99px;
    padding: 6px 14px;
    background: rgba(255, 255, 255, 0.1);
    color: #f4f0f7;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .pill:hover {
    background: rgba(255, 255, 255, 0.16);
  }

  @keyframes fade {
    from {
      opacity: 0;
      filter: blur(4px);
    }
    to {
      opacity: 1;
      filter: none;
    }
  }
</style>
