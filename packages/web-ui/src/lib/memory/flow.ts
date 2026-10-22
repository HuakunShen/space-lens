/**
 * The Svelte Flow node shape of the memory diagram.
 *
 * Kept apart from `lib/memory/graph.ts` so the layout module stays free of any
 * drawing library: the graph only knows numbers, this type knows how those
 * numbers are handed to `@xyflow/svelte`.
 */
import type { Node } from '@xyflow/svelte'
import type { GraphNode } from './graph.ts'

export type ProcessFlowNode = Node<{ node: GraphNode }, 'process'>
