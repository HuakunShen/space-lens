import type { ScanTarget } from '../types.ts'

/** A recent path is not evidence that the current host permits that root. */
export function scanTargetPaths(target: ScanTarget, targets: ScanTarget[]): string[] {
  if (target.path === '/' && targets.some((item) => item.path === '/System/Volumes/Data' && item.source !== 'recent')) {
    return ['/', '/System/Volumes/Data']
  }
  return [target.path]
}
