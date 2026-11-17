<script lang="ts">
  import { base } from '$app/paths'
  import {
    DiskChart, ChildList, MemoryPressureCard, MemoryBreakdownBar, ProcessTable,
    formatBytes,
  } from '@space-lens/web-ui'
  import type { TreeNodeSummary } from '@space-lens/contract'
  import type { ChartMode, TreemapDensity } from '@space-lens/web-ui'
  import type { TableQuery } from '@space-lens/web-ui'
  import { makeDemoTree, makeDemoMemorySnapshot, findDemoNode } from '../lib/demo-fixture.js'

  const tree = makeDemoTree()
  const memory = makeDemoMemorySnapshot()
  const releaseUrl = 'https://github.com/HuakunShen/space-lens/releases/latest'

  let view = $state<'disk' | 'memory'>('disk')
  let focusId = $state(tree.id)
  let chartMode = $state<ChartMode>('sunburst')
  let density = $state<TreemapDensity>('nested')
  let hoveredId = $state<string | null>(null)
  let inspectedId = $state<string | null>(null)
  let collectedIds = $state<Set<string>>(new Set())
  let memoryQuery = $state<TableQuery>({ sort: 'footprint', desc: true, filter: 'all', search: '' })
  let processSelection = $state<Set<string>>(new Set())
  let processName = $state<string | null>(null)

  const focusedTree = $derived(findDemoNode(tree, focusId) ?? tree)
  const hoveredNode = $derived(hoveredId ? findDemoNode(tree, hoveredId) : null)
  const inspectedNode = $derived((inspectedId ? findDemoNode(tree, inspectedId) : null) ?? hoveredNode ?? focusedTree)
  const collectedBytes = $derived([...collectedIds].reduce((total, id) => total + (findDemoNode(tree, id)?.size ?? 0), 0))
  const breadcrumb = $derived(focusedTree.id.split(':').map((_, index, parts) =>
    findDemoNode(tree, parts.slice(0, index + 1).join(':'))).filter((node): node is NonNullable<typeof node> => node !== null))

  function openNode(node: TreeNodeSummary): void {
    inspectedId = node.id
    if (node.hasChildren) {
      focusId = node.id
      hoveredId = null
    }
  }
  function back(): void {
    if (focusId === tree.id) return
    focusId = focusId.split(':').slice(0, -1).join(':')
    hoveredId = null
  }
  function collect(node: TreeNodeSummary): void {
    // Collect is a visualization-only selection. No planner, delete or API runs.
    if (!findDemoNode(tree, node.id)) return
    collectedIds = new Set([...collectedIds, node.id])
  }
  function uncollect(node: TreeNodeSummary): void {
    const next = new Set(collectedIds)
    next.delete(node.id)
    collectedIds = next
  }
  function reset(): void {
    focusId = tree.id
    chartMode = 'sunburst'
    density = 'nested'
    hoveredId = null
    inspectedId = null
    collectedIds = new Set()
    memoryQuery = { sort: 'footprint', desc: true, filter: 'all', search: '' }
    processSelection = new Set()
    processName = null
  }
</script>

<svelte:head>
  <title>Space Lens — See what your machine is really using</title>
  <meta name="description" content="Explore disk usage, storage cleanup candidates, and memory pressure through interactive native-grade visualizations. Space Lens is an open-source, local-first workbench for macOS, Windows and Linux." />
  <meta property="og:title" content="Space Lens — Your disk and memory, in focus" />
  <meta property="og:description" content="A visual, local-first disk and memory workbench with a live, safely simulated Svelte demo." />
  <link rel="canonical" href="https://space-lens.huakun.tech/" />
</svelte:head>

