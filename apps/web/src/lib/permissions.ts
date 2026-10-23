/**
 * Full Disk Access on the desktop app. macOS has no prompt for it: the app can
 * only say whether it is in effect and open the Settings pane. The Tauri API is
 * imported lazily so a browser build never evaluates it.
 */
export type FullDiskAccess = 'granted' | 'denied' | 'unknown' | 'unsupported'

const STATUSES: readonly FullDiskAccess[] = ['granted', 'denied', 'unknown', 'unsupported']

export function parseFullDiskAccess(value: unknown): FullDiskAccess {
  return STATUSES.find((status) => status === value) ?? 'unknown'
}

export async function readFullDiskAccess(): Promise<FullDiskAccess> {
  const { invoke } = await import('@tauri-apps/api/core')
  return parseFullDiskAccess(await invoke('fda_status'))
}

export async function openFullDiskAccessSettings(): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('fda_open_settings')
}
