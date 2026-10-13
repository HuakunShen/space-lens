/**
 * Shared fetch-once cache for app icons, used by the app storage cards and
 * the drawer header. A failed logo stays failed and never re-enqueues, and
 * at most four icon fetches are in flight at once.
 */

export interface AppIconState {
  status: 'loading' | 'ready' | 'failed'
  url: string | null
}

const ICON_CONCURRENCY = 4

const results = new Map<string, AppIconState>()
const pending = new Map<string, Promise<string | null>>()
const queue: string[] = []
const queued = new Set<string>()
let inFlight = 0
let fetcher: ((appPath: string) => Promise<string | null>) | null = null
const listeners = new Set<() => void>()

function emit(): void {
  for (const notify of listeners) notify()
}

/** Re-render hook for Svelte cards: call inside `$effect`, use the cleanup. */
export function subscribeIcons(notify: () => void): () => void {
  listeners.add(notify)
  return () => {
    listeners.delete(notify)
  }
}

/** The last known state for a path, if it was ever requested. */
export function readIcon(path: string): AppIconState | undefined {
  return results.get(path)
}

function pump(): void {
  const active = fetcher
  if (!active) return
  while (inFlight < ICON_CONCURRENCY && queue.length > 0) {
    const path = queue.shift() as string
    if (pending.has(path) || results.get(path)?.status === 'ready') continue
    const run = active
    inFlight += 1
    const task = Promise.resolve()
      .then(() => run(path))
      .then(
        (url) => {
          results.set(path, url ? { status: 'ready', url } : { status: 'failed', url: null })
          emit()
          return url
        },
        () => {
          results.set(path, { status: 'failed', url: null })
          emit()
          return null
        },
      )
      .finally(() => {
        inFlight -= 1
        pending.delete(path)
        pump()
      })
    pending.set(path, task)
  }
}

/**
 * Enqueues one icon fetch unless the path already resolved (ready or
 * failed). CLI entries and empty paths never reach this function — the
 * card renders the initials avatar for them directly.
 */
export function ensureIcon(path: string, fetch: (appPath: string) => Promise<string | null>): void {
  if (!fetcher) fetcher = fetch
  if (results.has(path) || queued.has(path)) return
  queued.add(path)
  results.set(path, { status: 'loading', url: null })
  queue.push(path)
  pump()
}

/** Test hook: drops every cached outcome so fetches run again. */
export function resetIconCache(): void {
  results.clear()
  pending.clear()
  queue.length = 0
  queued.clear()
  inFlight = 0
  fetcher = null
}

/**
 * Visibility-gated icon loading. App icons cost a backend parse (icns → PNG
 * over IPC), so a 400-app grid must not enqueue all of them at mount: the
 * card registers itself here and the fetch is only requested when the
 * element enters (or nears) the viewport. One shared IntersectionObserver
 * serves every card; hitting the viewport detaches the element for good.
 */
const iconRequests = new Map<Element, () => void>()
let iconObserver: IntersectionObserver | null = null

function iconObserverInstance(): IntersectionObserver {
  if (!iconObserver) {
    iconObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue
          const request = iconRequests.get(entry.target)
          if (request) {
            iconRequests.delete(entry.target)
            iconObserver?.unobserve(entry.target)
            request()
          }
        }
      },
      // Prefetch a screen ahead so scrolling rarely meets an empty avatar.
      { rootMargin: '300px' },
    )
  }
  return iconObserver
}

/** Observes `element`; calls `request` once when it becomes visible. */
export function whenVisible(element: Element, request: () => void): () => void {
  iconRequests.set(element, request)
  iconObserverInstance().observe(element)
  return () => {
    iconRequests.delete(element)
    iconObserverInstance().unobserve(element)
  }
}
