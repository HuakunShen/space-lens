import type { WorkbenchService } from '@space-lens/client'
import type { ChildrenPage, CollectorEntry, ScanStatus, TreeNodeSummary } from '@space-lens/contract'
import { unmeasuredSkipCount } from '@space-lens/contract'
import { canCollectNode, collectRefusalReason } from '@space-lens/web-ui/lib/app-scan'
import {
  CLEANUP_RISKS,
  type CacheCandidatePage,
  type ChatTool,
  type CleanupRisk,
  type FocusView,
  type LensPort,
  type ProposalRow,
  type ScanSummary,
  type StageOutcome,
  type StageRejection,
  type StageRequestItem,
} from './types.ts'

/**
 * Slice A tool layer for whole-disk scans (millions of nodes).
 *
 * The model never receives the tree. It gets compact `id|name|MB|kind|children`
 * rows for one page at a time, navigates by opaque `nodeId`, and every guard
 * below is enforced in code rather than by the prompt:
 *
 * 1. `nodeId` only, and only ids seen in earlier results this run (bounded to
 *    `maxSeenIds`, oldest dropped).
 * 2. `propose_cleanup` stages `regenerable`/`confirm` only, at most 50 per
 *    call, only nodes passing `canCollectNode`; `keep` is shown, never staged.
 * 3. Token budget (≈4 bytes/token): per result ≤1.5k, whole run ≤24k of tool
 *    output, whole context ≤32k. Exceeding returns a notice and ends the run.
 * 4. `MB` is MiB (1024²) to match the app's `formatBytes`; names are stuck to
 *    one line, stripped of separators and truncated to 80 chars.
 */

export const AGENT_LIMITS = {
  maxRounds: 8,
  maxToolCalls: 24,
  /** Rough English/JSON text ratio used for every token estimate. */
  bytesPerToken: 4,
  maxResultTokens: 1_500,
  maxToolOutputTokens: 24_000,
  maxContextTokens: 32_000,
  maxListLimit: 40,
  defaultListLimit: 20,
  maxProposeItems: 50,
  maxSeenIds: 5_000,
  maxStaged: 500,
  maxNameChars: 80,
  maxInitialRows: 80,
  maxInitialBytes: 4 * 1024,
  maxFocusChildren: 15,
  maxExpandChildren: 5,
} as const

const MAX_RESULT_BYTES = AGENT_LIMITS.maxResultTokens * AGENT_LIMITS.bytesPerToken

export const TOOL_NAMES = [
  'get_scan_summary',
  'list_children',
  'find_cache_candidates',
  'propose_cleanup',
  'list_proposal',
  'remove_from_proposal',
] as const

export type ToolName = (typeof TOOL_NAMES)[number]

const ROW_HEADER = 'id|name|MB|kind|children'

