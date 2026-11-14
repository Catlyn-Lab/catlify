<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { updater } from "../lib/updater.svelte";
  import icon from "../../src-tauri/icons/128x128@2x.png";

  let current = $state("");
  getVersion().then((v) => (current = v)).catch(() => {});

  const update = $derived(updater.update);
  const working = $derived(updater.phase === "downloading" || updater.phase === "installing");
  const percent = $derived(updater.progress >= 0 ? Math.round(updater.progress * 100) : null);
  const notes = $derived(update?.body?.trim() ?? "");
  const date = $derived.by(() => {
    const d = update?.date ? new Date(update.date.replace(" ", "T").replace(/ .*$/, "")) : null;
    return d && !isNaN(+d) ? d.toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric" }) : "";
  });
  const meta = $derived([current && `You have ${current}`, date && `Released ${date}`].filter(Boolean).join(" · "));
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && updater.later()} />

{#if updater.open && update}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="modal-scrim" onclick={(e) => e.target === e.currentTarget && updater.later()}>
    <div class="dialog fade-in" role="dialog" aria-modal="true" aria-labelledby="update-title" tabindex="-1">
      <div class="head">
        <img src={icon} alt="" class="logo" />
        <div>
          <p class="eyebrow">Update available</p>
          <h3 id="update-title">Catlify {update.version}</h3>
          <p class="meta">{meta}</p>
        </div>
      </div>

      {#if notes}
        <div class="notes">{notes}</div>
      {/if}

      {#if working}
        <div class="progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={percent ?? undefined}>
          <div class="bar" class:indeterminate={percent === null} style:width={percent === null ? undefined : `${percent}%`}></div>
        </div>
        <p class="status">
          {#if updater.phase === "installing"}Installing… Catlify will restart in a moment.
          {:else if percent !== null}Downloading… {percent}%
          {:else}Downloading…{/if}
        </p>
      {:else if updater.phase === "error"}
        <p class="error">The update didn't install: {updater.error}</p>
      {/if}

      <div class="actions">
        {#if !working}
          <button type="button" class="btn link" onclick={() => updater.skip()}>Skip this version</button>
          <span class="grow"></span>
          <button type="button" class="btn" onclick={() => updater.later()}>Later</button>
          <button type="button" class="btn primary" onclick={() => updater.install()}>
            {updater.phase === "error" ? "Try again" : "Update & restart"}
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-scrim {
    position: fixed;
    inset: 0;
    z-index: 120;
    background: rgba(0, 0, 0, 0.5);
    display: grid;
    place-items: center;
    padding: 16px;
  }
  .dialog {
    width: min(440px, 100%);
    padding: 22px;
    border-radius: var(--radius-lg);
    background: var(--bg-raised);
    border: 1px solid var(--line-strong);
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.6);
    outline: none;
  }
  .head {
    display: flex;
    gap: 14px;
    align-items: center;
  }
  .logo {
    width: 56px;
    height: 56px;
    border-radius: 14px;
    flex: none;
  }
  .eyebrow {
    margin: 0;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--violet);
  }
  h3 {
    margin: 2px 0 2px;
    font-family: var(--serif);
    font-size: 22px;
  }
  .meta {
    margin: 0;
    font-size: 12.5px;
    color: var(--muted);
  }
  .notes {
    margin-top: 16px;
    max-height: 200px;
    overflow-y: auto;
    padding: 12px 14px;
    border-radius: 10px;
    background: var(--surface);
    font-size: 13px;
    line-height: 1.5;
    white-space: pre-wrap;
    color: var(--text);
  }
  .progress {
    margin-top: 18px;
    height: 6px;
    border-radius: 3px;
    background: var(--surface);
    overflow: hidden;
  }
  .bar {
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transition: width 0.2s ease;
  }
  .bar.indeterminate {
    width: 35%;
    animation: slide 1.1s ease-in-out infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }
  .status {
    margin: 8px 0 0;
    font-size: 12.5px;
    color: var(--muted);
  }
  .error {
    margin: 14px 0 0;
    font-size: 13px;
    color: #ff8a8a;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 18px;
  }
  .grow {
    flex: 1;
  }
  .btn.link {
    background: none;
    border: none;
    padding-left: 0;
    color: var(--muted);
  }
  .btn.link:hover {
    color: var(--text);
  }
</style>
