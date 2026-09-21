import type { TauriPorts } from '@space-lens/client'

/**
 * The only place @tauri-apps/api is imported. Loaded dynamically so browser
 * builds never evaluate Tauri code; the ports are injected into the adapter
 * (packages/client) which stays transport-agnostic.
 */
export async function loadTauriPorts(): Promise<TauriPorts> {
  const core = await import('@tauri-apps/api/core')
  return {
    invoke: core.invoke,
    createChannel(onmessage) {
      const channel = new core.Channel<unknown>()
      channel.onmessage = onmessage
      return channel
    },
  }
}