<div class="site-shell" id="top">
  <div class="grid-haze" aria-hidden="true"></div>
  <header class="site-nav">
    <a class="brand" href={`${base}/`} aria-label="Space Lens home"><span class="brand-mark"><span></span><span></span><span></span></span> Space<span>Lens</span><i>.</i></a>
    <nav aria-label="Main navigation">
      <a href="#demo">Live demo</a>
      <a href="#features">Features</a>
      <a href="https://github.com/HuakunShen/space-lens">Source ↗</a>
    </nav>
    <a class="top-download" href={releaseUrl}>Download <span>↗</span></a>
  </header>

  <main>
    <section class="hero" aria-labelledby="hero-heading">
      <div class="hero-kicker"><span class="live-beacon"></span> YOUR MACHINE, DECODED <i></i> OPEN SOURCE</div>
      <h1 id="hero-heading">See the space.<br/><em>Understand the story.</em></h1>
      <p class="hero-description">Your disk has a thousand folders. Your memory has a hundred processes. Space Lens turns the chaos into a clear, explorable picture — so you know what matters before you act.</p>
      <div class="hero-actions">
        <a href="#demo" class="action-primary">Explore the live demo <span>↘</span></a>
        <a href={releaseUrl} class="action-secondary">Get Space Lens <span>↗</span></a>
      </div>
      <div class="hero-proof"><span>◉ NO CLOUD SCANS</span><span>✳ REAL SVELTE COMPONENTS</span><span>⌘ NATIVE &amp; WEB</span></div>
    </section>

    <section class="showcase" id="demo" aria-label="Interactive Space Lens demonstration">
      <div class="section-intro">
        <div><p class="overline">/ TRY IT FOR YOURSELF</p><h2>Go beyond<br/><em>the screenshot.</em></h2></div>
        <div class="section-description"><p>Everything inside this frame is interactive. Browse the real visualization components used by Space Lens — with fictional files and processes that never touch your computer.</p><span><span class="live-beacon"></span> SANDBOX · LOCAL TO YOUR BROWSER</span></div>
      </div>

      <div class="app-window">
        <div class="window-topbar">
          <div class="traffic-lights" aria-hidden="true"><i></i><i></i><i></i></div>
          <span class="window-title">◉ space lens <small>— demo workspace</small></span>
          <div class="window-indicator"><span class="live-beacon"></span> SAMPLE DATA</div>
        </div>
        <div class="window-main">
          <aside class="app-sidebar">
            <div class="side-label">YOUR WORKSPACE</div>
            <button type="button" class:active={view === 'disk'} onclick={() => view = 'disk'} data-testid="disk-tab"><span>◫</span> Disk Space <small>135 GB</small></button>
            <button type="button" class:active={view === 'memory'} onclick={() => view = 'memory'} data-testid="memory-tab"><span>≋</span> Memory <small>32 GB</small></button>
            <div class="side-line"></div>
            <div class="side-label">PRESET SCAN</div>
            <div class="scan-card"><span>▣</span><div><strong>Macintosh HD</strong><small>Fictional local volume</small></div><span class="scan-ok">●</span></div>
            <div class="scan-card subdued"><span>▣</span><div><strong>Developer workspace</strong><small>Preloaded example</small></div></div>
            <div class="sidebar-lower"><p>Everything here is a demonstration. Disk usage and process lists are fictional.</p><div><span class="live-beacon"></span> 100% OFFLINE</div></div>
          </aside>

          <section class="app-workspace" aria-label={view === 'disk' ? 'Disk visualization' : 'Memory visualization'}>
            <div class="workspace-heading"><div><span class="workspace-eyebrow">{view === 'disk' ? 'STORAGE ANALYSIS' : 'SYSTEM MEMORY'}</span><h3>{view === 'disk' ? 'Understand your storage.' : 'Watch your memory.'}</h3></div><button class="demo-reset" type="button" onclick={reset} data-testid="reset-demo">↻ Reset demo</button></div>
            {#if view === 'disk'}
              <div class="workspace-summary"><div><small>SCANNED SIZE</small><strong>{formatBytes(tree.size)}</strong></div><div><small>VISIBLE FOLDERS</small><strong>{tree.children.length} <em>top-level</em></strong></div><div><small>REVIEW SELECTION</small><strong>{formatBytes(collectedBytes)}</strong></div></div>
              <div class="breadcrumb-row">{#each breadcrumb as crumb, index (crumb.id)}{#if index > 0}<span>›</span>{/if}<button type="button" onclick={() => openNode(crumb)}>{crumb.name}</button>{/each}</div>
              <div class="disk-content">
                <div class="chart-pane" data-testid="real-disk-chart">
                  <DiskChart
                    tree={focusedTree}
                    focusNode={focusedTree}
                    mode={chartMode}
                    onModeChange={(mode) => chartMode = mode}
                    {density}
                    onDensityChange={(next) => density = next}
                    {hoveredId}
                    {hoveredNode}
                    {collectedIds}
                    onHover={(id) => hoveredId = id}
                    onOpen={openNode}
                    onContext={(node) => collectedIds.has(node.id) ? uncollect(node) : collect(node)}
                    canGoBack={focusedTree.id !== tree.id}
                    onBack={back}
                    softBubbles={false}
                  />
                </div>
                <aside class="file-pane" aria-label="Directory children">
                  <div class="file-pane-title"><strong>In this folder</strong><small>{focusedTree.children.length} items</small></div>
                  <div class="file-list" data-testid="real-directory-list">
                    <ChildList
                      items={focusedTree.children}
                      totalSize={focusedTree.size}
                      {hoveredId}
                      {collectedIds}
                      onHover={(id) => hoveredId = id}
                      onOpen={openNode}
                      onCollect={collect}
                      onRemove={uncollect}
                    />
                  </div>
                  <div class="inspector-mini"><small>CURRENT INSPECTION</small><strong>{inspectedNode.name}</strong><span>{formatBytes(inspectedNode.size)} · sample directory</span></div>
                </aside>
              </div>
            {:else}
              <div class="memory-content">
                <div class="memory-cards" data-testid="real-memory-cards"><MemoryPressureCard snapshot={memory} /><MemoryBreakdownBar breakdown={memory.breakdown} /></div>
                <div class="memory-table-head"><div><strong>Top memory consumers</strong><span>Real sortable Svelte process table, fictional processes</span></div><span>{memory.groups.length} processes</span></div>
                <div class="memory-table" data-testid="real-process-table">
                  <ProcessTable
                    snapshot={memory}
                    query={memoryQuery}
                    selection={processSelection}
                    onSelect={(key) => { const next = new Set(processSelection); if (next.has(key)) next.delete(key); else next.add(key); processSelection = next; }}
                    onOpen={(group) => processName = group.name}
                    onQueryChange={(query) => memoryQuery = query}
                  />
                </div>
                <div class="memory-note">{processName ? `Inspecting ${processName} · ` : ''}Demo process selection only. No kill/force-quit actions are connected.</div>
              </div>
            {/if}
            <div class="workspace-status"><span><span class="live-beacon"></span> {view === 'disk' ? 'SCAN COMPLETE · PRESET DATA' : 'MEMORY SNAPSHOT · PRESET DATA'}</span><span>REAL SPACE LENS UI COMPONENTS</span></div>
          </section>
        </div>
      </div>
      <div class="below-demo"><span>THE REAL COMPONENTS. FAKE FILES. ZERO RISK.</span><a href="https://github.com/HuakunShen/space-lens/tree/main/packages/web-ui">Explore shared Svelte UI ↗</a></div>
    </section>

    <section class="features" id="features">
      <div class="feature-intro"><p class="overline">/ ONE CLEAR VIEW</p><h2>Tools that make<br/><em>the invisible visible.</em></h2></div>
      <div class="feature-grid">
        <article><div class="feature-number">01 / DISK</div><div class="feature-art rings-art"><i></i><i></i><i></i></div><h3>Find what grew.</h3><p>Navigate folders visually. Flip between Sunburst, Treemap, Icicle, Bubbles and Strips to see your disk from another angle.</p></article>
        <article><div class="feature-number">02 / MEMORY</div><div class="feature-art bars-art"><i></i><i></i><i></i><i></i><i></i></div><h3>Know what is running.</h3><p>Inspect system pressure, compare memory consumers and understand where your resources are going.</p></article>
        <article><div class="feature-number">03 / CONTROL</div><div class="feature-art blocks-art"><i></i><i></i><i></i></div><h3>Review before acting.</h3><p>Identify cleanup candidates with context. Your machine stays in control; the public sandbox never deletes anything.</p></article>
      </div>
    </section>

    <section class="closing"><p class="overline">/ SEE YOUR SYSTEM DIFFERENTLY</p><h2>Less guessing.<br/><em>More breathing room.</em></h2><p>Open source. Built with Rust and Svelte. Available across multiple frontends.</p><div class="hero-actions"><a class="action-primary" href={releaseUrl}>Download Space Lens ↗</a><a class="action-secondary" href="https://github.com/HuakunShen/space-lens">View source on GitHub ↗</a></div></section>
  </main>
  <footer class="site-footer"><a class="brand" href={`${base}/`}><span class="brand-mark"><span></span><span></span><span></span></span> Space<span>Lens</span><i>.</i></a><span>Made for the curious, built for the careful.</span><div><a href="#demo">Demo</a><a href={releaseUrl}>Releases</a><a href="https://github.com/HuakunShen/space-lens">GitHub ↗</a></div></footer>
</div>
