import type { ChatPort } from './types.ts'

/**
 * The advisor's host-facing config port.
 *
 * `AiConfig` is the frozen, read-back shape a host returns; `AiConfigInput` is
 * the write shape (an omitted `apiKey` keeps the stored key, `null` clears it,
 * a string replaces it). `AiChatPort` pairs that config surface with the one
 * non-streaming `ChatPort` the agent loop consumes. The concrete adapters (the
 * Tauri proxy and the browser `#ai-dev` port) live with their host; this module
 * is host-free.
 */

export interface AiConfig {
  baseUrl: string
  model: string
  /** True when a key is stored. The key itself is never handed back. */
  hasKey: boolean
  /** Sends `thinking: { type: 'disabled' }`; much faster on reasoning models (GLM, DeepSeek). */
  disableThinking: boolean
}

export interface AiConfigInput {
  baseUrl: string
  model: string
  /** Omitted keeps the stored key; `null` clears it; a string replaces it. */
  apiKey?: string | null
  /** Omitted keeps the stored value. */
  disableThinking?: boolean
}

/** Config + chat, sharing one lazily loaded set of host ports. */
export interface AiChatPort {
  /** The one non-streaming completion the agent loop consumes. */
  chat: ChatPort
  getConfig(): Promise<AiConfig>
  setConfig(input: AiConfigInput): Promise<AiConfig>
}

/** Reads the frozen config reply leniently; a stale host backfills defaults. */
export function readConfig(value: unknown): AiConfig {
  if (typeof value !== 'object' || value === null) return { baseUrl: '', model: '', hasKey: false, disableThinking: false }
  const record = value as { baseUrl?: unknown; model?: unknown; hasKey?: unknown; disableThinking?: unknown }
  return {
    baseUrl: typeof record.baseUrl === 'string' ? record.baseUrl : '',
    model: typeof record.model === 'string' ? record.model : '',
    hasKey: record.hasKey === true,
    disableThinking: record.disableThinking === true,
  }
}
