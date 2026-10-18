import type { FreeUpCandidate } from '@space-lens/contract'
import type { ChatTool, LensPort } from './types.ts'
import type { GroupView, MemoryPort, MemorySummaryView, QuitStageItem, QuitStageOutcome } from './memory-types.ts'
import {
  AGENT_LIMITS,
  TOOL_NAMES,
  budgetStopNotice,
  byteLength,
  estimateTokens,
  executeTool,
  mb,
  truncateUtf8,
  type ToolOutcome,
  type ToolRuntime,
} from './tools.ts'

/**
 * Memory tool layer for the AI advisor (§8 of docs/memory-cleaner-plan.md).
 *
 * Four read-mostly tools over `MemoryPort`, composed with the disk tools only
 * when a host supplies both ports. The model navigates by opaque `groupId` and
 * every guard is enforced in code rather than by the prompt:
 *
 * 1. `groupId` only, and only ids returned by an earlier result this run
 *    (bounded to `maxSeenGroups`, oldest dropped).
 * 2. `propose_quit` adds to the user's Quit list at most 10 ids per call, and
 *    refuses protected, self, frontmost, restricted and vanished groups. There
 *    is no `mode` argument anywhere in the schemas: the model cannot ask for
 *    force, and it cannot quit anything — only the user's confirmation runs a
 *    plan.
 * 3. Names are sanitised before output: control characters and `|` removed,
 *    newlines to spaces, truncated to `maxNameChars`; rows carry `F`/`P`/`S`/`R`
 *    flags instead of free text.
 * 4. Nothing else leaves the host: no pids, start times, exe or bundle paths,
 *    command lines, windows or bundle ids — they are attacker-chosen strings
 *    too. Members are `name|MB|cpu` only.
 *
 * Budgets are the shared `AGENT_LIMITS` / `ToolRuntime` counters from
 * `tools.ts`. The per-run group memory (`seenGroups`, cap 500) lives in a
 * `WeakMap` keyed by that runtime, so it shares the runtime's lifetime, and the
 * guard helpers below are mirrored from `tools.ts` over the same counters:
 * `tools.ts` is outside this task's write set, so its private guards cannot be
 * imported.
 */

export const MEMORY_LIMITS = {
  /** Ids the run remembers from its own results. */
  maxSeenGroups: 500,
  maxListLimit: 40,
  defaultListLimit: 20,
  /** Members listed by `get_memory_group`. */
  maxMembers: 20,
  /** Rows in the initial memory window and in `get_memory_summary`. */
  maxTop: 10,
  maxNameChars: 60,
  maxProposeItems: 10,
  maxReasonChars: 200,
} as const

export const MEMORY_TOOL_NAMES = [
  'get_memory_summary',
  'list_memory_groups',
  'get_memory_group',
  'propose_quit',
] as const

export type MemoryToolName = (typeof MEMORY_TOOL_NAMES)[number]

/** Column header for the group rows (`MB` is MiB, matching the disk tools). */
const ROW_HEADER = 'gid|name|MB|cpu|procs|kind|flags'

/** Column header for a group's member rows. */
const MEMBER_HEADER = 'name|MB|cpu'

