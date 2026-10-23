<script lang="ts">
  import { describeError } from './workbench.svelte.ts'
  import {
    openFullDiskAccessSettings,
    readAppBundlePath,
    readFullDiskAccess,
    relaunchApp,
    type FullDiskAccess,
  } from './permissions.ts'

  /**
   * A quiet line at the bottom of the sidebar while Full Disk Access is off.
   * Nothing is blocked without it: the scan just cannot read protected folders
   * (Mail, Safari, parts of Library), so it measures less than the disk's used
   * space. macOS has no prompt for this grant, so the details open Settings and
   * name the exact app that must be listed. The check repeats while the window
   * is in front, and a grant only applies after a relaunch, hence the button.
   */
  let status = $state<FullDiskAccess>('unsupported')
  let appPath = $state<string | null>(null)
  let open = $state(false)
  let error = $state<string | null>(null)

  async function refresh(): Promise<void> {
    try {
      status = await readFullDiskAccess()
    } catch {
      status = 'unsupported'
    }
  }

  $effect(() => {
    void refresh()
    void readAppBundlePath().then((path) => (appPath = path), () => {})
    const onFocus = () => void refresh()
    window.addEventListener('focus', onFocus)
    // While Settings is open the user may tick the box; notice it without a click.
    const timer = setInterval(() => {
      if (status === 'denied' && document.hasFocus()) void refresh()
    }, 3000)
    return () => {
      window.removeEventListener('focus', onFocus)
      clearInterval(timer)
    }
  })

  async function run(action: () => Promise<void>): Promise<void> {
    error = null
    try {
      await action()
    } catch (failed) {
      error = describeError(failed)
    }
  }
</script>

{#if status === 'denied'}
  <div class="fda" role="note">
    <button type="button" class="fda-summary" aria-expanded={open} onclick={() => (open = !open)}>
      <i class="fda-dot"></i>
      <span>Full Disk Access is off</span>
      <span class="fda-more">{open ? 'Hide' : 'Details'}</span>
    </button>
    {#if open}
      <div class="fda-body">
        <p>
          Optional. Without it Space Lens skips protected folders such as Mail and Safari and measures less than the
          disk's used space. It reads only file sizes and names, and never downloads iCloud or Photos files.
        </p>
        {#if appPath}
          <p>
            Turn on <strong>this copy</strong> in Privacy &amp; Security → Full Disk Access:
            <code title={appPath}>{appPath}</code>
          </p>
        {/if}
        <p>A new grant takes effect after Space Lens restarts. Rebuilt or re-signed copies need it again.</p>
        <div class="fda-actions">
          <button type="button" onclick={() => void run(openFullDiskAccessSettings)}>Open Settings</button>
          <button type="button" onclick={() => void run(relaunchApp)}>Restart</button>
        </div>
        {#if error}<p class="fda-error" role="alert">{error}</p>{/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .fda {
    border: 1px solid color-mix(in oklab, #f59e0b 35%, transparent);
    border-radius: 0.5rem;
    background: color-mix(in oklab, #f59e0b 8%, transparent);
    font-size: 0.75rem;
  }
  .fda-summary {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.6rem;
    text-align: left;
  }
  .fda-dot {
    width: 0.4rem;
    height: 0.4rem;
    flex: none;
    border-radius: 50%;
    background: #f59e0b;
  }
  .fda-more {
    margin-left: auto;
    color: var(--muted-foreground);
  }
  .fda-body {
    display: grid;
    gap: 0.4rem;
    border-top: 1px solid color-mix(in oklab, #f59e0b 25%, transparent);
    padding: 0.5rem 0.6rem 0.6rem;
    color: var(--muted-foreground);
  }
  .fda-body p {
    margin: 0;
  }
  .fda-body code {
    display: block;
    margin-top: 0.15rem;
    overflow-wrap: anywhere;
    color: var(--foreground);
    font-size: 0.6875rem;
  }
  .fda-actions {
    display: flex;
    gap: 0.4rem;
  }
  .fda-actions button {
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    padding: 0.2rem 0.6rem;
    color: var(--foreground);
  }
  .fda-actions button:hover {
    background: var(--muted);
  }
  .fda-error {
    color: var(--destructive);
  }
</style>
