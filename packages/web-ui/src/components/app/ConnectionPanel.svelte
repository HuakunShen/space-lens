<script lang="ts">
  import { useLensI18n } from '../../lib/i18n/context.svelte'
  /**
   * Full-width connection surface shown when no backend session exists.
   * Pure presentation: the page owns discovery, tickets, and the exchange.
   */
  interface Props {
    phase: 'idle' | 'connecting' | 'failed'
    resolvedUrl: string | null
    sameOrigin: boolean
    hosted: boolean
    ticket: string
    password: string
    message: string | null
    onBaseUrl: (url: string) => void
    onTicket: (ticket: string) => void
    onPassword: (password: string) => void
    onConnect: () => void
  }

  let {
    phase,
    resolvedUrl,
    sameOrigin,
    hosted,
    ticket,
    password,
    message,
    onBaseUrl,
    onTicket,
    onPassword,
    onConnect,
  }: Props = $props()
  const i18n = useLensI18n()
</script>

<div class="mx-auto flex min-h-screen w-full max-w-xl flex-col items-center justify-center gap-6 p-6 pt-14">
  <div class="flex flex-col items-center gap-2 text-center">
    <h1 class="text-2xl font-semibold tracking-tight">Space Lens</h1>
    <p class="text-muted-foreground text-sm">{i18n.t('lens.connection.intro')}</p>
  </div>

  <form
    class="bg-card w-full rounded-xl border p-5 shadow-sm"
    onsubmit={(event) => {
      event.preventDefault()
      onConnect()
    }}
  >
    <div class="flex flex-col gap-4">
      <label class="flex flex-col gap-1.5">
        <span class="text-sm font-medium">{i18n.t('lens.connection.address')}</span>
        <input
          class="border-input bg-background h-9 rounded-md border px-3 font-mono text-sm"
          placeholder="http://127.0.0.1:9420"
          value={resolvedUrl ?? ''}
          oninput={(event) => onBaseUrl(event.currentTarget.value)}
        />
        <span class="text-muted-foreground text-xs">
          {i18n.t(sameOrigin ? 'lens.connection.sameOrigin' : 'lens.connection.remote')} — {i18n.t('lens.connection.printedBy')}
        </span>
      </label>

      <label class="flex flex-col gap-1.5">
        <span class="text-sm font-medium">{i18n.t('lens.connection.ticket')}</span>
        <input
          class="border-input bg-background h-9 rounded-md border px-3 font-mono text-sm"
          placeholder={i18n.t('lens.connection.ticketHint')}
          value={ticket}
          oninput={(event) => onTicket(event.currentTarget.value)}
        />
      </label>

      {#if hosted}
        <label class="flex flex-col gap-1.5">
          <span class="text-sm font-medium">{i18n.t('lens.connection.password')}</span>
          <input
            class="border-input bg-background h-9 rounded-md border px-3 text-sm"
            type="password"
            placeholder={i18n.t('lens.connection.passwordHint')}
            value={password}
            oninput={(event) => onPassword(event.currentTarget.value)}
          />
        </label>
      {/if}

      {#if message}
        <p class="text-destructive text-sm" role="alert">{message}</p>
      {/if}

      <button
        class="bg-primary text-primary-foreground hover:bg-primary/90 h-9 rounded-md px-4 text-sm font-medium disabled:opacity-50"
        type="submit"
        disabled={phase === 'connecting'}
      >
        {i18n.t(phase === 'connecting' ? 'lens.connection.connecting' : 'lens.connection.connect')}
      </button>
    </div>
  </form>
</div>
