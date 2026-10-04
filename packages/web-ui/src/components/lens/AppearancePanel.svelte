<script lang="ts">
  import * as Sheet from '../ui/sheet/index'
  interface Props {
    open: boolean
    style: 'auto' | 'web' | 'macos' | 'windows'
    density: 'compact' | 'comfortable'
    mode: 'system' | 'light' | 'dark'
    automaticStyle: string
    onClose: () => void
    onStyle: (style: Props['style']) => void
    onDensity: (density: Props['density']) => void
    onMode: (mode: Props['mode']) => void
  }
  let { open, style, density, mode, automaticStyle, onClose, onStyle, onDensity, onMode }: Props = $props()
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
      <fieldset><legend>Interface style</legend><p>Automatic follows your platform. Choose any style on any device.</p>
        <div class="style-options">
          {#each [{id:'auto',label:'Automatic',detail:automaticStyle}, {id:'web',label:'Web',detail:'Clean and familiar'}, {id:'macos',label:'macOS',detail:'System type · soft controls'}, {id:'windows',label:'Windows',detail:'Segoe type · crisp controls'}] as option}
            <button type="button" class:selected={style === option.id} aria-pressed={style === option.id} onclick={() => onStyle(option.id === 'macos' ? 'macos' : option.id === 'windows' ? 'windows' : option.id === 'web' ? 'web' : 'auto')}><strong>{option.label}</strong><span>{option.detail}</span></button>
          {/each}
        </div>
      </fieldset>
      <fieldset><legend>Density</legend><div class="setting-options">
        <button type="button" class:selected={density === 'compact'} aria-pressed={density === 'compact'} onclick={() => onDensity('compact')}>Compact</button>
        <button type="button" class:selected={density === 'comfortable'} aria-pressed={density === 'comfortable'} onclick={() => onDensity('comfortable')}>Comfortable</button>
      </div></fieldset>
      <p class="settings-note">Your preferences are saved on this device.</p>
    </div>
  </Sheet.Content>
</Sheet.Root>