export const MEMORY_TOOLS: readonly ChatTool[] = [
  {
    type: 'function',
    function: {
      name: 'get_memory_summary',
      description:
        'Memory pressure, system totals (used/total/swap/compressed MB) and the minutes this memory view has been open, with the 10 largest app groups. The initial context already lists them; use this to re-read totals.',
      parameters: { type: 'object', properties: {}, additionalProperties: false },
    },
  },
  {
    type: 'function',
    function: {
      name: 'list_memory_groups',
      description:
        'One page of running app groups, largest first (or busiest first with sort "cpu"). Hard cap 40 rows (default 20); the run is told `total` and `remaining`. Rows are gid|name|MB|cpu|procs|kind|flags.',
      parameters: {
        type: 'object',
        properties: {
          sort: {
            type: 'string',
            enum: ['footprint', 'cpu'],
            description: 'Row order: memory footprint (default) or CPU.',
          },
          limit: { type: 'integer', description: 'Rows to return (1-40, default 20).', minimum: 1, maximum: 40 },
          offset: { type: 'integer', description: 'Rows to skip, according to the sort.', minimum: 0 },
        },
        additionalProperties: false,
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'get_memory_group',
      description:
        "One group: its row, its 20 largest members as name|MB|cpu, and the engine's free-up verdict (reason codes and risks). groupId must come from an earlier result this run.",
      parameters: {
        type: 'object',
        properties: {
          groupId: { type: 'string', description: 'Group id from an earlier result this run.' },
        },
        required: ['groupId'],
        additionalProperties: false,
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'propose_quit',
      description:
        'Add apps to the Quit list the user reviews and confirms. Stages nothing itself and quits nothing: it is the only way to suggest a quit, it takes no mode, and the user still has to confirm. Group ids must come from earlier results this run.',
      parameters: {
        type: 'object',
        properties: {
          items: {
            type: 'array',
            description: `Apps to add to the Quit list (max ${MEMORY_LIMITS.maxProposeItems}).`,
            items: {
              type: 'object',
              properties: {
                groupId: { type: 'string', description: 'Id from an earlier result this run.' },
                reason: {
                  type: 'string',
                  maxLength: MEMORY_LIMITS.maxReasonChars,
                  description: 'One short sentence: why this app can be quit.',
                },
              },
              required: ['groupId', 'reason'],
              additionalProperties: false,
            },
          },
        },
        required: ['items'],
        additionalProperties: false,
      },
    },
  },
]

export function isMemoryToolName(name: string): name is MemoryToolName {
  return (MEMORY_TOOL_NAMES as readonly string[]).includes(name)
}

// ---------------------------------------------------------------------------
// Run-scoped group memory (the `seenGroups` cap of §8)
// ---------------------------------------------------------------------------

/**
 * One `Map<string, GroupView>` per `ToolRuntime`, bounded to
 * `MEMORY_LIMITS.maxSeenGroups`. `ToolRuntime` is owned by `tools.ts`, so the
 * map hangs off it through a `WeakMap` with the same lifetime.
 */
const seenGroupsByRuntime = new WeakMap<ToolRuntime, Map<string, GroupView>>()

function seenGroups(runtime: ToolRuntime): Map<string, GroupView> {
  let groups = seenGroupsByRuntime.get(runtime)
  if (groups === undefined) {
    groups = new Map()
    seenGroupsByRuntime.set(runtime, groups)
  }
  return groups
}

/** Adds groups to the seen set, refreshing recency and dropping the oldest past the cap. */
export function rememberGroups(runtime: ToolRuntime, groups: readonly GroupView[]): void {
  const seen = seenGroups(runtime)
  for (const group of groups) {
    if (seen.has(group.groupId)) seen.delete(group.groupId)
    seen.set(group.groupId, group)
  }
  while (seen.size > MEMORY_LIMITS.maxSeenGroups) {
    const oldest = seen.keys().next()
    if (oldest.done === true) break
    seen.delete(oldest.value)
  }
}

/** The last view of one group from an earlier result this run, or `undefined`. */
export function knownGroup(runtime: ToolRuntime, groupId: string): GroupView | undefined {
  return seenGroups(runtime).get(groupId)
}

/** Ids the run remembers; exported so tests can pin the cap. */
export function seenGroupCount(runtime: ToolRuntime): number {
  return seenGroups(runtime).size
}

// ---------------------------------------------------------------------------
// Composition
// ---------------------------------------------------------------------------

/** The ports a run was given; at least one is set (enforced by `RunAgentOptions`). */
export interface AdvisorPorts {
  lens?: LensPort
  memory?: MemoryPort
}

/** The tool names a run offers, in composition order. */
export function advisorToolNames(ports: AdvisorPorts): string[] {
  return [...(ports.lens !== undefined ? TOOL_NAMES : []), ...(ports.memory !== undefined ? MEMORY_TOOL_NAMES : [])]
}

/**
 * Runs one tool call for a run that may have a lens, a memory port or both, and
 * always answers with a result. Malformed arguments, unknown tool names,
 * unknown ids, tools the run did not offer and port failures all come back as
 * text — they never throw into the loop.
 */
export async function executeAdvisorTool(
  runtime: ToolRuntime,
  ports: AdvisorPorts,
  name: string,
  argsJson: string,
): Promise<ToolOutcome> {
  const memory = ports.memory
  const lens = ports.lens

  if (isMemoryToolName(name)) {
    if (memory === undefined) return run(runtime, name, async () => errorOutput(name, notOffered(name)))
    return run(runtime, name, () => dispatchMemory(runtime, memory, name, argsJson))
  }
  if ((TOOL_NAMES as readonly string[]).includes(name)) {
    if (lens === undefined) return run(runtime, name, async () => errorOutput(name, notOffered(name)))
    return executeTool(runtime, lens, name, argsJson)
  }
  return run(runtime, name, async () =>
    errorOutput(name, `unknown tool; available: ${advisorToolNames(ports).join(', ')}`),
  )
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

interface HandlerOutput {
  content: string
  summary: string
}

type MemoryHandler = (runtime: ToolRuntime, memory: MemoryPort, args: Record<string, unknown>) => Promise<HandlerOutput>

async function dispatchMemory(
  runtime: ToolRuntime,
  memory: MemoryPort,
  name: MemoryToolName,
  argsJson: string,
): Promise<HandlerOutput> {
  const parsed = parseArgs(argsJson)
  if (!parsed.ok) return errorOutput(name, parsed.error)
  return MEMORY_HANDLERS[name](runtime, memory, parsed.value)
}

const MEMORY_HANDLERS: Record<MemoryToolName, MemoryHandler> = {
  async get_memory_summary(runtime, memory): Promise<HandlerOutput> {
    const summary = await memory.summary()
    const top = summary.top.slice(0, MEMORY_LIMITS.maxTop)
    rememberGroups(runtime, top)
    return {
      content: [`get_memory_summary ${summaryLine(summary)}`, `rows ${ROW_HEADER}`, ...top.map(groupRow)].join('\n'),
      summary: `get_memory_summary: ${summary.pressure.level}, ${top.length} groups`,
    }
  },

  async list_memory_groups(runtime, memory, args): Promise<HandlerOutput> {
    const sort = args.sort === undefined ? 'footprint' : asString(args.sort)
    if (sort !== 'footprint' && sort !== 'cpu') {
      return errorOutput('list_memory_groups', 'sort must be "footprint" or "cpu"')
    }
    const limit = readInt(args.limit, MEMORY_LIMITS.defaultListLimit, 1, MEMORY_LIMITS.maxListLimit)
    if (limit === null) return errorOutput('list_memory_groups', 'limit must be an integer')
    const offset = readInt(args.offset, 0, 0, Number.MAX_SAFE_INTEGER)
    if (offset === null) return errorOutput('list_memory_groups', 'offset must be an integer')

    const page = await memory.groups(sort, limit, offset)
    rememberGroups(runtime, page.rows)
    const remaining = Math.max(0, page.total - offset - page.rows.length)
    return {
      content: [
        `list_memory_groups sort=${sort} total=${page.total} rows=${page.rows.length} remaining=${remaining}`,
        `rows ${ROW_HEADER}`,
        ...page.rows.map(groupRow),
      ].join('\n'),
      summary: `list_memory_groups: ${page.rows.length} rows`,
    }
  },

  async get_memory_group(runtime, memory, args): Promise<HandlerOutput> {
    const groupId = asString(args.groupId)
    if (groupId === null || groupId === '') {
      return errorOutput('get_memory_group', 'groupId must be a non-empty string')
    }
    if (knownGroup(runtime, groupId) === undefined) return unknownGroup('get_memory_group', groupId)

    const detail = await memory.group(groupId)
    if (detail === null) {
      return errorOutput('get_memory_group', `group ${safeId(groupId)} is no longer running`)
    }
    rememberGroups(runtime, [detail.group])
    const members = detail.members.slice(0, MEMORY_LIMITS.maxMembers)
    return {
      content: [
        `get_memory_group ${groupRow(detail.group)}`,
        candidateLine(detail.candidate),
        `members ${MEMBER_HEADER}`,
        ...members.map((member) =>
          [
            sanitizeName(member.name),
            member.footprintBytes === null ? '-' : mb(member.footprintBytes),
            cpuLabel(member.cpuPercent),
          ].join('|'),
        ),
      ].join('\n'),
      summary: `get_memory_group: ${sanitizeName(detail.group.name)}, ${members.length} members`,
    }
  },

  async propose_quit(runtime, memory, args): Promise<HandlerOutput> {
    const rawItems = args.items
    if (!Array.isArray(rawItems)) return errorOutput('propose_quit', 'items must be an array')
    if (rawItems.length > MEMORY_LIMITS.maxProposeItems) {
      return errorOutput('propose_quit', `at most ${MEMORY_LIMITS.maxProposeItems} items per call`)
    }

    const accepted: QuitStageItem[] = []
    const reasons = new Map<string, string>()
    const rejected: { groupId: string; reason: string }[] = []
    const requested = new Set<string>()

    for (const raw of rawItems) {
      const item = asRecord(raw)
      if (item === null) {
        rejected.push({ groupId: '?', reason: 'item must be an object' })
        continue
      }
      const groupId = asString(item.groupId) ?? ''
      if (groupId === '') {
        rejected.push({ groupId: '?', reason: 'groupId must be a non-empty string' })
        continue
      }
      if (requested.has(groupId)) {
        rejected.push({ groupId, reason: 'duplicate item' })
        continue
      }
      requested.add(groupId)
      const reason = asString(item.reason)
      if (reason === null) {
        rejected.push({ groupId, reason: 'reason must be a string' })
        continue
      }
      if (knownGroup(runtime, groupId) === undefined) {
        rejected.push({
          groupId,
          reason: 'unknown groupId: only ids returned by earlier results this run can be staged',
        })
        continue
      }
      // Fresh read: flags can change between the row the model saw and now.
      const detail = await memory.group(groupId)
      if (detail === null) {
        rejected.push({ groupId, reason: 'no longer running' })
        continue
      }
      const refusal = quitRefusal(detail.group)
      if (refusal !== null) {
        rejected.push({ groupId, reason: refusal })
        continue
      }
      const trimmed = sanitizeReason(reason)
      accepted.push({ groupId, reason: trimmed })
      reasons.set(groupId, trimmed)
    }

    let outcome: QuitStageOutcome = { staged: [], rejected: [] }
    if (accepted.length > 0) outcome = await memory.stageQuit(accepted)
    const allRejected = [...rejected, ...outcome.rejected]
    return {
      content: [
        `propose_quit staged=${outcome.staged.length} rejected=${allRejected.length}`,
        ...outcome.staged.map((groupId) => `+ ${safeId(groupId)}|${sanitizeReason(reasons.get(groupId) ?? '')}`),
        ...allRejected.map((entry) => `- ${safeId(entry.groupId)}|${sanitizeReason(entry.reason)}`),
      ].join('\n'),
      summary: `propose_quit: ${outcome.staged.length} staged, ${allRejected.length} rejected`,
    }
  },
}

/** Why this group can never be staged, or `null` when it may be. §8 rule 2. */
function quitRefusal(group: GroupView): string | null {
  if (group.protected) return 'protected: a system or session-critical process'
  if (group.self) return 'self: it holds this app or one of its ancestors'
  if (group.frontmost) return 'frontmost: it is the app in front right now'
  if (group.restricted) return 'restricted: it belongs to another user or the system'
  return null
}

// ---------------------------------------------------------------------------
// Initial window
// ---------------------------------------------------------------------------

export interface MemoryInitialContext {
  /** The compact text placed at the head of the first user message. */
  text: string
  /** Every group mentioned, so `propose_quit` accepts them immediately. */
  groups: GroupView[]
  /** Group rows emitted. */
  rows: number
  /** Bytes actually used. */
  bytes: number
}

/**
 * Builds the model's starting window for a memory-only run: pressure, totals
 * and the 10 largest groups. Costs exactly one `summary` call and never reads
 * a group's members.
 */
export async function buildMemoryInitialContext(memory: MemoryPort): Promise<MemoryInitialContext> {
  const summary = await memory.summary()
  const top = summary.top.slice(0, MEMORY_LIMITS.maxTop)
  const text = [`memory ${summaryLine(summary)}`, `rows ${ROW_HEADER}`, ...top.map(groupRow)].join('\n')
  return { text, groups: top, rows: top.length, bytes: byteLength(text) }
}

// ---------------------------------------------------------------------------
// Row formatting and guards
// ---------------------------------------------------------------------------

/** One group row: `gid|name|MB|cpu|procs|kind|flags`. */
function groupRow(group: GroupView): string {
  return [
    group.groupId,
    sanitizeName(group.name),
    mb(group.footprintBytes),
    cpuLabel(group.cpuPercent),
    group.processCount,
    group.kind,
    flags(group),
  ].join('|')
}

/** `F` frontmost, `P` protected, `S` self, `R` restricted; `-` when none apply. */
function flags(group: GroupView): string {
  const chars = [
    ...(group.frontmost ? ['F'] : []),
    ...(group.protected ? ['P'] : []),
    ...(group.self ? ['S'] : []),
    ...(group.restricted ? ['R'] : []),
  ]
  return chars.length === 0 ? '-' : chars.join('')
}

function cpuLabel(value: number | null): string {
  if (value === null) return '-'
  return value >= 100 ? String(Math.round(value)) : String(Number(value.toFixed(1)))
}

function summaryLine(summary: MemorySummaryView): string {
  const breakdown = summary.breakdown
  return [
    `pressure=${summary.pressure.level}`,
    `usedMB=${mb(breakdown.usedBytes)}`,
    `totalMB=${mb(breakdown.totalBytes)}`,
    `swapMB=${mb(breakdown.swapUsedBytes)}`,
    `compressedMB=${breakdown.compressedBytes === null ? '-' : mb(breakdown.compressedBytes)}`,
    `observedMin=${summary.observedMinutes}`,
  ].join(' ')
}

function candidateLine(candidate: FreeUpCandidate | null): string {
  if (candidate === null) return 'candidate none'
  const codes = candidate.reasons.map((reason) => reason.code).join(',')
  return [
    'candidate',
    `reasons=${codes === '' ? 'none' : codes}`,
    `risks=${candidate.risks.join(',') || 'none'}`,
    `preselected=${candidate.preselected}`,
  ].join(' ')
}

/**
 * One line of untrusted text: newlines and tabs become spaces, control
 * characters and `|` are removed, runs of spaces collapse, and the result is
 * truncated to `maxChars` (§8 rule 3).
 */
export function sanitizeName(name: string, maxChars: number = MEMORY_LIMITS.maxNameChars): string {
  const flat = name
    .replaceAll(/[\r\n\t]+/g, ' ')
    .replaceAll(/[\u0000-\u001f\u007f-\u009f|]/g, '')
    .replaceAll(/\s+/g, ' ')
    .trim()
  return flat.length > maxChars ? `${flat.slice(0, maxChars - 1)}…` : flat
}

function sanitizeReason(reason: string): string {
  return sanitizeName(reason, MEMORY_LIMITS.maxReasonChars)
}

/** Ids are opaque, but a stray newline would break the row format. */
function safeId(id: string): string {
  return id.replaceAll(/[\r\n\t]/g, ' ')
}

function notOffered(name: string): string {
  return `${name} is not available in this run`
}

function unknownGroup(tool: string, groupId: string): HandlerOutput {
  return errorOutput(
    tool,
    `unknown groupId ${safeId(groupId)}: only ids returned by earlier results this run can be used`,
  )
}

function errorOutput(tool: string, message: string): HandlerOutput {
  const line = `error ${tool ? `${tool} ` : ''}${message}`
  return { content: line, summary: line }
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

// ---------------------------------------------------------------------------
// Budget guards (the same counters as tools.ts)
// ---------------------------------------------------------------------------

const MAX_RESULT_BYTES = AGENT_LIMITS.maxResultTokens * AGENT_LIMITS.bytesPerToken

/**
 * Runs one memory handler under the shared `ToolRuntime` counters, exactly as
 * `tools.ts` guards the disk tools: check the budget, count the call, answer
 * every handler error as text, then add the result to the output budget.
 */
async function run(runtime: ToolRuntime, name: string, produce: () => Promise<HandlerOutput>): Promise<ToolOutcome> {
  const guarded = checkBudget(runtime)
  if (guarded !== null) return guarded
  runtime.toolCalls += 1
  let output: HandlerOutput
  try {
    output = await produce()
  } catch (error) {
    output = errorOutput(name, `tool failed: ${describe(error)}`)
  }
  return finish(runtime, output)
}

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

function tokenLabel(tokens: number): string {
  return tokens >= 1000 ? `${tokens / 1000}k` : String(tokens)
}

function describe(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error)
  return message.length > 300 ? `${message.slice(0, 300)}…` : message
}
