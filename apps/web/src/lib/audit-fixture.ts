/**
 * Browser-side UI fixture for the App Storage views, enabled by opening the
 * app with `#audit-fixture` in the URL. The HTTP host has no audit endpoints
 * yet (plan Future Work), so this canned report is how the embeddable web
 * flavor is developed and screenshot-tested without the Tauri shell. It never
 * touches the filesystem: icons resolve to null (letter avatars) and cleanup
 * planning fails with a friendly problem, exactly like a read-only host.
 */
import type { AppAuditReport } from '@space-lens/contract'

const location = (
  path: string,
  label: string,
  classification:
    | 'application'
    | 'cache'
    | 'versions'
    | 'logs'
    | 're-downloadable'
    | 'user-data'
    | 'support',
  size: number,
  purgeable: boolean,
  comment?: string,
) => ({
  path,
  label,
  classification,
  purgeable,
  comment: comment ?? null,
  size,
  source: 'catalog' as const,
})

const GiB = 1024 ** 3
const MiB = 1024 ** 2

export const fixtureReport: AppAuditReport = {
  platform: 'macos',
  generatedAt: new Date().toISOString(),
  auditId: 'audit_fixture0000000000000000',
  apps: [
    {
      id: 'codex-cli',
      name: 'Codex CLI',
      kind: 'cli',
      source: 'catalog',
      bundleId: null,
      appPath: null,
      installedSize: 0,
      totalSize: 24 * GiB + 500 * MiB,
      locations: [
        location('~/.codex/worktrees'.replace('~', '/Users/demo'), 'Agent worktrees', 'user-data', 11.2 * GiB, true, 'Git worktrees created by agent runs. May contain uncommitted work — review before deleting.'),
        location('/Users/demo/.codex/sessions', 'Chat session logs', 'user-data', 9.05 * GiB, true, 'One JSONL per conversation. Months of sessions can reach tens of GB — back up first.'),
        location('/Users/demo/.codex', 'Codex data root', 'support', 3.67 * GiB, false, 'Config, auth and history databases; the itemized subfolders above are the big movers.'),
        location('/Users/demo/.codex/packages/standalone/releases', 'Historical CLI releases', 'versions', 634 * MiB, true, 'Every release the updater ever kept (~250 MB each). Only the current one is used.'),
      ],
    },
    {
      id: 'claude-desktop',
      name: 'Claude',
      kind: 'app',
      source: 'catalog',
      bundleId: 'com.anthropic.claudefordesktop',
      appPath: '/Applications/Claude.app',
      installedSize: 812 * MiB,
      totalSize: 1.11 * GiB,
      locations: [
        location('/Users/demo/Library/Application Support/Claude/claude-code-vm', 'Sandbox VM images', 're-downloadable', 296 * MiB, true, 'Linux VM images used for sandboxed code execution. Re-downloaded on next use.'),
        location('/Users/demo/Library/Application Support/Claude', 'Claude data root', 'support', 40 * MiB, false),
      ],
    },
    {
      id: 'com.apple.Xcode',
      name: 'Xcode',
      kind: 'app',
      source: 'heuristic',
      bundleId: 'com.apple.dt.Xcode',
      appPath: '/Applications/Xcode.app',
      installedSize: 3.6 * GiB,
      totalSize: 3.62 * GiB,
      locations: [location('/Users/demo/Library/Developer/Xcode/DerivedData', 'Derived data', 'cache', 20 * MiB, false)],
    },
    {
      id: 'npm-cache',
      name: 'npm cache',
      kind: 'cli',
      source: 'catalog',
      bundleId: null,
      appPath: null,
      installedSize: 0,
      totalSize: 1.47 * GiB,
      locations: [location('/Users/demo/.npm/_cacache', 'Package download cache', 'cache', 1.47 * GiB, true, 'Safe to clear (npm cache clean --force); contents re-download on demand.')],
    },
    {
      id: 'pnpm-store',
      name: 'pnpm store',
      kind: 'cli',
      source: 'catalog',
      bundleId: null,
      appPath: null,
      installedSize: 0,
      totalSize: 6.4 * GiB,
      locations: [location('/Users/demo/Library/pnpm', 'pnpm store and global bin', 'support', 6.4 * GiB, false, 'Content-addressable package store. Old-generation stores can be reclaimed with pnpm store prune.')],
    },
  ],
  totalApps: 5,
  truncated: false,
}

