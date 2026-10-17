import type { CollectorEntry, TreeNodeSummary } from '@space-lens/contract'
import { canCollectNode, collectRefusalReason } from '@space-lens/web-ui/lib/app-scan'
import { addSelection, selectedAncestor } from '@space-lens/web-ui/lib/selection'
import type { CleanupRisk, StageOutcome, StageRejection, StageRequestItem } from './types.ts'

/**
 * Slice C — the bridge from the agent's `propose_cleanup` to the existing
 * collector.
 *
 * The collector owns the only side effect, exactly as the manual selection
 * path does: entries go through `addSelection` (which itself refuses anything
 * `canCollectNode` rejects and collapses descendants under a staged ancestor).
 * Nothing here plans, executes or deletes; the user still reviews the collector
 * and presses the existing Clean button.
 *
 * `CollectorEntry` is a frozen, strict contract shape, so the AI's reason and
 * risk live in a side map keyed by the entry id — the drawer reads it for the
 * proposal card and the tooltip, and the collector schema is untouched.
 */

/** Reads one node by id; `null` when the host cannot serve it. */
export type NodeResolver = (nodeId: string) => Promise<TreeNodeSummary | null>

export interface StageMeta {
  /** The model's short, human-readable reason for this entry. */
  reason: string
  risk: CleanupRisk
}

export interface StagerOptions {
  scanId: string
  resolve: NodeResolver
  /** The live collector, read before every stage call. */
  collector: () => readonly CollectorEntry[]
  /** Written back after a stage call; the only mutation the stager performs. */
  setCollector: (entries: CollectorEntry[]) => void
  /** A fresh collector id (the page uses `col_` + a uuid slice). */
  newId: () => string
  now?: () => string
}

export interface Stager {
  /** The `LensPort['stage']` implementation handed to `lensPortFromService`. */
  stage: (items: readonly StageRequestItem[]) => Promise<StageOutcome>
  /** AI reason + risk per staged entry id (entry ids are unique per collector). */
  readonly meta: ReadonlyMap<string, StageMeta>
}

/**
 * Tells the model what to do next, not just why it was refused: a partially
 * scanned or contents-only folder cannot be staged whole, but its children can,
 * and that is what the user would do by hand (open it, tick the contents).
 */
export function refusalFor(node: TreeNodeSummary): string {
  const reason = collectRefusalReason(node) ?? 'this node cannot be collected'
  if (node.scanState === 'partial' || /contents-only|contents instead/i.test(reason)) {
    return `${reason}. Do not give up on it: call list_children on ${node.id} and propose its children (files or subfolders) instead.`
  }
  return reason
}

export function createStager(options: StagerOptions): Stager {
  const meta = new Map<string, StageMeta>()

  const stage = async (items: readonly StageRequestItem[]): Promise<StageOutcome> => {
    let entries = [...options.collector()]
    const rejected: StageRejection[] = []
    // The item accepted for each node id, once it landed in the collector.
    const accepted = new Map<string, StageRequestItem>()

    for (const item of items) {
      const node = await options.resolve(item.nodeId)
      if (node === null) {
        rejected.push({ nodeId: item.nodeId, reason: 'the host could not read this node' })
        continue
      }
      if (!canCollectNode(node)) {
        rejected.push({ nodeId: item.nodeId, reason: refusalFor(node) })
        continue
      }
      const covering = selectedAncestor(entries, node.path, options.scanId)
      if (covering) {
        rejected.push({
          nodeId: item.nodeId,
          reason:
            covering.nodeId === node.id ? 'already staged' : `already covered by the staged folder ${covering.name}`,
        })
        continue
      }
      entries = addSelection(
        entries,
        node,
        options.scanId,
        options.now?.() ?? new Date().toISOString(),
        options.newId(),
      )
      const entry = entries.find((candidate) => candidate.nodeId === node.id)
      if (entry === undefined) {
        rejected.push({ nodeId: item.nodeId, reason: 'the collector refused this node' })
        continue
      }
      accepted.set(node.id, item)
    }

    // A later, larger proposal can absorb an earlier child; the collector drops
    // that child, so the model is told its smaller candidate no longer stands.
    for (const [nodeId] of accepted) {
      if (entries.some((entry) => entry.nodeId === nodeId)) continue
      accepted.delete(nodeId)
      rejected.push({ nodeId, reason: 'covered by a larger staged folder' })
    }

    const staged = entries.filter((entry) => accepted.has(entry.nodeId))
    for (const entry of staged) {
      const item = accepted.get(entry.nodeId)
      if (item === undefined) continue
      meta.set(entry.id, { reason: item.reason, risk: item.risk })
    }

    options.setCollector(entries)
    return { staged, rejected }
  }

  return { stage, meta }
}
