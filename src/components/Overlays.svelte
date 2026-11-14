<script lang="ts">
  import { store } from "../lib/store.svelte";

  let menuEl: HTMLDivElement | undefined = $state();
  let pos = $state({ x: 0, y: 0 });

  $effect(() => {
    const m = store.menu;
    if (!m || !menuEl) return;
    const r = menuEl.getBoundingClientRect();
    pos = {
      x: Math.min(m.x, window.innerWidth - r.width - 8),
      y: Math.min(m.y, window.innerHeight - r.height - 8),
    };
  });

  function close() {
    store.menu = null;
  }

  function finishConfirm(ok: boolean) {
    store.confirm?.resolve(ok);
    store.confirm = null;
  }

  function finishAsk(value: string | null) {
    store.ask?.resolve(value);
    store.ask = null;
  }

  let askInput: HTMLInputElement | undefined = $state();
  $effect(() => {
    if (store.ask) askInput?.focus();
  });
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && (close(), store.ask && finishAsk(null))} />

{#if store.menu}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" class:dim={store.mobile} onclick={close} oncontextmenu={(e) => (e.preventDefault(), close())}></div>
  <div class="menu fade-in" class:sheet={store.mobile} bind:this={menuEl} style:left="{pos.x || store.menu.x}px" style:top="{pos.y || store.menu.y}px" role="menu">
    {#each store.menu.items as item}
      {#if item.heading}
        <div class="heading">{item.label}</div>
      {:else}
        <button
          role="menuitem"
          class:danger={item.danger}
          onclick={() => {
            close();
            item.action?.();
          }}>{item.label}</button
        >
      {/if}
    {/each}
  </div>
{/if}

{#if store.ask}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="modal-scrim" onclick={(e) => e.target === e.currentTarget && finishAsk(null)}>
    <form
      class="modal fade-in"
      onsubmit={(e) => {
        e.preventDefault();
        finishAsk(store.ask?.value ?? null);
      }}
    >
      <h3>{store.ask.title}</h3>
      <input bind:this={askInput} bind:value={store.ask.value} placeholder="Name" />
      <div class="actions">
        <button type="button" class="btn" onclick={() => finishAsk(null)}>Cancel</button>
        <button type="submit" class="btn primary">Save</button>
      </div>
    </form>
  </div>
{/if}

{#if store.confirm}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="modal-scrim" onclick={(e) => e.target === e.currentTarget && finishConfirm(false)}>
    <div class="modal fade-in" role="alertdialog" aria-labelledby="confirm-title">
      <h3 id="confirm-title">{store.confirm.title}</h3>
      <p class="confirm-msg">{store.confirm.message}</p>
      <div class="actions">
        <button type="button" class="btn" onclick={() => finishConfirm(false)}>Cancel</button>
        <button type="button" class="btn danger-btn" onclick={() => finishConfirm(true)}>{store.confirm.action}</button>
      </div>
    </div>
  </div>
{/if}

{#if store.toast}
  <div class="toast">{store.toast}</div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 90;
  }
  .menu {
    position: fixed;
    z-index: 91;
    min-width: 210px;
    max-height: 70vh;
    overflow-y: auto;
    padding: 6px;
    border-radius: 12px;
    background: rgba(30, 27, 37, 0.92);
    backdrop-filter: blur(24px) saturate(1.4);
    -webkit-backdrop-filter: blur(24px) saturate(1.4);
    border: 1px solid var(--line-strong);
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.5);
  }
  .menu button {
    display: block;
    width: 100%;
    text-align: left;
    padding: 7px 10px;
    border-radius: 7px;
    font-size: 13px;
  }
  .menu button:hover {
    background: rgba(255, 255, 255, 0.08);
  }
  .scrim.dim {
    background: rgba(0, 0, 0, 0.45);
  }
  /* Phones: action sheet anchored to the bottom, thumb-sized rows. */
  .menu.sheet {
    left: calc(8px + var(--safe-left)) !important;
    right: calc(8px + var(--safe-right));
    top: auto !important;
    bottom: calc(8px + var(--safe-bottom));
    max-height: 70vh;
    border-radius: 16px;
    padding: 8px;
  }
  .menu.sheet button {
    padding: 13px 14px;
    font-size: 15px;
  }
  .menu .danger {
    color: #ff8a8a;
  }
  .heading {
    padding: 10px 10px 4px;
    font-size: 10.5px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--dim);
    border-top: 1px solid var(--line);
    margin-top: 4px;
  }
  .confirm-msg {
    color: var(--muted);
    margin: 0 0 18px;
    line-height: 1.5;
  }
  .danger-btn {
    background: #e5484d;
    color: #fff;
  }
  .danger-btn:hover {
    background: #f05d61;
  }
  .modal-scrim {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: rgba(0, 0, 0, 0.5);
    display: grid;
    place-items: center;
  }
  .modal {
    width: 380px;
    padding: 22px;
    border-radius: var(--radius-lg);
    background: var(--bg-raised);
    border: 1px solid var(--line-strong);
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.6);
  }
  .modal h3 {
    margin: 0 0 14px;
    font-family: var(--serif);
    font-size: 22px;
  }
  .modal input {
    width: 100%;
    height: 40px;
    padding: 0 12px;
    border-radius: 8px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    outline: none;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 18px;
  }
  .toast {
    position: fixed;
    bottom: calc(var(--player-h) + 18px);
    left: 50%;
    transform: translateX(-50%);
    z-index: 95;
    padding: 10px 18px;
    border-radius: 999px;
    background: var(--text);
    color: #111;
    font-weight: 600;
    font-size: 13px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.4);
    animation: toast-in 0.25s ease both;
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translate(-50%, 8px);
    }
  }
</style>
