import { getVersion } from '@tauri-apps/api/app'

export type UpdateState = 'idle' | 'checking' | 'available' | 'downloading' | 'uptodate' | 'error'
export interface UpdateStatus {
  state: UpdateState
  note?: string
}

interface PendingUpdate {
  version: string
  downloadAndInstall: () => Promise<void>
}
let pending: PendingUpdate | null = null

/** The embedded app version, or null outside the Tauri desktop shell. */
export async function currentAppVersion(): Promise<string | null> {
  try {
    return (await getVersion()) || null
  } catch {
    return null
  }
}

export async function checkForAppUpdate(report: (status: UpdateStatus) => void): Promise<void> {
  report({ state: 'checking' })
  try {
    const { check } = await import('@tauri-apps/plugin-updater')
    const update = await check()
    if (!update) {
      pending = null
      report({ state: 'uptodate' })
      return
    }
    pending = update
    report({ state: 'available', note: update.version })
  } catch (error) {
    pending = null
    report({ state: 'error', note: error instanceof Error ? error.message : String(error) })
  }
}

export async function installAppUpdate(report: (status: UpdateStatus) => void): Promise<void> {
  if (!pending) return
  report({ state: 'downloading' })
  try {
    await pending.downloadAndInstall()
    const { relaunch } = await import('@tauri-apps/plugin-process')
    await relaunch()
  } catch (error) {
    report({ state: 'error', note: error instanceof Error ? error.message : String(error) })
  }
}
