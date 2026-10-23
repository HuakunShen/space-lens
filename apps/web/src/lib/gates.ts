/**
 * View-availability gates for the workbench page, shared by the browser and
 * desktop flavors. The page is one component with no component tests, so
 * these pure functions are the only seam `node --test` can pin.
 *
 * Both hosts derive discovery from the protected scan report in memory, so a
 * coverage report no longer disables the discovery views on the desktop; the
 * gitignored view stays gated on full startup-disk scans, whose engine never
 * reads `.gitignore` content (mirrored by the backend's UnsupportedOperation).
 * Cleanup stays host-shaped: the desktop shell plans and executes against the
 * report, while the browser host's protected scans remain read-only.
 */
import type { Capabilities, ScanStatus } from '@space-lens/contract'

export interface DiscoveryGateInput {
  capabilities: Capabilities | null
  /** `Pick` of the workbench service; only the optional discover method matters. */
  service: { discover?: unknown } | null
}

/** Both backends declare discovery and derive it from the measured report. */
export function discoveryAvailable(input: DiscoveryGateInput): boolean {
  return Boolean(input.capabilities?.scan.discovery && input.service?.discover)
}

export interface AppAuditGateInput {
  capabilities: Capabilities | null
  /** `Pick` of the workbench service; only the optional appAudit method matters. */
  service: { appAudit?: unknown } | null
}

/** The desktop shell declares appAudit and serves it over Tauri IPC. */
export function appAuditAvailable(input: AppAuditGateInput): boolean {
  return Boolean(input.capabilities?.scan.appAudit && input.service?.appAudit)
}

export interface AppCleanupGateInput {
  capabilities: Capabilities | null
  /** `Pick` of the workbench service; only the V2 app cleanup methods matter. */
  service: { appCleanupPlan?: unknown; appCleanupExecute?: unknown } | null
}

/** Audit-generation cleanup needs both submit arms declared and implemented. */
export function appCleanupAvailable(input: AppCleanupGateInput): boolean {
  return Boolean(
    input.capabilities?.cleanup.appPlan &&
      input.capabilities?.cleanup.appExecute &&
      input.service?.appCleanupPlan &&
      input.service?.appCleanupExecute,
  )
}

export interface RevealGateInput {
  capabilities: Capabilities | null
  /** `Pick` of the workbench service; only the reveal method matters. */
  service: { revealAppLocation?: unknown } | null
}

/** Finder reveal needs the host flag and the host request method. */
export function revealAvailable(input: RevealGateInput): boolean {
  return Boolean(input.capabilities?.host.appLocationReveal && input.service?.revealAppLocation)
}

export interface PathRevealGateInput {
  capabilities: Capabilities | null
  /** `Pick` of the workbench service; only the reveal method matters. */
  service: { revealPath?: unknown } | null
}

/**
 * Opening a scanned path in the host's own file manager needs the host flag
 * and the host request method — both, like every other gate here.
 */
export function pathRevealAvailable(input: PathRevealGateInput): boolean {
  return Boolean(input.capabilities?.host.pathReveal && input.service?.revealPath)
}

export interface AppScanGateInput {
  capabilities: Capabilities | null
  /** `Pick` of the workbench service; only the optional startAppScan method matters. */
  service: { startAppScan?: unknown } | null
}

/** V3 app scans need the host flag and the submit arm. */
export function appScanAvailable(input: AppScanGateInput): boolean {
  return Boolean(input.capabilities?.scan.appScan && input.service?.startAppScan)
}

/** The engine skips `.gitignore` content reads for the startup-disk roots. */
const FULL_DISK_PATHS = ['/', '/System/Volumes/Data']

export function gitignoredAvailable(
  input: DiscoveryGateInput & {
    coverage: ScanStatus['coverage']
    scannedPaths: readonly string[]
  },
): boolean {
  return (
    discoveryAvailable(input) && !(input.coverage && input.scannedPaths.some((path) => FULL_DISK_PATHS.includes(path)))
  )
}

export function cleanupAvailable(input: {
  capabilities: Capabilities | null
  desktop: boolean
  coverage: ScanStatus['coverage']
}): boolean {
  if (!input.capabilities?.cleanup.execute) return false
  // The browser host refuses to plan against protected scans (read-only
  // server contract); the desktop shell owns the same report locally.
  return input.desktop || !input.coverage
}

export interface AiGateInput {
  /** Only the desktop shell ships the BYOK key store and the `ai_chat` proxy. */
  desktop: boolean
  /** Injected Tauri ports; null until `loadTauriPorts()` has resolved. */
  ports: { invoke?: unknown } | null
  /** The HTTP host declares and serves the advisor (`capabilities.ai` + `aiChat`). */
  hostAi: boolean
}

/**
 * The AI advisor is available either through the desktop shell's BYOK proxy
 * (its own key store and `invoke`) or through an HTTP host that serves an
 * operator-configured advisor. The invoke half covers the window between mount
 * and the async port load; a plain browser host without the routes stays
 * hidden.
 */
export function aiAvailable(input: AiGateInput): boolean {
  if (input.desktop && typeof input.ports?.invoke === 'function') return true
  return input.hostAi
}

export interface MemoryGateInput {
  capabilities: Capabilities | null
  /** `Pick` of the workbench service; the optional memory methods matter. */
  service: {
    memorySnapshot?: unknown
    memoryTerminatePlan?: unknown
    memoryTerminateExecute?: unknown
  } | null
}

/**
 * The Memory view needs the host to declare `memory.sample` *and* to serve the
 * snapshot read; older hosts and machine-mode hosts never do (§4.4, §6.1).
 */
export function memoryAvailable(input: MemoryGateInput): boolean {
  return Boolean(input.capabilities?.memory?.sample && input.service?.memorySnapshot)
}

/**
 * Quit / force quit need the `memory.terminate` flag plus both submit arms
 * (§4.4). Without it the table is read-only and the Quit actions are hidden.
 */
export function memoryTerminateAvailable(input: MemoryGateInput): boolean {
  return Boolean(
    input.capabilities?.memory?.terminate &&
      input.service?.memoryTerminatePlan &&
      input.service?.memoryTerminateExecute &&
      memoryAvailable(input),
  )
}