export const AI_TOOLS: readonly ChatTool[] = [
  {
    type: 'function',
    function: {
      name: 'get_scan_summary',
      description:
        'Scan totals, state, coverage notes and the 10 largest children of the scan root. The initial context already lists the open view; use this to re-read totals.',
      parameters: { type: 'object', properties: {}, additionalProperties: false },
    },
  },
  {
    type: 'function',
    function: {
      name: 'list_children',
      description:
        'One page of a folder, largest first. Hard cap 40 rows (default 20); the run is told `total` and `remaining`. nodeId must come from an earlier result this run.',
      parameters: {
        type: 'object',
        properties: {
          nodeId: { type: 'string', description: 'Folder id from an earlier result.' },
          limit: { type: 'integer', description: 'Rows to return (1-40, default 20).', minimum: 1, maximum: 40 },
          offset: { type: 'integer', description: 'Rows to skip, largest first.', minimum: 0 },
        },
        required: ['nodeId'],
        additionalProperties: false,
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'find_cache_candidates',
      description: 'Existing cache discovery rows (build output, package caches, logs) with category and size.',
      parameters: {
        type: 'object',
        properties: {
          limit: { type: 'integer', description: 'Rows to return (1-40, default 20).', minimum: 1, maximum: 40 },
        },
        additionalProperties: false,
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'propose_cleanup',
      description:
        'Stage candidates for the human to review in the Collector. Stages nothing itself and deletes nothing. Node ids must come from earlier results this run.',
      parameters: {
        type: 'object',
        properties: {
          items: {
            type: 'array',
            description: `Candidates to stage (max ${AGENT_LIMITS.maxProposeItems}).`,
            items: {
              type: 'object',
              properties: {
                nodeId: { type: 'string', description: 'Id from an earlier result this run.' },
                reason: { type: 'string', description: 'One short sentence: why this can go.' },
                risk: {
                  type: 'string',
                  enum: [...CLEANUP_RISKS],
                  description: 'regenerable = rebuildable; confirm = needs a human check; keep = never staged.',
                },
              },
              required: ['nodeId', 'reason', 'risk'],
              additionalProperties: false,
            },
          },
        },
        required: ['items'],
        additionalProperties: false,
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'list_proposal',
      description:
        "The user's live cleanup selection for this scan: what you staged plus their own picks, minus anything they removed. Read it before changing the selection.",
      parameters: { type: 'object', properties: {}, additionalProperties: false },
    },
  },
  {
    type: 'function',
    function: {
      name: 'remove_from_proposal',
      description:
        'Take items out of the cleanup selection when the user wants to keep them. Only shrinks the selection; ids come from list_proposal or earlier results.',
      parameters: {
        type: 'object',
        properties: {
          nodeIds: {
            type: 'array',
            description: `Ids to remove (max ${AGENT_LIMITS.maxProposeItems}).`,
            items: { type: 'string' },
          },
        },
        required: ['nodeIds'],
        additionalProperties: false,
      },
    },
  },
]

/** Run-scoped state: the seen-id set, the token budget and the staged list. */
export interface ToolRuntime {
  /** nodeId → summary, LRU-bounded to `maxSeenIds`. Populated only by results. */
  readonly seen: Map<string, TreeNodeSummary>
  toolCalls: number
  /** Estimated tokens of tool output returned to the model this run. */
  toolOutputTokens: number
  /** Raw UTF-8 bytes of that output. */
  toolOutputBytes: number
  readonly staged: CollectorEntry[]
  /** Non-null once a budget guard tripped; the agent then ends the run. */
  stopReason: string | null
}

/** What one tool call hands back to the loop. */
export interface ToolOutcome {
  /** Compact text the model receives. */
  content: string
  /** One line that replaces this result once it is older than 2 rounds. */
  summary: string
  /** True when a budget guard tripped and the run should end. */
  stop: boolean
}

export function createToolRuntime(): ToolRuntime {
  return { seen: new Map(), toolCalls: 0, toolOutputTokens: 0, toolOutputBytes: 0, staged: [], stopReason: null }
}

/**
 * Starts a new run on a runtime kept for a whole conversation: the seen ids
 * (and the memory tools' seen groups, keyed by this object) survive, so a
 * follow-up can name what an earlier answer found; every budget starts over.
 */
export function resetRunBudget(runtime: ToolRuntime): void {
  runtime.toolCalls = 0
  runtime.toolOutputTokens = 0
  runtime.toolOutputBytes = 0
  runtime.staged.length = 0
  runtime.stopReason = null
}

/** The notice the model receives when a run budget guard trips. */
export function budgetStopNotice(reason: string): string {
  return `error budget: ${reason}; this run stops here.`
}

/** Adds ids to the seen set, refreshing recency and dropping the oldest past the cap. */
export function rememberNodes(runtime: ToolRuntime, nodes: readonly TreeNodeSummary[]): void {
  for (const node of nodes) {
    if (runtime.seen.has(node.id)) runtime.seen.delete(node.id)
    runtime.seen.set(node.id, node)
  }
  while (runtime.seen.size > AGENT_LIMITS.maxSeenIds) {
    const oldest = runtime.seen.keys().next()
    if (oldest.done === true) break
    runtime.seen.delete(oldest.value)
  }
}

/**
 * Runs one tool call and always answers with a result. Malformed arguments,
 * unknown tools, unknown ids and lens failures all come back as text — they
 * never throw into the loop.
 */
export async function executeTool(
  runtime: ToolRuntime,
  lens: LensPort,
  name: string,
  argsJson: string,
): Promise<ToolOutcome> {
  const guarded = checkBudget(runtime)
  if (guarded !== null) return guarded
  runtime.toolCalls += 1
  let output: HandlerOutput
  try {
    output = await dispatch(runtime, lens, name, argsJson)
  } catch (error) {
    output = errorOutput(name, `tool failed: ${describe(error)}`)
  }
  return finish(runtime, output)
}

interface HandlerOutput {
  content: string
  summary: string
}

type Handler = (runtime: ToolRuntime, lens: LensPort, args: Record<string, unknown>) => Promise<HandlerOutput>

async function dispatch(runtime: ToolRuntime, lens: LensPort, name: string, argsJson: string): Promise<HandlerOutput> {
  if (!isToolName(name)) {
    return errorOutput(name, `unknown tool; available: ${TOOL_NAMES.join(', ')}`)
  }
  const parsed = parseArgs(argsJson)
  if (!parsed.ok) return errorOutput(name, parsed.error)
  return HANDLERS[name](runtime, lens, parsed.value)
}

const HANDLERS: Record<ToolName, Handler> = {
  async get_scan_summary(runtime, lens): Promise<HandlerOutput> {
    const summary: ScanSummary = await lens.summary()
    const top = summary.topChildren.slice(0, 10)
    rememberNodes(runtime, [summary.root, ...top])
    const lines = [
      `get_scan_summary state=${summary.state} totalMB=${mb(summary.totalBytes)} entries=${summary.entryCount}`,
      `root ${rowLine(summary.root)}`,
      ...(summary.scope !== undefined ? [`scope ${compactName(summary.scope).slice(0, 300)}`] : []),
      ...summary.notes.slice(0, 5).map((note) => `note ${compactName(note)}`),
      ROW_HEADER,
      ...top.map(rowLine),
    ]
    return {
      content: lines.join('\n'),
      summary: `get_scan_summary: ${summary.state}, ${mb(summary.totalBytes)} MB, ${summary.entryCount} entries`,
    }
  },

  async list_children(runtime, lens, args): Promise<HandlerOutput> {
    const nodeId = asString(args.nodeId)
    if (nodeId === null || nodeId === '') return errorOutput('list_children', 'nodeId must be a non-empty string')
    if (!runtime.seen.has(nodeId)) return unknownNode('list_children', nodeId)
    const limit = readInt(args.limit, AGENT_LIMITS.defaultListLimit, 1, AGENT_LIMITS.maxListLimit)
    if (limit === null) return errorOutput('list_children', 'limit must be an integer')
    const offset = readInt(args.offset, 0, 0, Number.MAX_SAFE_INTEGER)
    if (offset === null) return errorOutput('list_children', 'offset must be an integer')

    const page = await lens.children(nodeId, limit, offset)
    // The service pages largest-first; keep the invariant even if a host does not.
    const items = [...page.items].sort((a, b) => b.size - a.size)
    rememberNodes(runtime, items)
    const bytes = items.reduce((sum, item) => sum + item.size, 0)
    const remaining = Math.max(0, page.total - page.offset - items.length)
    return {
      content: [
        `list_children ${safeId(nodeId)} total=${page.total} rows=${items.length} remaining=${remaining}`,
        ROW_HEADER,
        ...items.map(rowLine),
      ].join('\n'),
      summary: `list_children ${safeId(nodeId)}: ${items.length} rows, ${mb(bytes)} MB`,
    }
  },

  async find_cache_candidates(runtime, lens, args): Promise<HandlerOutput> {
    const limit = readInt(args.limit, AGENT_LIMITS.defaultListLimit, 1, AGENT_LIMITS.maxListLimit)
    if (limit === null) return errorOutput('find_cache_candidates', 'limit must be an integer')
    const page: CacheCandidatePage = await lens.cacheCandidates(limit)
    rememberNodes(
      runtime,
      page.items.map((item) => item.node),
    )
    const rows = page.items.map(
      (item) =>
        `${safeId(item.node.id)}|${compactName(item.node.name)}|${mb(item.node.size)}|${compactName(item.category)}`,
    )
    return {
      content: [
        `find_cache_candidates total=${page.total} totalMB=${mb(page.totalSize)} rows=${page.items.length}`,
        'id|name|MB|category',
        ...rows,
      ].join('\n'),
      summary: `find_cache_candidates: ${page.items.length} rows, ${mb(page.totalSize)} MB`,
    }
  },

  async propose_cleanup(runtime, lens, args): Promise<HandlerOutput> {
    const rawItems = args.items
    if (!Array.isArray(rawItems)) return errorOutput('propose_cleanup', 'items must be an array')
    if (rawItems.length > AGENT_LIMITS.maxProposeItems) {
      return errorOutput('propose_cleanup', `at most ${AGENT_LIMITS.maxProposeItems} items per call`)
    }

    const accepted: StageRequestItem[] = []
    const reasons = new Map<string, string>()
    const rejected: StageRejection[] = []
    const requested = new Set<string>()

    for (const raw of rawItems) {
      const item = asRecord(raw)
      if (item === null) {
        rejected.push({ nodeId: '?', reason: 'item must be an object' })
        continue
      }
      const nodeId = asString(item.nodeId) ?? ''
      if (nodeId === '') {
        rejected.push({ nodeId: '?', reason: 'nodeId must be a non-empty string' })
        continue
      }
      if (requested.has(nodeId)) {
        rejected.push({ nodeId, reason: 'duplicate item' })
        continue
      }
      requested.add(nodeId)
      const reason = asString(item.reason)
      if (reason === null) {
        rejected.push({ nodeId, reason: 'reason must be a string' })
        continue
      }
      const risk = asString(item.risk)
      if (risk === null || !isRisk(risk)) {
        rejected.push({ nodeId, reason: `risk must be one of ${CLEANUP_RISKS.join('|')}` })
        continue
      }
      if (risk === 'keep') {
        rejected.push({ nodeId, reason: 'risk keep is never staged' })
        continue
      }
      const node = runtime.seen.get(nodeId)
      if (node === undefined) {
        rejected.push({
          nodeId,
          reason: 'unknown nodeId: only ids returned by earlier results this run can be staged',
        })
        continue
      }
      if (!canCollectNode(node)) {
        rejected.push({ nodeId, reason: collectRefusalReason(node) ?? 'this node cannot be collected' })
        continue
      }
      const trimmed = reason.slice(0, 500)
      accepted.push({ nodeId, reason: trimmed, risk })
      reasons.set(nodeId, trimmed)
    }

    let outcome: StageOutcome = { staged: [], rejected: [] }
    if (accepted.length > 0) outcome = await lens.stage(accepted)
    for (const entry of outcome.staged) runtime.staged.push(entry)
    if (runtime.staged.length > AGENT_LIMITS.maxStaged) {
      runtime.staged.splice(0, runtime.staged.length - AGENT_LIMITS.maxStaged)
    }

    const allRejected = [...rejected, ...outcome.rejected]
    return {
      content: [
        `propose_cleanup staged=${outcome.staged.length} rejected=${allRejected.length}`,
        ...outcome.staged.map(
          (entry) =>
            `+ ${safeId(entry.nodeId)}|${mb(entry.size)}|${riskOf(accepted, entry.nodeId) ?? '?'}|${compactName(reasons.get(entry.nodeId) ?? '')}`,
        ),
        ...allRejected.map((entry) => `- ${safeId(entry.nodeId)}|${compactName(entry.reason)}`),
      ].join('\n'),
      summary: `propose_cleanup: ${outcome.staged.length} staged, ${allRejected.length} rejected`,
    }
  },

  async list_proposal(_runtime, lens): Promise<HandlerOutput> {
    if (!lens.proposal) return errorOutput('list_proposal', 'this host cannot read the selection')
    const rows = await lens.proposal()
    const shown = rows.slice(0, AGENT_LIMITS.maxListLimit)
    const bytes = rows.reduce((sum, row) => sum + row.size, 0)
    return {
      content: [
        `list_proposal rows=${rows.length} totalMB=${mb(bytes)}${rows.length > shown.length ? ` shown=${shown.length}` : ''}`,
        'id|name|MB|risk|reason (risk "user" = the user picked it)',
        ...shown.map(proposalLine),
      ].join('\n'),
      summary: `list_proposal: ${rows.length} rows, ${mb(bytes)} MB`,
    }
  },

  async remove_from_proposal(runtime, lens, args): Promise<HandlerOutput> {
    if (!lens.unstage) return errorOutput('remove_from_proposal', 'this host cannot edit the selection')
    const raw = args.nodeIds
    if (!Array.isArray(raw) || raw.length === 0) {
      return errorOutput('remove_from_proposal', 'nodeIds must be a non-empty array')
    }
    if (raw.length > AGENT_LIMITS.maxProposeItems) {
      return errorOutput('remove_from_proposal', `at most ${AGENT_LIMITS.maxProposeItems} ids per call`)
    }
    const ids = [...new Set(raw.map(asString).filter((id): id is string => id !== null && id !== ''))]
    // Removal only ever shrinks what would be deleted, so it needs no seen-id
    // check: the worst a bad id does is miss.
    const outcome = await lens.unstage(ids)
    const removedIds = new Set(outcome.removed.map((row) => row.nodeId))
    for (let index = runtime.staged.length - 1; index >= 0; index -= 1) {
      if (removedIds.has(runtime.staged[index]!.nodeId)) runtime.staged.splice(index, 1)
    }
    const bytes = outcome.removed.reduce((sum, row) => sum + row.size, 0)
    return {
      content: [
        `remove_from_proposal removed=${outcome.removed.length} freedMB=${mb(bytes)} missing=${outcome.missing.length}`,
        ...outcome.removed.map((row) => `- ${proposalLine(row)}`),
        ...outcome.missing.map((id) => `? ${safeId(id)}|not in the selection`),
      ].join('\n'),
      summary: `remove_from_proposal: ${outcome.removed.length} removed`,
    }
  },
}

function proposalLine(row: ProposalRow): string {
  return `${safeId(row.nodeId)}|${compactName(row.name)}|${mb(row.size)}|${row.risk ?? 'user'}|${compactName(row.reason ?? '')}`
}

// ---------------------------------------------------------------------------
// Compact rows
// ---------------------------------------------------------------------------

/** MiB with one decimal, matching the app's `formatBytes` unit family. */
export function mb(bytes: number): string {
  const value = bytes / (1024 * 1024)
  if (!Number.isFinite(value) || value <= 0) return '0'
  return value >= 100 ? String(Math.round(value)) : String(Number(value.toFixed(1)))
}

/** One line, no separators the row format uses, at most `maxNameChars`. */
export function compactName(name: string): string {
  const flat = name
    .replaceAll(/[|\r\n\t]+/g, ' ')
    .replaceAll(/\s+/g, ' ')
    .trim()
  return flat.length > AGENT_LIMITS.maxNameChars ? `${flat.slice(0, AGENT_LIMITS.maxNameChars - 1)}…` : flat
}

/** Ids are opaque, but a stray newline would break the row format. */
function safeId(id: string): string {
  return id.replaceAll(/[\r\n\t]/g, ' ')
}

function rowLine(node: TreeNodeSummary): string {
  const kind = (node.isDirectory ?? node.hasChildren) ? 'dir' : 'file'
  return `${safeId(node.id)}|${compactName(node.name)}|${mb(node.size)}|${kind}|${node.childCount}`
}

function riskOf(items: readonly StageRequestItem[], nodeId: string): CleanupRisk | null {
  return items.find((item) => item.nodeId === nodeId)?.risk ?? null
}

// ---------------------------------------------------------------------------
// Initial context: the view the user has open, nothing more
// ---------------------------------------------------------------------------

export interface InitialContext {
  /** The compact text placed at the head of the first user message. */
  text: string
  /** Every node mentioned, so `list_children` accepts them immediately. */
  nodes: TreeNodeSummary[]
  /** Data rows emitted (headers and `others` lines excluded). */
  rows: number
  /** Bytes actually used, never above `maxInitialBytes`. */
  bytes: number
}

/**
 * Builds the model's starting window: the focused folder's breadcrumb names,
 * its 15 largest children, the 5 largest of each of those (bounded to
 * `maxInitialRows` rows and `maxInitialBytes`), an `others` row per level, and
 * the coverage notes. Costs one `summary` call, at most one `focus` call and
 * one paged `children` call per expanded child — never a subtree.
 */
export async function buildInitialContext(lens: LensPort, focusNodeId?: string): Promise<InitialContext> {
  const summary = await lens.summary()
  const view: FocusView =
    focusNodeId === undefined
      ? { node: summary.root, ancestors: [], children: summary.topChildren, totalChildren: summary.root.childCount }
      : await lens.focus(focusNodeId)

  const nodes: TreeNodeSummary[] = [view.node, ...view.ancestors]
  const parts: string[] = []
  let bytes = 0
  let rows = 0
  const push = (line: string): boolean => {
    const size = byteLength(line) + 1
    if (bytes + size > AGENT_LIMITS.maxInitialBytes) return false
    parts.push(line)
    bytes += size
    return true
  }

  push(`scan state=${summary.state} totalMB=${mb(summary.totalBytes)} entries=${summary.entryCount}`)
  if (summary.scope !== undefined) {
    push(`scope ${compactName(summary.scope).slice(0, 300)} (the folder the user has open; answer about it)`)
  }
  for (const note of summary.notes.slice(0, 5)) push(`note ${compactName(note)}`)
  push(`breadcrumb ${compactName([...view.ancestors, view.node].map((node) => node.name).join(' > ')).slice(0, 300)}`)
  push(`focus ${rowLine(view.node)}`)
  push(`rows ${ROW_HEADER}`)

  const top = view.children.slice(0, AGENT_LIMITS.maxFocusChildren)
  const topBytes = top.reduce((sum, child) => sum + child.size, 0)
  for (const child of top) {
    if (rows >= AGENT_LIMITS.maxInitialRows || !push(rowLine(child))) break
    nodes.push(child)
    rows += 1
  }
  pushOthers(push, top.length, view.totalChildren, topBytes, view.node.size)

  for (const child of top) {
    if (rows >= AGENT_LIMITS.maxInitialRows) break
    // One unreadable folder (permissions, a vanished mount) must not fail the run.
    let page: ChildrenPage
    try {
      page = await lens.children(child.id, AGENT_LIMITS.maxExpandChildren, 0)
    } catch (error) {
      push(`note ${safeId(child.id)} unreadable: ${compactName(describe(error))}`)
      continue
    }
    if (page.items.length === 0 && page.total === 0) continue
    if (!push(`in ${safeId(child.id)}`)) break
    const listed = page.items.slice(0, AGENT_LIMITS.maxExpandChildren)
    const listedBytes = listed.reduce((sum, item) => sum + item.size, 0)
    let shown = 0
    for (const item of listed) {
      if (rows >= AGENT_LIMITS.maxInitialRows || !push(rowLine(item))) break
      nodes.push(item)
      rows += 1
      shown += 1
    }
    pushOthers(push, shown, page.total, listedBytes, child.size)
  }

  return { text: parts.join('\n'), nodes, rows, bytes }
}

function pushOthers(
  push: (line: string) => boolean,
  shown: number,
  total: number,
  shownBytes: number,
  totalBytes: number,
): void {
  const count = Math.max(0, total - shown)
  const bytes = Math.max(0, totalBytes - shownBytes)
  if (count > 0 || bytes > 0) push(`others ${count}|${mb(bytes)}`)
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

function checkBudget(runtime: ToolRuntime): ToolOutcome | null {
  if (runtime.stopReason !== null) return stopped(runtime.stopReason)
  if (runtime.toolCalls >= AGENT_LIMITS.maxToolCalls) {
    return tripBudget(runtime, `tool-call budget of ${AGENT_LIMITS.maxToolCalls} reached`)
  }
  if (runtime.toolOutputTokens >= AGENT_LIMITS.maxToolOutputTokens) {
    return tripBudget(runtime, `tool-output budget of ${tokenLabel(AGENT_LIMITS.maxToolOutputTokens)} reached`)
  }
  return null
}

function stopped(reason: string): ToolOutcome {
  return { content: budgetStopNotice(reason), summary: `budget: ${reason}`, stop: true }
}

function tripBudget(runtime: ToolRuntime, reason: string): ToolOutcome {
  runtime.stopReason = reason
  const content = budgetStopNotice(reason)
  runtime.toolOutputTokens += estimateTokens(content)
  runtime.toolOutputBytes += byteLength(content)
  return { content, summary: `budget: ${reason}`, stop: true }
}

function finish(runtime: ToolRuntime, output: HandlerOutput): ToolOutcome {
  const content = truncateUtf8(output.content, MAX_RESULT_BYTES)
  runtime.toolOutputTokens += estimateTokens(content)
  runtime.toolOutputBytes += byteLength(content)
  if (runtime.toolOutputTokens > AGENT_LIMITS.maxToolOutputTokens && runtime.stopReason === null) {
    runtime.stopReason = `tool-output budget of ${tokenLabel(AGENT_LIMITS.maxToolOutputTokens)} reached`
  }
  return { content, summary: output.summary, stop: runtime.stopReason !== null }
}

const encoder = new TextEncoder()
const decoder = new TextDecoder()

export function byteLength(text: string): number {
  return encoder.encode(text).length
}

/** ≈4 bytes/token, rounded up. The only token estimate the run uses. */
export function estimateTokens(text: string): number {
  return Math.ceil(byteLength(text) / AGENT_LIMITS.bytesPerToken)
}

function tokenLabel(tokens: number): string {
  return tokens >= 1000 ? `${tokens / 1000}k` : String(tokens)
}

/** Hard per-result cap (tokens, enforced as bytes). Slices on a byte boundary. */
export function truncateUtf8(text: string, maxBytes: number): string {
  const encoded = encoder.encode(text)
  if (encoded.length <= maxBytes) return text
  const notice = `…[truncated at ${tokenLabel(Math.round(maxBytes / AGENT_LIMITS.bytesPerToken))} tokens]`
  const room = Math.max(0, maxBytes - byteLength(notice))
  return decoder.decode(encoded.slice(0, room)) + notice
}

function errorOutput(tool: string, message: string): HandlerOutput {
  const line = `error ${tool ? `${tool} ` : ''}${message}`
  return { content: line, summary: line }
}

function unknownNode(tool: string, nodeId: string): HandlerOutput {
  return errorOutput(
    tool,
    `unknown nodeId ${safeId(nodeId)}: only ids returned by earlier results this run can be used`,
  )
}

function isToolName(name: string): name is ToolName {
  return (TOOL_NAMES as readonly string[]).includes(name)
}

function isRisk(value: string): value is CleanupRisk {
  return (CLEANUP_RISKS as readonly string[]).includes(value)
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function asString(value: unknown): string | null {
  return typeof value === 'string' ? value : null
}

/** Undefined falls back to the clamped default; anything else must be an integer. */
function readInt(value: unknown, fallback: number, min: number, max: number): number | null {
  if (value === undefined) return clamp(fallback, min, max)
  if (typeof value !== 'number' || !Number.isInteger(value)) return null
  return clamp(value, min, max)
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}

type ParseResult = { ok: true; value: Record<string, unknown> } | { ok: false; error: string }

function parseArgs(argsJson: string): ParseResult {
  const trimmed = argsJson.trim()
  if (trimmed === '') return { ok: true, value: {} }
  try {
    const parsed: unknown = JSON.parse(trimmed)
    const record = asRecord(parsed)
    if (record === null) return { ok: false, error: 'tool arguments must be a JSON object' }
    return { ok: true, value: record }
  } catch {
    return { ok: false, error: 'tool arguments were not valid JSON' }
  }
}

function describe(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error)
  return message.length > 300 ? `${message.slice(0, 300)}…` : message
}

// ---------------------------------------------------------------------------
// Real service adapter
// ---------------------------------------------------------------------------

/**
 * Maps the real workbench service onto `LensPort` for one scan.
 *
 * `stage` is supplied by the caller — the panel wires it to the existing
 * collector (`addSelection`), which owns the side effect. Everything else is
 * read-only and paged: `summary`/`focus` are `scanStatus` + one depth-1 tree
 * slice, `children` and `cacheCandidates` are the existing `children`/`discover`
 * methods (discovery is always asked for a page, never the 200-row default).
 */
/** The slice of `WorkbenchService` the tools read; `discover` is optional so a bridge without it still works. */
export type LensService = Pick<WorkbenchService, 'scanStatus' | 'treeSlice' | 'children'> & Partial<Pick<WorkbenchService, 'discover'>>

/** The folder a conversation is about, when it is narrower than the scan root. */
export interface LensScope {
  nodeId: string
  path: string
}

export interface LensPortOptions {
  /** Narrows the summary and the cache search to this folder; `null` = whole scan. */
  scope?: LensScope | null
  proposal?: LensPort['proposal']
  unstage?: LensPort['unstage']
}

/** Cache rows are filtered client-side under a scope, so a scoped search reads a wider page first. */
const SCOPED_DISCOVERY_PAGE = 200

function isUnder(path: string, folder: string): boolean {
  const base = folder.endsWith('/') ? folder : `${folder}/`
  return path === folder || path.startsWith(base)
}

export function lensPortFromService(
  service: LensService,
  scanId: string,
  stage: LensPort['stage'],
  options: LensPortOptions = {},
): LensPort {
  const scope = options.scope ?? null
  const rootId = async (): Promise<string> => {
    const status = await service.scanStatus(scanId)
    const id = status.rootIds[0]
    if (id === undefined) throw new Error('scan has no root node')
    return id
  }
  return {
    async summary(): Promise<ScanSummary> {
      const status = await service.scanStatus(scanId)
      const id = scope?.nodeId ?? status.rootIds[0]
      if (id === undefined) throw new Error('scan has no root node to summarise')
      const slice = await service.treeSlice({ scanId, nodeId: id, depth: 1, maxChildrenPerNode: 15 })
      return {
        state: status.state,
        label: status.label,
        totalBytes: slice.focusNode.size,
        entryCount: status.entriesScanned,
        notes: summaryNotes(status),
        root: slice.focusNode,
        topChildren: slice.tree.children.slice(0, 15),
        ...(scope !== null ? { scope: scope.path } : {}),
      }
    },
    async focus(nodeId: string | null): Promise<FocusView> {
      const slice = await service.treeSlice({
        scanId,
        nodeId: nodeId ?? (await rootId()),
        depth: 1,
        maxChildrenPerNode: 15,
      })
      return {
        node: slice.focusNode,
        ancestors: slice.ancestors,
        children: slice.tree.children,
        totalChildren: slice.focusNode.childCount,
      }
    },
    children(nodeId: string, limit: number, offset: number): Promise<ChildrenPage> {
      return service.children({ scanId, nodeId, offset, limit, sort: 'size' })
    },
    async cacheCandidates(limit: number): Promise<CacheCandidatePage> {
      if (!service.discover) throw new Error('this host cannot run discovery')
      if (scope === null) {
        const page = await service.discover({ scanId, kind: 'caches', minSize: 0, offset: 0, limit })
        return { items: page.items, total: page.total, totalSize: page.totalSize }
      }
      // Discovery has no path filter; the largest rows are read and narrowed here.
      const page = await service.discover({ scanId, kind: 'caches', minSize: 0, offset: 0, limit: SCOPED_DISCOVERY_PAGE })
      const inside = page.items.filter((item) => isUnder(item.node.path, scope.path))
      return {
        items: inside.slice(0, limit),
        total: inside.length,
        totalSize: inside.reduce((sum, item) => sum + item.node.size, 0),
      }
    },
    stage,
    ...(options.proposal ? { proposal: options.proposal } : {}),
    ...(options.unstage ? { unstage: options.unstage } : {}),
  }
}

function summaryNotes(status: ScanStatus): string[] {
  const notes: string[] = []
  if (status.state !== 'ready') notes.push(`scan state: ${status.state}`)
  if (status.message !== '') notes.push(status.message.slice(0, 300))
  const coverage = status.coverage
  if (coverage) {
    notes.push(`${unmeasuredSkipCount(coverage)} skipped, ${coverage.deniedCount} denied, ${coverage.issueCount} unreadable`)
    for (const issue of coverage.issues.slice(0, 3)) notes.push(`${issue.path}: ${issue.reason}`)
  }
  return notes
}
