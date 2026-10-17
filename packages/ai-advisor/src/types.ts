import type { ChildrenPage, CollectorEntry, DiscoveryItem, ScanState, TreeNodeSummary } from '@space-lens/contract'

/**
 * Slice A — agent core types.
 *
 * The chat format is a minimal subset of the OpenAI chat completions wire
 * shape (system/user/assistant/tool messages, `tool_calls` on assistant
 * turns). The real transport lands later in Rust: `ChatPort` is injected so
 * the loop is testable without a network, and the desktop implementation will
 * forward to `invoke('ai_chat')`.
 */

export type ChatRole = 'system' | 'user' | 'assistant' | 'tool'

export interface ChatToolCall {
  id: string
  type: 'function'
  function: {
    /** Tool name, matching one of `AI_TOOLS`. */
    name: string
    /** Raw JSON string as produced by the model; may be malformed. */
    arguments: string
  }
}

export interface ChatMessage {
  role: ChatRole
  /** Null on assistant turns that only carry `tool_calls`. */
  content: string | null
  tool_calls?: ChatToolCall[]
  /** Set on `role: 'tool'` messages: the call this result answers. */
  tool_call_id?: string
  name?: string
}

export interface ChatFunctionSchema {
  name: string
  description: string
  parameters: {
    type: 'object'
    properties: Record<string, unknown>
    required?: string[]
    additionalProperties: boolean
  }
}

/** One function tool definition, as sent in the request's `tools` array. */
export interface ChatTool {
  type: 'function'
  function: ChatFunctionSchema
}

export interface ChatRequest {
  messages: ChatMessage[]
  tools?: ChatTool[]
}

export interface ChatChoice {
  message?: ChatMessage
  finish_reason?: string | null
}

/** Parsed `/chat/completions` body, read leniently — the model host may answer anything. */
export interface ChatResponse {
  choices?: ChatChoice[]
  error?: { message?: string }
}

/** One non-streaming chat completion. Injected; never called from tools. */
export type ChatPort = (req: ChatRequest) => Promise<ChatResponse>

/**
 * The scan facts `get_scan_summary` exposes. `topChildren` are the root's
 * largest children (at most 10); every node here becomes a known id for later
 * tool calls.
 */
export interface ScanSummary {
  state: ScanState
  label: string | null
  totalBytes: number
  entryCount: number
  /** Coverage / unreadable notes, already trimmed to a few lines. */
  notes: string[]
  root: TreeNodeSummary
  topChildren: TreeNodeSummary[]
}

/** Cache discovery rows (`find_cache_candidates`), derived from `DiscoveryPage`. */
export interface CacheCandidatePage {
  items: DiscoveryItem[]
  total: number
  totalSize: number
}

export const CLEANUP_RISKS = ['regenerable', 'confirm', 'keep'] as const

export type CleanupRisk = (typeof CLEANUP_RISKS)[number]

export interface StageRequestItem {
  nodeId: string
  reason: string
  risk: CleanupRisk
}

export interface StageRejection {
  nodeId: string
  reason: string
}

/** What the caller's collector accepted, and why it refused the rest. */
export interface StageOutcome {
  staged: CollectorEntry[]
  rejected: StageRejection[]
}

/** The user's open view: the focused node, its breadcrumb and its largest children. */
export interface FocusView {
  node: TreeNodeSummary
  /** Outermost first; excludes `node`. */
  ancestors: TreeNodeSummary[]
  /** Largest children first, at most the page that was asked for. */
  children: TreeNodeSummary[]
  /** The engine's full child count for `node` — may be far larger than `children`. */
  totalChildren: number
}

/**
 * The read-only window onto one scan. Implemented by `lensPortFromService`
 * (real service methods) and by fixtures in tests, so tools never need an
 * engine. Every method is paged or single-node: nothing materialises a
 * subtree. `stage` is the only side effect and is supplied by the caller: the
 * agent can propose, never delete.
 */
export interface LensPort {
  /** Whole-scan totals and coverage notes (one bounded call). */
  summary(): Promise<ScanSummary>
  /** One node's view. `null` means the scan root. */
  focus(nodeId: string | null): Promise<FocusView>
  /** One page of a folder, sorted by size descending. */
  children(nodeId: string, limit: number, offset: number): Promise<ChildrenPage>
  cacheCandidates(limit: number): Promise<CacheCandidatePage>
  stage(items: StageRequestItem[]): Promise<StageOutcome>
}
