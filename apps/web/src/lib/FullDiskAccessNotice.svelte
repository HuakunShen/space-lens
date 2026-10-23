<script lang="ts">
  import { Button } from '@space-lens/web-ui'
  import { describeError } from './workbench.svelte.ts'
  import { openFullDiskAccessSettings, readFullDiskAccess, type FullDiskAccess } from './permissions.ts'

  /**
   * Shown on the desktop app while Full Disk Access is off. Without it the scan
   * cannot read protected folders (Mail, Safari, parts of Library), so the
   * measured total comes out lower than the disk's used space. macOS offers no
   * prompt for this grant; the button opens the right Settings pane and the
   * notice re-checks when the window gets focus back.
   */
  const DISMISS_KEY = 'space-lens.fda.dismissed'

  let status = $state<FullDiskAccess>('unsupported')
  let dismissed = $state(readDismissed())
  let error = $state<string | null>(null)

  function readDismissed(): boolean {
    try {
      return localStorage.getItem(DISMISS_KEY) === '1'
    } catch {
      return false
    }
  }

  async function refresh(): Promise<void> {
    try {
      status = await readFullDiskAccess()
      // A granted app that is later revoked should ask again.
      if (status === 'granted') {
        dismissed = false
        try {
          localStorage.removeItem(DISMISS_KEY)
        } catch {
          // Storage blocked: the notice just stays hidden while granted.
        }
      }
    } catch {
      status = 'unsupported'
    }
  }

  $effect(() => {
    void refresh()
    const onFocus = () => void refresh()
    window.addEventListener('focus', onFocus)
    return () => window.removeEventListener('focus', onFocus)
  })

  async function openSettings(): Promise<void> {
    error = null
    try {
      await openFullDiskAccessSettings()
    } catch (failed) {
      error = describeError(failed)
    }
  }

  function dismiss(): void {
    dismissed = true
    try {
      localStorage.setItem(DISMISS_KEY, '1')
    } catch {
      // Dismissal then lasts until the next launch.
    }
  }
</script>

{#if status === 'denied' && !dismissed}
  <section
    class="mx-4 my-3 flex flex-col gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-sm"
    aria-label="Full Disk Access is off"
  >
    <div class="flex items-start justify-between gap-3">
      <div class="grid gap-1">
        <strong>Full Disk Access is off</strong>
        <p class="text-xs text-muted-foreground">
          Space Lens cannot read protected folders such as Mail, Safari and parts of Library, so the scan will
          measure less than the disk's used space. It only reads file sizes and names — never file contents — and
          never downloads iCloud or Photos originals.
        </p>
      </div>
      <button
        type="button"
        class="shrink-0 text-xs text-muted-foreground hover:text-foreground"
        onclick={dismiss}
        aria-label="Dismiss">Not now</button
      >
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <Button.Root size="sm" onclick={() => void openSettings()}>Open System Settings</Button.Root>
      <span class="text-xs text-muted-foreground"
        >Turn on Space Lens under Privacy &amp; Security → Full Disk Access, then restart the app and scan again.</span
      >
    </div>
    {#if error}<p class="text-xs text-destructive" role="alert">{error}</p>{/if}
  </section>
{/if}
