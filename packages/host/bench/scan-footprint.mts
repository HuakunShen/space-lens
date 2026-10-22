// Heap footprint of a protected scan of SYNTH_NODES synthetic entries, through
// the real ScanManager: streamed ingest, browse, then all three discovery views.
// From packages/host:
//   SYNTH_NODES=6400000 node --expose-gc --max-old-space-size=16384 --import tsx bench/scan-footprint.mts [src dir]
// Pass another checkout's packages/host/src to measure an older index.
import { fileURLToPath, pathToFileURL } from 'node:url'
const src = process.argv[2] ?? fileURLToPath(new URL('../src', import.meta.url))
const { ScanManager } = await import(pathToFileURL(`${src}/scan-store.ts`).href)
const { EventRing } = await import(pathToFileURL(`${src}/events.ts`).href)
const mib = (n: number) => (n / 1048576).toFixed(0)
const snap = (label: string) => {
  globalThis.gc!()
  globalThis.gc!()
  const m = process.memoryUsage()
  console.log(
    `${label.padEnd(28)} heapUsed ${mib(m.heapUsed)} MiB  external ${mib(m.external)} MiB  arrayBuffers ${mib(m.arrayBuffers)} MiB  rss ${mib(m.rss)} MiB`,
  )
}
let peakHeap = 0
const timer = setInterval(() => {
  peakHeap = Math.max(peakHeap, process.memoryUsage().heapUsed + process.memoryUsage().arrayBuffers)
}, 50)
snap('start')
const root = '/synthetic/root'
const manager = new ScanManager({
  events: new EventRing(),
  trash: { trash: async () => new Map() },
  roots: [root],
  cleanupMode: 'none',
  maxConcurrent: 1,
  maxTotal: 2,
  localScanBin: new URL('./synthetic-scanner.cjs', import.meta.url).pathname,
})
const t0 = Date.now()
const session = await manager.start(
  { paths: [root], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize', localOnly: true },
  undefined,
)
await manager.wait(session.scanId)
const status = manager.status(session.scanId)
console.log(`state ${status.state} ${status.message} entries ${status.entriesScanned} in ${Date.now() - t0} ms`)
snap('indexed')
const t1 = Date.now()
const slice = manager.slice(session.scanId, status.rootIds[0], 3, 50)
const kids = manager.children(session.scanId, status.rootIds[0], 0, 100, 'name')
console.log(
  `slice children ${slice.tree.children.length} page ${kids.items.length}/${kids.total} in ${Date.now() - t1} ms`,
)
const t2 = Date.now()
const results: string[] = []
for (const kind of ['large-files', 'caches', 'gitignored'] as const) {
  const page = await manager.discover({ scanId: session.scanId, kind, minSize: 0, offset: 0, limit: 5 })
  results.push(
    `${kind} total ${page.total} size ${page.totalSize} first ${page.items[0]?.node.path} ${page.items[0]?.node.size}`,
  )
}
console.log(`discovery in ${Date.now() - t2} ms`)
for (const line of results) console.log('  ' + line)
snap('after discovery')
clearInterval(timer)
console.log(`sampled peak heapUsed+arrayBuffers ${mib(peakHeap)} MiB`)
await manager.close()
