<script lang="ts">
  /**
   * One banner for every non-normal state. Pure presentation; the page maps
   * its situation onto a kind.
   */
  type BannerState = 'loading' | 'empty' | 'stale' | 'truncated' | 'disconnected' | 'error' | 'info'

  interface Props {
    state: BannerState
    title: string
    detail?: string
    action?: { label: string; onClick: () => void }
  }

  let { state, title, detail, action }: Props = $props()

  const styles: Record<BannerState, string> = {
    loading: 'border-border text-muted-foreground',
    empty: 'border-border text-muted-foreground',
    stale: 'border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-400',
    truncated: 'border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-400',
    disconnected: 'border-red-500/40 bg-red-500/10 text-red-700 dark:text-red-400',
    error: 'border-red-500/40 bg-red-500/10 text-red-700 dark:text-red-400',
    info: 'border-border text-muted-foreground',
  }
</script>

<div class="rounded-lg border p-3 text-sm {styles[state]}" role={state === 'error' || state === 'disconnected' ? 'alert' : undefined}>
  <p class="font-medium">{title}</p>
  {#if detail}
    <p class="text-muted-foreground mt-0.5 text-xs">{detail}</p>
  {/if}
  {#if action}
    <button class="mt-2 rounded-md border px-2.5 py-1 text-xs font-medium hover:bg-accent" type="button" onclick={action.onClick}>
      {action.label}
    </button>
  {/if}
</div>
