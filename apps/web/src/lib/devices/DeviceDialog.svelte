<script lang="ts">
  import {
    Check,
    HardDrive,
    LoaderCircle,
    RefreshCw,
    Server,
    TriangleAlert,
    Unplug,
  } from '@lucide/svelte'
  import { Button, Dialog, Input } from '@space-lens/web-ui'
  import type {
    RemoteDevice,
    RemoteInstallProgress,
    RemoteSshHost,
    RemoteSshListing,
    SshTarget,
  } from '@space-lens/client'

  interface Props {
    open: boolean
    devices: RemoteDevice[]
    activeDeviceId: string
    sshHosts: RemoteSshListing | null
    sshLoading: boolean
    connecting: boolean
    progress: RemoteInstallProgress | null
    error: string | null
    onClose: () => void
    onRefresh: () => void
    onSwitch: (deviceId: string) => void
    onDisconnect: (deviceId: string) => void
    onConnect: (target: SshTarget) => void
  }

  let {
    open,
    devices,
    activeDeviceId,
    sshHosts,
    sshLoading,
    connecting,
    progress,
    error,
    onClose,
    onRefresh,
    onSwitch,
    onDisconnect,
    onConnect,
  }: Props = $props()

  const STAGE_LABELS: Record<RemoteInstallProgress['stage'], string> = {
    probe: 'Checking the host',
    resolve: 'Resolving the agent build',
    download: 'Downloading Space Lens',
    verify: 'Verifying the download',
    'agent-install': 'Installing the agent',
    'host-check': 'Checking Node.js',
    'serve-start': 'Starting the service',
    tunnel: 'Opening the SSH tunnel',
    pair: 'Pairing',
    ready: 'Ready',
  }

  let manual = $state('')

  const manualTarget = $derived.by<SshTarget | null>(() => {
    const value = manual.trim()
    const match = /^([^@\s]+)@(\[[^\]\s]+\]|[^:\s]+)(?::(\d+))?$/.exec(value)
    if (!match) return null
    const port = match[3] === undefined ? null : Number(match[3])
    if (port !== null && (!Number.isInteger(port) || port < 1 || port > 65535)) return null
    return { kind: 'host', user: match[1], host: match[2], port }
  })

  const manualInvalid = $derived(manual.trim() !== '' && manualTarget === null)
  const busy = $derived(connecting)

  function aliasDevice(alias: string): RemoteDevice | undefined {
    return devices.find((device) => device.deviceId === `remote:${alias}`)
  }

  function useAlias(host: RemoteSshHost): void {
    const existing = aliasDevice(host.alias)
    if (existing) onSwitch(existing.deviceId)
    else onConnect({ kind: 'alias', alias: host.alias })
  }

  function submitManual(): void {
    const target = manualTarget
    if (target === null || busy) return
    const id =
      target.kind === 'host'
        ? `remote:${target.user}@${target.host}${target.port ? `:${target.port}` : ''}`
        : `remote:${target.alias}`
    const existing = devices.find((device) => device.deviceId === id)
    if (existing) onSwitch(existing.deviceId)
    else onConnect(target)
  }

  function detail(host: RemoteSshHost): string {
    const destination = host.hostName ?? host.alias
    const user = host.user ? `${host.user}@` : ''
    const port = host.port ? `:${host.port}` : ''
    return `${user}${destination}${port}`
  }
</script>

