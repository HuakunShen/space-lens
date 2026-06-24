/**
 * Defense-in-depth filesystem path policy for the Kunkun Space Lens backend.
 * Kunkun derives this process sandbox from scoped fs-read grants before spawn;
 * this module enforces the approved roots again before scanner or cleanup calls
 * reach the shared SpaceLensAPI implementation.
 */
import { existsSync, realpathSync } from 'node:fs'
import path from 'node:path'

export const BACKEND_FS_READ_ROOTS_ENV = 'KUNKUN_BACKEND_FS_READ_ROOTS'

export class PathPolicyError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'PathPolicyError'
  }
}

export function readAllowedRootsFromEnv(env: NodeJS.ProcessEnv = process.env): string[] {
  const raw = env[BACKEND_FS_READ_ROOTS_ENV]
  if (!raw) return []

  try {
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed) || !parsed.every((entry) => typeof entry === 'string')) {
      throw new PathPolicyError(`${BACKEND_FS_READ_ROOTS_ENV} must be a JSON string array`)
    }
    return parsed
  } catch (error) {
    if (error instanceof PathPolicyError) throw error
    throw new PathPolicyError(`${BACKEND_FS_READ_ROOTS_ENV} is not valid JSON`)
  }
}

export function normalizeAllowedRoots(roots: readonly string[]): string[] {
  const normalized = new Set<string>()

  for (const root of roots) {
    const candidate = stripRecursiveSuffix(root.trim())
    if (!candidate) continue
    if (!path.isAbsolute(candidate)) {
      throw new PathPolicyError(`Allowed root must be absolute: ${root}`)
    }
    normalized.add(normalizeForPolicy(candidate))
  }

  return [...normalized]
}

export function assertPathUnderAllowedRoots(
  candidatePath: string,
  allowedRoots: readonly string[],
  label = 'path',
): void {
  if (allowedRoots.length === 0) {
    throw new PathPolicyError('No approved filesystem roots were provided to the Space Lens backend')
  }
  if (!path.isAbsolute(candidatePath)) {
    throw new PathPolicyError(`${label} must be absolute: ${candidatePath}`)
  }

  const normalizedCandidate = normalizeForPolicy(candidatePath)
  const allowed = allowedRoots.some((root) => isSameOrInside(root, normalizedCandidate))
  if (!allowed) {
    throw new PathPolicyError(`${label} is outside the approved Space Lens roots: ${candidatePath}`)
  }
}

export function assertPathsUnderAllowedRoots(
  candidatePaths: readonly string[],
  allowedRoots: readonly string[],
  label = 'path',
): void {
  for (const candidatePath of candidatePaths) {
    assertPathUnderAllowedRoots(candidatePath, allowedRoots, label)
  }
}

function stripRecursiveSuffix(input: string): string {
  return input.endsWith('/**') ? input.slice(0, -3) : input
}

function normalizeForPolicy(input: string): string {
  const resolved = path.resolve(input)
  if (!existsSync(resolved)) return resolved
  return realpathSync.native(resolved)
}

function isSameOrInside(root: string, candidate: string): boolean {
  if (candidate === root) return true
  const relative = path.relative(root, candidate)
  return relative.length > 0 && !relative.startsWith('..') && !path.isAbsolute(relative)
}