export function auditFixtureActive(): boolean {
  return typeof window !== 'undefined' && window.location.hash.includes('audit-fixture')
}

/** Tiny placeholder logos so fixture screenshots look representative; real
 * logos come from the Tauri appIcon channel (icns -> png), which a browser
 * fixture cannot reach. Keyed by the fixture report's appPath values. */
const fixtureIcons: Record<string, string> = {
  '/Applications/Claude.app':
    'data:image/svg+xml;utf8,' +
    encodeURIComponent(
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#d97757"/><g stroke="#fff" stroke-width="4" stroke-linecap="round"><path d="M32 14v36M14 32h36M19 19l26 26M45 19L19 45"/></g></svg>',
    ),
  '/Applications/Xcode.app':
    'data:image/svg+xml;utf8,' +
    encodeURIComponent(
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#1478f0"/><circle cx="32" cy="32" r="17" fill="none" stroke="#fff" stroke-width="4"/><path d="M20 44 44 20" stroke="#fff" stroke-width="5" stroke-linecap="round"/></svg>',
    ),
}

/** Installs fixture capabilities + service into the workbench state. */
export function installAuditFixture(workbench: {
  capabilities: unknown
  service: unknown
}): void {
  workbench.capabilities = {
    apiMajor: 1,
    contractVersion: 'fixture',
    scan: { start: false, cancel: false, discovery: false, appAudit: true, maxConcurrent: 0 },
    cleanup: { plan: false, execute: false, mode: 'trash', appPlan: true, appExecute: true },
    host: { folderPicker: false, appLocationReveal: true },
    icloud: 'unavailable',
  }
  const delay = <T>(value: T): Promise<T> => new Promise((resolve) => setTimeout(() => resolve(value), 350))
  workbench.service = {
    baseUrl: 'fixture',
    health: () => delay({ status: 'ok', serviceInstanceId: 'fixture', apiMajor: 1, contractVersion: 'fixture' }),
    capabilities: () => delay(workbench.capabilities),
    roots: () => delay({ roots: [] }),
    startScan: () => Promise.reject(new Error('fixture host cannot scan')),
    scanStatus: () => Promise.reject(new Error('fixture host cannot scan')),
    cancelScan: () => Promise.reject(new Error('fixture host cannot scan')),
    treeSlice: () => Promise.reject(new Error('fixture host cannot scan')),
    children: () => Promise.reject(new Error('fixture host cannot scan')),
    plan: () => Promise.reject(new Error('fixture host cannot plan engine cleanup')),
    execute: () => Promise.reject(new Error('fixture host cannot execute cleanup')),
    appAudit: () => delay(JSON.parse(JSON.stringify(fixtureReport))),
    appIcon: (body: { appPath: string }) => {
      const url = fixtureIcons[body.appPath]
      // Contract shape: AppIcon = { dataUrl } (the page wrapper unwraps it).
      return delay(url ? { dataUrl: url } : null)
    },
    appCleanupPlan: (body: { paths: string[] }) =>
      Promise.reject({
        problem: {
          code: 'InvalidRequest',
          message: 'fixture host cannot verify plans — sizes and classifications are illustrative only',
        },
        paths: body.paths,
      }),
    appCleanupExecute: () => Promise.reject({ problem: { code: 'InvalidRequest', message: 'fixture host cannot execute' } }),
    revealAppLocation: () => delay({ revealed: false }),
  }
}