<Dialog.Root {open} onOpenChange={(value) => { if (!value && !busy) onClose() }}>
  <Dialog.Content class="max-w-xl">
    <Dialog.Header>
      <Dialog.Title>Devices</Dialog.Title>
      <Dialog.Description>
        Read a remote machine over your own OpenSSH connection. Space Lens installs into your
        home directory, keeps the service on loopback, and never stores passwords or keys.
      </Dialog.Description>
    </Dialog.Header>

    <div class="flex max-h-[60dvh] min-h-0 flex-col gap-5 overflow-y-auto px-5 pb-2">
      <section class="flex flex-col gap-2" aria-label="Connected devices">
        {#each devices as device (device.deviceId)}
          {@const active = device.deviceId === activeDeviceId}
          <div
            class="flex items-center gap-3 rounded-lg border px-3 py-2.5 {active
              ? 'border-primary/40 bg-muted/40'
              : ''}"
          >
            <span class="text-muted-foreground">
              {#if device.kind === 'local'}<HardDrive size={17} />{:else}<Server size={17} />{/if}
            </span>
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <strong class="truncate text-sm font-medium">{device.label}</strong>
                {#if active}<span
                    class="rounded-full bg-primary/10 px-1.5 py-0.5 text-[10px] font-semibold text-primary"
                    >In use</span
                  >{/if}
              </div>
              <span class="text-xs text-muted-foreground">
                {device.kind === 'local' ? 'This machine' : `${device.platform} · SSH`}
              </span>
            </div>
            {#if !active}
              <Button.Root
                variant="outline"
                size="sm"
                type="button"
                disabled={busy}
                onclick={() => onSwitch(device.deviceId)}>Switch</Button.Root
              >
            {/if}
            {#if device.kind === 'remote'}
              <Button.Root
                variant="ghost"
                size="icon-sm"
                type="button"
                aria-label={`Disconnect ${device.label}`}
                title={`Disconnect ${device.label}`}
                disabled={busy}
                onclick={() => onDisconnect(device.deviceId)}><Unplug size={15} /></Button.Root
              >
            {/if}
          </div>
        {/each}
      </section>

      {#if progress !== null}
        <div class="flex items-start gap-2 rounded-lg border bg-muted/40 px-3 py-2.5" role="status">
          <LoaderCircle size={16} class="mt-0.5 shrink-0 animate-spin text-muted-foreground" />
          <div>
            <p class="text-sm font-medium">{STAGE_LABELS[progress.stage]}</p>
            <p class="text-xs text-muted-foreground">{progress.message}</p>
          </div>
        </div>
      {/if}

      {#if error !== null}
        <p class="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive" role="alert">
          {error}
        </p>
      {/if}

      <section class="flex flex-col gap-2" aria-label="SSH configuration hosts">
        <div class="flex items-center justify-between">
          <div>
            <h3 class="text-sm font-medium">SSH hosts</h3>
            <p class="text-xs text-muted-foreground">
              Concrete aliases from <code>~/.ssh/config</code>.
            </p>
          </div>
          <Button.Root
            variant="ghost"
            size="icon-sm"
            type="button"
            aria-label="Refresh SSH hosts"
            title="Refresh SSH hosts"
            disabled={sshLoading || busy}
            onclick={onRefresh}
          >
            {#if sshLoading}<LoaderCircle size={15} class="animate-spin" />{:else}<RefreshCw size={15} />{/if}
          </Button.Root>
        </div>

        {#if sshHosts === null && sshLoading}
          <p class="text-xs text-muted-foreground">Reading your SSH configuration…</p>
        {:else if sshHosts !== null && sshHosts.hosts.length === 0}
          <p class="rounded-lg border border-dashed px-3 py-3 text-xs text-muted-foreground">
            No concrete hosts in {sshHosts.configPath}. Add one there, or connect with
            <code>user@host</code> below.
          </p>
        {:else if sshHosts !== null}
          <div class="flex flex-col gap-1.5">
            {#each sshHosts.hosts as host (host.alias)}
              {@const connected = aliasDevice(host.alias)}
              <div class="flex items-center gap-3 rounded-lg border px-3 py-2">
                <span class="text-muted-foreground"><Server size={15} /></span>
                <div class="min-w-0 flex-1">
                  <strong class="block truncate text-sm font-medium">{host.alias}</strong>
                  <span class="block truncate text-xs text-muted-foreground">{detail(host)}</span>
                </div>
                <Button.Root
                  variant={connected ? 'secondary' : 'outline'}
                  size="sm"
                  type="button"
                  disabled={busy}
                  onclick={() => useAlias(host)}
                >
                  {#if connected}<Check size={14} />{:else}<Server size={14} />{/if}
                  {connected ? 'Switch' : 'Connect'}
                </Button.Root>
              </div>
            {/each}
          </div>
        {/if}

        {#if sshHosts !== null && !sshHosts.complete}
          <div class="flex items-start gap-2 text-xs text-amber-600 dark:text-amber-400">
            <TriangleAlert size={14} class="mt-0.5 shrink-0" />
            <div>
              <p>The listing may be incomplete.</p>
              {#each sshHosts.problems as problem}<p class="break-all">{problem}</p>{/each}
            </div>
          </div>
        {/if}
      </section>

      <section class="flex flex-col gap-2" aria-label="Connect to another host">
        <div>
          <h3 class="text-sm font-medium">Another host</h3>
          <p class="text-xs text-muted-foreground">
            Requires OpenSSH access and Node.js 20 or newer on the remote machine.
          </p>
        </div>
        <form
          class="flex items-start gap-2"
          onsubmit={(event) => {
            event.preventDefault()
            submitManual()
          }}
        >
          <div class="min-w-0 flex-1">
            <Input.Root
              bind:value={manual}
              placeholder="user@host or user@host:2222"
              aria-label="SSH destination"
              autocomplete="off"
              spellcheck={false}
              disabled={busy}
            />
            {#if manualInvalid}
              <p class="mt-1 text-xs text-muted-foreground">
                Use <code>user@host</code> or <code>user@host:port</code>.
              </p>
            {/if}
          </div>
          <Button.Root type="submit" size="sm" disabled={busy || manualTarget === null}>
            Connect
          </Button.Root>
        </form>
      </section>
    </div>

    <Dialog.Footer>
      <p class="mr-auto text-xs text-muted-foreground">
        Remote hosts are read-only in this beta: scans, discovery, app audits, and memory.
      </p>
      <Button.Root variant="outline" type="button" disabled={busy} onclick={onClose}>
        Close
      </Button.Root>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
