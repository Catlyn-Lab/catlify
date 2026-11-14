<script lang="ts">
  import { rpc, type SyncDevice, type SyncStatus } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import Icon from "../components/Icon.svelte";

  let status = $state<SyncStatus | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let code = $state("");
  let manual = $state("");
  let editingName = $state(false);
  let nameDraft = $state("");

  async function refresh() {
    try {
      const prev = status?.progress;
      status = await rpc<SyncStatus>("syncStatus");
      const p = status.progress;
      // A sync just finished: show the new songs, likes and playlists.
      if (prev?.active && p && !p.active) {
        await store.loadLibrary();
        if (p.phase === "done") store.notify(p.added ? `Synced ${p.added} new song${p.added === 1 ? "" : "s"}` : "Already in sync");
      }
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    refresh();
    const t = setInterval(refresh, 1000);
    return () => clearInterval(t);
  });

  async function act(fn: () => Promise<unknown>) {
    busy = true;
    error = null;
    try {
      await fn();
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const toggle = () => act(() => rpc("syncEnable", { on: !status?.enabled }));
  const pair = (addr: string) => act(() => rpc("syncPairBegin", { addr }));
  const finishPair = () =>
    act(async () => {
      const r = await rpc<{ name: string; id: string }>("syncPairComplete", { code });
      code = "";
      store.notify(`Paired with ${r.name}`);
    });
  const cancelPair = () => act(() => rpc("syncPairCancel"));
  const syncNow = (d: SyncDevice) => act(() => rpc("syncNow", { id: d.id }));
  // Forgetting only removes the pairing; songs already copied stay. Pair again any time.
  const forget = (d: SyncDevice) => act(() => rpc("syncForget", { id: d.id }));
  function saveName() {
    editingName = false;
    if (nameDraft.trim() && nameDraft.trim() !== status?.me.name) act(() => rpc("syncSetName", { name: nameDraft.trim() }));
  }

  const unpairedNearby = $derived((status?.nearby ?? []).filter((n) => !n.paired));
  const p = $derived(status?.progress ?? null);
  const pct = $derived(p && p.total ? Math.round((p.done / p.total) * 100) : 0);

  function ago(ts: number | null | undefined) {
    if (!ts) return "Never synced";
    const s = Date.now() / 1000 - ts;
    if (s < 60) return "Synced just now";
    if (s < 3600) return `Synced ${Math.round(s / 60)} min ago`;
    if (s < 86400) return `Synced ${Math.round(s / 3600)} h ago`;
    return `Synced ${new Date(ts * 1000).toLocaleDateString()}`;
  }
  const mb = (b: number) => (b / 1e6).toFixed(b > 1e8 ? 0 : 1);
  const phaseText: Record<string, string> = {
    connecting: "Connecting…",
    comparing: "Comparing libraries…",
    copying: "Copying songs",
    merging: "Merging likes and playlists…",
  };
</script>

<div class="page fade-in">
  <div class="eyebrow">Library</div>
  <h1 class="page-title">Sync devices</h1>
  <p class="lede">
    Copy songs, likes and playlists between your devices on the same Wi-Fi. No account or cloud: your music goes
    straight from one device to the other.
  </p>

  {#if status}
    <div class="card me">
      <label class="switch">
        <input type="checkbox" checked={status.enabled} onchange={toggle} disabled={busy} />
        <span class="track"><span class="knob"></span></span>
      </label>
      <div class="me-text">
        {#if editingName}
          <input
            class="name-input"
            bind:value={nameDraft}
            onblur={saveName}
            onkeydown={(e) => e.key === "Enter" && saveName()}
            maxlength="40"
          />
        {:else}
          <button class="name" onclick={() => ((nameDraft = status!.me.name), (editingName = true))} title="Rename this device">
            {status.me.name} <Icon name="more" size={14} />
          </button>
        {/if}
        <span class="sub">
          {#if status.enabled}
            Visible on Wi-Fi{#if status.me.addresses?.length && status.me.port} · {status.me.addresses[0]}:{status.me.port}{/if}
          {:else}
            Turn on to sync with your other devices
          {/if}
        </span>
      </div>
    </div>

    {#if error}<p class="error">{error.replace(/^Error: /, "")}</p>{/if}

    {#if status.incoming}
      <div class="card pairing">
        <div class="eyebrow">Pairing with {status.incoming.name}</div>
        <p>Enter this code on <b>{status.incoming.name}</b>:</p>
        <div class="code">{status.incoming.code.slice(0, 3)} {status.incoming.code.slice(3)}</div>
        <button class="btn" onclick={cancelPair}>Cancel</button>
      </div>
    {/if}

    {#if status.outgoing}
      <div class="card pairing">
        <div class="eyebrow">Pairing with {status.outgoing.name}</div>
        <p>Enter the 6-digit code shown on <b>{status.outgoing.name}</b>:</p>
        <form class="row" onsubmit={(e) => (e.preventDefault(), finishPair())}>
          <input class="code-input" bind:value={code} inputmode="numeric" maxlength="7" placeholder="123 456" autocomplete="one-time-code" />
          <button class="btn primary" type="submit" disabled={busy || code.replace(/\D/g, "").length !== 6}>Pair</button>
          <button class="btn" type="button" onclick={cancelPair}>Cancel</button>
        </form>
      </div>
    {/if}

    {#if p && (p.active || p.phase === "error" || (p.phase === "done" && p.finishedAt && Date.now() / 1000 - p.finishedAt < 30))}
      <div class="card progress" class:err={p.phase === "error"}>
        {#if p.active}
          <div class="row-between">
            <b>Syncing with {p.peer}</b>
            <span class="muted num">{p.phase === "copying" ? `${p.done} / ${p.total} · ${mb(p.bytes)} MB` : ""}</span>
          </div>
          <span class="bar"><span style:width="{p.phase === 'copying' ? pct : p.phase === 'merging' ? 100 : 8}%"></span></span>
          <span class="muted ellipsis">{p.phase === "copying" && p.current ? p.current : phaseText[p.phase] ?? ""}</span>
        {:else if p.phase === "error"}
          <b>Sync with {p.peer} didn't finish</b>
          <span class="muted">{p.error}</span>
        {:else}
          <b>Synced with {p.peer}</b>
          <span class="muted">{p.added ? `${p.added} new song${p.added === 1 ? "" : "s"} copied` : p.failed ? "No songs copied" : "Everything was already here"}, likes and playlists merged.</span>
          {#if p.failed}<span class="warn">{p.failed} song{p.failed === 1 ? "" : "s"} couldn't be copied: {p.error}</span>{/if}
        {/if}
      </div>
    {/if}

    {#if status.enabled}
      <h2 class="section-title">My devices</h2>
      {#each status.paired as d (d.id)}
        <div class="device">
          <span class="dot" class:on={d.online}></span>
          <div class="dev-text">
            <b class="ellipsis">{d.name}</b>
            <span class="muted">{d.online ? "On this Wi-Fi" : "Not nearby"} · {ago(d.lastSync)}</span>
          </div>
          <button class="btn primary" onclick={() => syncNow(d)} disabled={busy || !!p?.active || (!d.online && !d.addr)}>
            <Icon name="refresh" size={14} />Sync now
          </button>
          <button class="icon-btn" onclick={() => forget(d)} aria-label="Forget {d.name}" title="Forget"><Icon name="trash" size={15} /></button>
        </div>
      {:else}
        <p class="muted">No paired devices yet. Turn on sync on your other device too, then pair it below.</p>
      {/each}

      <h2 class="section-title">Nearby</h2>
      {#each unpairedNearby as d (d.id)}
        <div class="device">
          <span class="dot on"></span>
          <div class="dev-text"><b class="ellipsis">{d.name}</b><span class="muted">{d.addr}</span></div>
          <button class="btn" onclick={() => pair(d.addr!)} disabled={busy || !!status.outgoing}>Pair</button>
        </div>
      {:else}
        <p class="muted searching">Looking for Catlify on this Wi-Fi… Open Sync on the other device and turn it on.</p>
      {/each}

      <details class="manual">
        <summary>Not showing up? Pair by address</summary>
        <p class="muted">Some routers block device discovery. Type the address shown under the device name on the other device.</p>
        <form class="row" onsubmit={(e) => (e.preventDefault(), pair(manual))}>
          <input bind:value={manual} placeholder="192.168.1.20:52814" autocapitalize="off" spellcheck="false" />
          <button class="btn" type="submit" disabled={busy || !manual.trim()}>Pair</button>
        </form>
      </details>

      <p class="note">
        Songs from other devices are saved to <code>{status.syncDir}</code>. Keep Catlify open on both devices while they
        sync. Nothing is ever deleted: removing a song on one device doesn't remove it from the other.
      </p>
    {/if}
  {/if}
</div>

<style>
  .lede {
    color: var(--muted);
    margin: 4px 0 20px;
    max-width: 620px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 16px 18px;
    border-radius: var(--radius-lg);
    background: var(--surface);
    border: 1px solid var(--line);
    margin-bottom: 14px;
    max-width: 640px;
  }
  .card.me {
    flex-direction: row;
    align-items: center;
    gap: 16px;
  }
  .me-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 17px;
    font-weight: 650;
    text-align: left;
  }
  .name-input {
    font-size: 17px;
    font-weight: 650;
    background: var(--bg);
    border: 1px solid var(--violet);
    border-radius: 8px;
    padding: 4px 8px;
    outline: none;
  }
  .sub,
  .muted {
    color: var(--muted);
    font-size: 13px;
  }
  .switch {
    position: relative;
    flex: none;
    cursor: pointer;
  }
  .switch input {
    position: absolute;
    opacity: 0;
    inset: 0;
  }
  .track {
    display: block;
    width: 46px;
    height: 28px;
    border-radius: 99px;
    background: var(--line-strong);
    transition: background 0.2s ease;
  }
  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: #fff;
    transition: transform 0.2s ease;
  }
  .switch input:checked + .track {
    background: var(--coral);
  }
  .switch input:checked + .track .knob {
    transform: translateX(18px);
  }
  .pairing p {
    margin: 0;
  }
  .code {
    font-family: var(--mono);
    font-size: 40px;
    font-weight: 700;
    letter-spacing: 0.08em;
    margin: 6px 0 8px;
  }
  .row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .row input {
    flex: 1;
    min-width: 140px;
    height: 40px;
    padding: 0 12px;
    border-radius: var(--radius);
    border: 1px solid var(--line-strong);
    background: var(--bg);
    color: var(--text);
    outline: none;
  }
  .code-input {
    font-family: var(--mono);
    font-size: 20px !important;
    letter-spacing: 0.15em;
    max-width: 180px;
  }
  .row-between {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
  .bar {
    height: 5px;
    border-radius: 3px;
    background: var(--line-strong);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
  }
  .progress.err b {
    color: var(--coral);
  }
  .device {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 4px;
    border-bottom: 1px solid var(--line);
    max-width: 640px;
  }
  .dev-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--dim);
    flex: none;
  }
  .dot.on {
    background: #6fd39b;
    box-shadow: 0 0 8px rgba(111, 211, 155, 0.6);
  }
  .icon-btn {
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    color: var(--dim);
  }
  .icon-btn:hover {
    color: var(--coral);
  }
  .searching {
    animation: pulse 1.6s ease-in-out infinite alternate;
  }
  @keyframes pulse {
    from {
      opacity: 0.55;
    }
  }
  .manual {
    margin-top: 18px;
    max-width: 640px;
  }
  .manual summary {
    cursor: pointer;
    color: var(--muted);
    font-size: 13.5px;
  }
  .error,
  .warn {
    color: var(--coral);
  }
  .warn {
    font-size: 13px;
  }
  .note {
    margin-top: 24px;
    color: var(--dim);
    font-size: 12.5px;
    max-width: 640px;
  }
  code {
    font-family: var(--mono);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
