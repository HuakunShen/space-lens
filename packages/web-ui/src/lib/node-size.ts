import { formatBytes } from './format.ts'
import type { TreeNodeSummary } from '../types.ts'

export function formatNodeSize(node: Pick<TreeNodeSummary, 'size' | 'scanState' | 'skipReason'> | null | undefined): string {
  if (!node) return ''
  if (node.skipReason === 'duplicate-directory') return 'Counted elsewhere'
  return node.scanState === 'skipped' ? 'Unknown' : formatBytes(node.size)
}

export function formatNodeName(node: Pick<TreeNodeSummary, 'name' | 'path'>, macOS = false): string {
  return node.name || (node.path === '/' ? macOS ? 'Macintosh HD' : 'Filesystem' : node.path)
}
