<script lang="ts">
  import { Monitor, Laptop, AppWindow, CircleCheck } from '@lucide/svelte'
  import * as Sheet from '../ui/sheet/index'
  interface UpdateStatusShape {
    state: 'idle' | 'checking' | 'available' | 'downloading' | 'uptodate' | 'error'
    note?: string
  }
  interface Props {
    open: boolean
    style: 'auto' | 'web' | 'macos' | 'windows' | 'linux'
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
  let {
    open,
    style,
    density,
    mode,
    automaticStyle,
    appVersion,
    update,
    onClose,
    onStyle,
    onDensity,
    onMode,
    onCheckUpdates,
    onInstallUpdate,
  }: Props = $props()
  const styleOptions: { id: Props['style']; label: string; detail: string; icon: typeof Monitor }[] = [
    { id: 'auto', label: 'Automatic', detail: 'Use this platform', icon: Monitor },
    { id: 'web', label: 'Web', detail: 'Clean and familiar', icon: AppWindow },
    { id: 'macos', label: 'macOS', detail: 'Compact toolbar · source lists', icon: Laptop },
    { id: 'windows', label: 'Windows', detail: 'WinUI navigation · command bars', icon: AppWindow },
    { id: 'linux', label: 'Linux', detail: 'GNOME header bar · roomy controls', icon: Monitor },
  ]
  const segmentClass =
    'macos:rounded-[5px] macos:border-0 macos:py-1 macos:text-[13px] macos:data-[selected=true]:bg-background macos:data-[selected=true]:shadow-sm windows:relative windows:min-h-9 windows:border-0 windows:data-[selected=true]:bg-card windows:data-[selected=true]:shadow-none windows:data-[selected=true]:after:absolute windows:data-[selected=true]:after:bottom-0 windows:data-[selected=true]:after:left-1/4 windows:data-[selected=true]:after:w-1/2 windows:data-[selected=true]:after:h-0.5 windows:data-[selected=true]:after:bg-primary linux:min-h-10 linux:rounded-lg linux:border-0 linux:text-sm linux:data-[selected=true]:bg-background linux:data-[selected=true]:shadow-sm focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-inset'
  const updateLabel: Record<UpdateStatusShape['state'], string> = {
    idle: 'Check for updates',
    checking: 'Checking…',
    available: 'Download and restart',
    downloading: 'Downloading…',
    uptodate: 'Check for updates',
    error: 'Check for updates',
  }
</script>

<Sheet.Root
  {open}
  onOpenChange={(value) => {
    if (!value) onClose()
  }}
>
  <Sheet.Content
    class="settings-panel macos:p-6 windows:sm:max-w-[460px] windows:p-7 linux:sm:max-w-[440px] linux:bg-background linux:p-6"
  >
    <Sheet.Header
      ><Sheet.Title>Settings</Sheet.Title><Sheet.Description>Make Space Lens feel at home.</Sheet.Description
      ></Sheet.Header
    >
    <div class="settings-fields windows:space-y-3 linux:space-y-3">
      <fieldset
        class="windows:mb-3 windows:rounded-lg windows:border windows:bg-card windows:p-4 linux:mb-3 linux:rounded-xl linux:border linux:bg-card linux:p-4"
      >
        <legend class="windows:float-left windows:w-full windows:text-sm linux:float-left linux:w-full linux:text-sm"
          >Appearance</legend
        >
        <div
          class="setting-options macos:overflow-visible macos:gap-0.5 macos:border-0 macos:bg-muted macos:p-0.5 windows:clear-both windows:rounded-[4px] windows:border-0 windows:bg-muted linux:clear-both linux:gap-1 linux:border-0 linux:bg-muted linux:p-1"
        >
          {#each ['system', 'light', 'dark'] as option}
            <button
              class={segmentClass}
              data-selected={mode === option}
              class:selected={mode === option}
              type="button"
              aria-pressed={mode === option}
              onclick={() => onMode(option === 'light' ? 'light' : option === 'dark' ? 'dark' : 'system')}
              >{option}</button
            >
          {/each}
        </div>
      </fieldset>
      <fieldset
        class="windows:mb-3 windows:rounded-lg windows:border windows:bg-card windows:p-4 linux:mb-3 linux:rounded-xl linux:border linux:bg-card linux:p-4"
      >
        <legend class="windows:float-left windows:w-full windows:text-sm linux:float-left linux:w-full linux:text-sm"
          >Interface style</legend
        >
        <p>Automatic uses macOS, Windows or Linux (GNOME) in the desktop app; Web in a browser.</p>
        <div
          class="style-options macos:grid-cols-1 macos:gap-0 macos:overflow-hidden macos:rounded-lg macos:border windows:clear-both windows:grid-cols-1 linux:clear-both linux:grid-cols-1 linux:gap-0 linux:overflow-hidden linux:rounded-xl linux:border"
        >
          {#each styleOptions as option}
            <button
              type="button"
              data-selected={style === option.id}
              class="flex items-center gap-3 macos:rounded-none macos:border-0 macos:border-b macos:last:border-b-0 macos:bg-card macos:data-[selected=true]:bg-accent macos:data-[selected=true]:shadow-none windows:min-h-16 windows:rounded-[4px] windows:bg-background windows:data-[selected=true]:border-primary windows:data-[selected=true]:shadow-none linux:min-h-16 linux:rounded-none linux:border-0 linux:border-b linux:last:border-b-0 linux:bg-card linux:data-[selected=true]:bg-accent linux:data-[selected=true]:shadow-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-inset"
              class:selected={style === option.id}
              aria-pressed={style === option.id}
              onclick={() => onStyle(option.id)}
            >
              <option.icon size={19} class="shrink-0 text-muted-foreground" />
              <div class="min-w-0 flex-1">
                <strong class="linux:text-sm windows:text-sm">{option.label}</strong><span
                  >{option.id === 'auto' ? automaticStyle : option.detail}</span
                >
              </div>
              <CircleCheck size={17} class={style === option.id ? 'shrink-0 text-primary' : 'invisible shrink-0'} />
            </button>
          {/each}
        </div>
      </fieldset>
      <fieldset
        class="windows:mb-3 windows:rounded-lg windows:border windows:bg-card windows:p-4 linux:mb-3 linux:rounded-xl linux:border linux:bg-card linux:p-4"
      >
        <legend class="windows:float-left windows:w-full windows:text-sm linux:float-left linux:w-full linux:text-sm"
          >Density</legend
        >
        <div
          class="setting-options macos:overflow-visible macos:gap-0.5 macos:border-0 macos:bg-muted macos:p-0.5 windows:clear-both windows:rounded-[4px] windows:border-0 windows:bg-muted linux:clear-both linux:gap-1 linux:border-0 linux:bg-muted linux:p-1"
        >
          <button
            type="button"
            class={segmentClass}
            data-selected={density === 'compact'}
            class:selected={density === 'compact'}
            aria-pressed={density === 'compact'}
            onclick={() => onDensity('compact')}>Compact</button
          >
          <button
            type="button"
            class={segmentClass}
            data-selected={density === 'comfortable'}
            class:selected={density === 'comfortable'}
            aria-pressed={density === 'comfortable'}
            onclick={() => onDensity('comfortable')}>Comfortable</button
          >
        </div>
      </fieldset>
      {#if onCheckUpdates}
        <fieldset
          class="windows:mb-3 windows:rounded-lg windows:border windows:bg-card windows:p-4 linux:mb-3 linux:rounded-xl linux:border linux:bg-card linux:p-4"
        >
          <legend class="windows:float-left windows:w-full windows:text-sm linux:float-left linux:w-full linux:text-sm"
            >Updates</legend
          >
          <div class="update-row">
            <span class="update-version">{appVersion ? `Space Lens ${appVersion}` : 'Space Lens'}</span>
            {#if update?.state === 'available' || update?.state === 'downloading'}
              <button
                type="button"
                class="update-action focus-visible:ring-2 focus-visible:ring-ring macos:bg-(--control) linux:rounded-lg linux:bg-(--control)"
                disabled={update.state === 'downloading'}
                onclick={() => onInstallUpdate?.()}>{updateLabel[update.state]}</button
              >
            {:else}
              <button
                type="button"
                class="update-action focus-visible:ring-2 focus-visible:ring-ring macos:bg-(--control) linux:rounded-lg linux:bg-(--control)"
                disabled={update?.state === 'checking'}
                onclick={() => onCheckUpdates?.()}>{updateLabel[update?.state ?? 'idle']}</button
              >
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
