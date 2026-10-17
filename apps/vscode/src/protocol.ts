// Shared by the extension host and the webview. The webview never sees a
// token, a URL, or fetch: it names intentions with ids and gets typed answers
// back.
import type { ScanSession, ScanStatus, TreeSlice, ChildrenPage, CleanupPlan } from '@space-lens/contract'

export interface BridgeRequest {
  id: number
  kind:
    | 'pair'
    | 'scan.start'
    | 'scan.status'
    | 'tree.slice'
    | 'tree.children'
    | 'ai.config'
    | 'ai.setConfig'
    | 'ai.chat'
    | 'ai.chatStream'
    | 'ai.cancel'
    | 'reveal'
  paths?: string[]
  scanId?: string
  /** For `ai.cancel`: the request id of the stream to abort. */
  runId?: number
  nodeId?: string
  depth?: number
  maxChildrenPerNode?: number
  offset?: number
  limit?: number
  sort?: 'size' | 'name' | 'path'
  ai?: { baseUrl?: string; model?: string; apiKey?: string | null; disableThinking?: boolean }
  messages?: unknown[]
  tools?: unknown[]
  path?: string
}

export type BridgeAnswerPayload =
  | { kind: 'pair'; scanId: string | null }
  | { kind: 'scan.start'; session: ScanSession }
  | { kind: 'scan.status'; status: ScanStatus }
  | { kind: 'tree.slice'; slice: TreeSlice }
  | { kind: 'tree.children'; page: ChildrenPage }
  | {
      kind: 'ai.config'
      config: { baseUrl: string; model: string; hasKey: boolean; disableThinking: boolean; managed: false }
    }
  | { kind: 'ai.chat'; completion: unknown }
  | { kind: 'ai.chatStream'; done: true }
  | { kind: 'ai.cancel'; cancelled: boolean }
  | { kind: 'reveal'; revealed: boolean }

export interface BridgeAnswer {
  id: number
  ok: true
  answer: BridgeAnswerPayload
}

export interface BridgeRefusal {
  id: number
  ok: false
  problem: { code: string; message: string }
}

export type BridgeReply = BridgeAnswer | BridgeRefusal

export interface BridgePush {
  kind: 'push'
  status: ScanStatus
}

/**
 * One raw text slice of a streamed answer, posted extension -> webview while an
 * `ai.chatStream` request is in flight. `id` is the request id of that stream.
 */
export interface BridgeStreamChunk {
  kind: 'ai.chunk'
  id: number
  text: string
}

export type BridgeMessage = BridgeReply | BridgePush | BridgeStreamChunk

export type CleanupPlanDTO = CleanupPlan
