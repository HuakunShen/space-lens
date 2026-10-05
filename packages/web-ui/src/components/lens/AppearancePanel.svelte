<script lang="ts">
  import * as Sheet from '../ui/sheet/index'
  interface UpdateStatusShape {
    state: 'idle' | 'checking' | 'available' | 'downloading' | 'uptodate' | 'error'
    note?: string
  }
  interface Props {
    open: boolean
    style: 'auto' | 'web' | 'macos' | 'windows'
    density: 'compact' | 'comfortable'
    mode: 'system' | 'light' | 'dark'
    automaticStyle: string
    appVersion?: string
    update?: UpdateStatusShape
    onClose: () => void
    onStyle: (style: Props['style']) => void
    onDensity: (density: Props['density']) => void
    onMode: (mode: Props['mode']) => void
    onCheckUpdates?: () => void
    onInstallUpdate?: () => void
  }
  let { open, style, density, mode, automaticStyle, appVersion, update, onClose, onStyle, onDensity, onMode, onCheckUpdates, onInstallUpdate }: Props = $props()
  const updateLabel: Record<UpdateStatusShape['state'], string> = {
    idle: 'Check for updates',
    checking: 'Checking…',
    available: 'Download and restart',
    downloading: 'Downloading…',
    uptodate: 'Check for updates',
    error: 'Check for updates',
  }
</script>
<Sheet.Root {open} onOpenChange={value => { if (!value) onClose() }}>
  <Sheet.Content class="settings-panel">
    <Sheet.Header><Sheet.Title>Settings</Sheet.Title><Sheet.Description>Make Space Lens feel at home.</Sheet.Description></Sheet.Header>
    <div class="settings-fields">
      <fieldset><legend>Appearance</legend><div class="setting-options">
        {#each ['system', 'light', 'dark'] as option}
          <button class:selected={mode === option} type="button" aria-pressed={mode === option} onclick={() => onMode(option === 'light' ? 'light' : option === 'dark' ? 'dark' : 'system')}>{option}</button>
        {/each}
      </div></fieldset>
      <fieldset><legend>Interface style</legend><p>Automatic uses the platform style in the desktop app on Windows and macOS; Web elsewhere.</p>
        <div class="style-options">
          {#each [{id:'auto',label:'Automatic',detail:automaticStyle}, {id:'web',label:'Web',detail:'Clean and familiar'}, {id:'macos',label:'macOS',detail:'System type · translucent chrome'}, {id:'windows',label:'Windows',detail:'Fluent style · accent blue'}] as option}
            <button type="button" class:selected={style === option.id} aria-pressed={style === option.id} onclick={() => onStyle(option.id === 'macos' ? 'macos' : option.id === 'windows' ? 'windows' : option.id === 'web' ? 'web' : 'auto')}><strong>{option.label}</strong><span>{option.detail}</span></button>
          {/each}
        </div>
      </fieldset>
      <fieldset><legend>Density</legend><div class="setting-options">
        <button type="button" class:selected={density === 'compact'} aria-pressed={density === 'compact'} onclick={() => onDensity('compact')}>Compact</button>
        <button type="button" class:selected={density === 'comfortable'} aria-pressed={density === 'comfortable'} onclick={() => onDensity('comfortable')}>Comfortable</button>
      </div></fieldset>
      {#if onCheckUpdates}
        <fieldset><legend>Updates</legend>
          <div class="update-row">
            <span class="update-version">{appVersion ? `Space Lens ${appVersion}` : 'Space Lens'}</span>
            {#if update?.state === 'available' || update?.state === 'downloading'}
              <button type="button" class="update-action" disabled={update.state === 'downloading'} onclick={() => onInstallUpdate?.()}>{updateLabel[update.state]}</button>
            {:else}
              <button type="button" class="update-action" disabled={update?.state === 'checking'} onclick={() => onCheckUpdates?.()}>{updateLabel[update?.state ?? 'idle']}</button>
            {/if}
          </div>
          {#if update?.state === 'uptodate'}
            <p class="settings-note">You are on the latest version.</p>
          {:else if update?.state === 'available'}
            <p class="settings-note">Version {update.note} is available.</p>
          {:else if update?.state === 'error'}
            <p class="settings-note">Update check failed{update.note ? `: ${update.note}` : ''}.</p>
          {/if}
        </fieldset>
      {/if}
      <p class="settings-note">Your preferences are saved on this device.</p>
    </div>
  </Sheet.Content>
</Sheet.Root>
