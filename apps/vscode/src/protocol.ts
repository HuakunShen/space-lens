// Shared by the extension host and the webview. The webview never sees a
// token, a URL, or fetch: it names intentions with ids and gets typed answers
// back.
import type { ScanSession, ScanStatus, TreeSlice, ChildrenPage, CleanupPlan } from '@space-lens/contract'

export interface BridgeRequest {
  id: number
  kind: 'pair' | 'scan.start' | 'scan.status' | 'tree.slice' | 'tree.children'
  paths?: string[]
  scanId?: string
  nodeId?: string
  depth?: number
  maxChildrenPerNode?: number
  offset?: number
  limit?: number
  sort?: 'size' | 'name' | 'path'
}

export type BridgeAnswerPayload =
  | { kind: 'pair'; scanId: string | null }
  | { kind: 'scan.start'; session: ScanSession }
  | { kind: 'scan.status'; status: ScanStatus }
  | { kind: 'tree.slice'; slice: TreeSlice }
  | { kind: 'tree.children'; page: ChildrenPage }

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

export type BridgeMessage = BridgeReply | BridgePush

export type CleanupPlanDTO = CleanupPlan
