/**
 * Packaging smoke tests for the built Kunkun plugin artifacts.
 * The backend bundle must resolve the native scanner from plugin dist so a
 * Kunkun extension install does not depend on the source workspace layout.
 */
import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'

describe('Kunkun plugin packaging', () => {
  test('ships the Space Lens scanner package wrapper next to the backend bundle', () => {
    const dist = path.resolve(import.meta.dir, '../dist')
    const requireFromDist = createRequire(path.join(dist, 'backend.js'))
    const scannerEntry = requireFromDist.resolve('space-lens')

    expect(scannerEntry).toBe(path.join(dist, 'node_modules/space-lens/index.js'))
  })

  test('writes a scanner runtime report for current-platform and release-matrix validation', () => {
    const dist = path.resolve(import.meta.dir, '../dist')
    const report = JSON.parse(readFileSync(path.join(dist, 'scanner-runtime-report.json'), 'utf8')) as {
      packageName: string
      packageVersion: string
      currentPlatform: {
        platform: string
        arch: string
        artifact: string
        bundled: boolean
      }
      expectedArtifacts: string[]
      bundledArtifacts: string[]
      missingArtifacts: string[]
      crossPlatformComplete: boolean
      releaseHint: string
    }

    expect(report.packageName).toBe('space-lens')
    expect(report.currentPlatform.platform).toBe(process.platform)
    expect(report.currentPlatform.arch).toBe(process.arch)
    expect(report.expectedArtifacts).toContain(report.currentPlatform.artifact)
    expect(report.currentPlatform.bundled).toBe(
      report.bundledArtifacts.includes(report.currentPlatform.artifact),
    )
    if (report.currentPlatform.bundled) {
      expect(report.bundledArtifacts).toContain(report.currentPlatform.artifact)
    } else {
      expect(report.missingArtifacts).toContain(report.currentPlatform.artifact)
    }
    expect(report.crossPlatformComplete).toBe(report.missingArtifacts.length === 0)
    expect(report.releaseHint).toContain(
      report.crossPlatformComplete
        ? 'All configured NAPI targets are bundled.'
        : 'SPACE_LENS_REQUIRE_ALL_NATIVE_ARTIFACTS=1',
    )
  })
})
