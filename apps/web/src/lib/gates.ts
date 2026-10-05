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
