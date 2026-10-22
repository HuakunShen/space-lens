// Runs the same scan through two ScanManager implementations and diffs every
// browse, discovery and cleanup-planning result. From packages/host:
//   SYNTH_EXACT=1 SYNTH_NODES=300000 node --import tsx bench/compare.mts <before src> src local
//   node --import tsx bench/compare.mts <before src> src native <served directory>
// <before src> must sit inside packages/host so its imports resolve, e.g.
//   mkdir .bench-before && git archive <rev>:packages/host/src | tar -x -C .bench-before
import { pathToFileURL } from 'node:url'
import { isDeepStrictEqual } from 'node:util'
const [beforeSrc, afterSrc, mode, nativeRoot] = process.argv.slice(2)
const load = async (src: string) => ({
  ScanManager: (await import(pathToFileURL(`${src}/scan-store.ts`).href)).ScanManager,
  EventRing: (await import(pathToFileURL(`${src}/events.ts`).href)).EventRing,
})
const root = mode === 'local' ? '/synthetic/root' : nativeRoot
// The native engine walks in parallel, so equal-size siblings come back in run-dependent order.
const scrub = (value: unknown): unknown =>
  JSON.parse(
    JSON.stringify(value, (key, v) => {
      if (['scanId', 'generatedAt', 'planId', 'createdAt', 'expiresAt'].includes(key)) return undefined
      if (mode === 'native' && key === 'children' && Array.isArray(v))
        return [...v].sort((a, b) => b.size - a.size || a.id.localeCompare(b.id))
      return v
    }),
  )

async function run(src: string) {
  const { ScanManager, EventRing } = await load(src)
  const manager = new ScanManager({
    events: new EventRing(),
    trash: { trash: async () => new Map() },
    roots: [root],
    cleanupMode: 'trash',
    maxConcurrent: 2,
    maxTotal: 2,
    localScanBin: new URL('./synthetic-scanner.cjs', import.meta.url).pathname,
  })
  const session = await manager.start(
    {
      paths: [root],
      ignoreHidden: mode === 'native',
      respectGitignore: true,
      ignoredMode: 'summarize',
      localOnly: mode === 'local',
    },
    undefined,
  )
  await manager.wait(session.scanId)
  const scanId = session.scanId
  const out: Record<string, unknown> = {}
  const status = manager.status(scanId)
  out.status = { ...status, updatedAt: undefined, scanId: undefined }
  // walk directories breadth-first through the public API to collect ids
  const ids: string[] = []
  const queue = [status.rootIds[0]]
  while (queue.length > 0 && ids.length < 4000) {
    const id = queue.shift()!
    ids.push(id)
    const page = manager.children(scanId, id, 0, 1000, 'size')
    for (const item of page.items)
      if (item.hasChildren) queue.push(item.id)
      else if (ids.length % 7 === 0) ids.push(item.id)
  }
  out.ids = ids.length
  for (const [position, id] of ids.entries()) {
    if (position % 5 !== 0) continue
    out[`slice:${id}`] = manager.slice(scanId, id, 2, mode === 'native' ? 100000 : 12)
    for (const sort of ['size', 'name', 'path'] as const)
      out[`children:${sort}:${id}`] = manager.children(scanId, id, 3, 40, sort)
  }
  out.rootSlice = manager.slice(scanId, status.rootIds[0], 3, mode === 'native' ? 100000 : 30)
  out.missing = (() => {
    try {
      return manager.slice(scanId, 'ffffffffffffffffffffffff', 1, 1)
    } catch (e: any) {
      return e.problem?.code
    }
  })()
  const discoveredIds: string[] = []
  for (const kind of ['large-files', 'caches', 'gitignored'] as const) {
    for (const [minSize, offset, limit] of [
      [0, 0, 50],
      [0, 137, 20],
      [4096, 0, 30],
      [1e8, 3, 10],
      [0, 10_000_000, 5],
    ]) {
      try {
        const page = await manager.discover({ scanId, kind, minSize, offset, limit })
        out[`discover:${kind}:${minSize}:${offset}`] = page
        for (const item of page.items.slice(0, 3))
          discoveredIds.push(item.node.id, ...(item.parentId ? [item.parentId] : []))
      } catch (error: any) {
        out[`discover:${kind}:${minSize}:${offset}`] = error.problem?.code ?? String(error)
      }
    }
  }
  for (const id of discoveredIds) {
    try {
      out[`dslice:${id}`] = manager.slice(scanId, id, 1, mode === 'native' ? 100000 : 5)
    } catch (error: any) {
      out[`dslice:${id}`] = error.problem?.code
    }
    try {
      out[`plan:${id}`] = manager.plan(scanId, [id])
    } catch (error: any) {
      out[`plan:${id}`] = error.problem?.code ?? String(error)
    }
  }
  try {
    out.planMany = manager.plan(scanId, discoveredIds)
  } catch (error: any) {
    out.planMany = error.problem?.code ?? String(error)
  }
  await manager.close()
  return scrub(out) as Record<string, unknown>
}

const before = await run(beforeSrc)
const after = await run(afterSrc)
let differences = 0
for (const key of new Set([...Object.keys(before), ...Object.keys(after)])) {
  if (!isDeepStrictEqual(before[key], after[key])) {
    differences += 1
    if (differences <= 5) {
      const a = JSON.stringify(before[key]),
        b = JSON.stringify(after[key])
      let i = 0
      while (a[i] === b[i]) i++
      console.log(
        'DIFF',
        key,
        '\n before',
        a.slice(Math.max(0, i - 300), i + 200),
        '\n after ',
        b.slice(Math.max(0, i - 300), i + 200),
      )
    }
  }
}
console.log(`${Object.keys(before).length} queries compared, ${differences} differences`)
const plans = Object.entries(after).filter(([key]) => key.startsWith('plan:'))
console.log(
  `plans ok ${plans.filter(([, v]) => typeof v === 'object').length}, errors ${JSON.stringify([...new Set(plans.filter(([, v]) => typeof v !== 'object').map(([, v]) => v))])}`,
)
