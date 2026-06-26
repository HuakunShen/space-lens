/**
 * Kunkun backend process for Space Lens.
 * The custom-view frontend talks to this process over Kunkun's backend kkrpc
 * relay, while the standalone NPX host exposes the same SpaceLensAPI over
 * WebSocket.
 *
 * The Kunkun backend helper is resolved through a local pnpm link while the API
 * package is unpublished.
 */
import { exposeBackend } from '@kunkunsh/sdk/backend'
import { createSpaceLensAPI } from '@space-lens/cli/web-service'
import type { SpaceLensAPI } from '@space-lens/cli/web-api'
import {
  assertPathsUnderAllowedRoots,
  normalizeAllowedRoots,
  readAllowedRootsFromEnv,
} from './path-policy.js'

const api = createKunkunSpaceLensAPI()

exposeBackend<SpaceLensAPI>(api)

function createKunkunSpaceLensAPI(): SpaceLensAPI {
  const allowedRoots = normalizeAllowedRoots(readAllowedRootsFromEnv())
  const api = createSpaceLensAPI()

  return {
    ...api,
    async startScan(options) {
      assertPathsUnderAllowedRoots(options.paths, allowedRoots, 'scan path')
      return api.startScan(options)
    },
    async planCleanup(options) {
      assertPathsUnderAllowedRoots(
        options.entries.map((entry) => entry.path),
        allowedRoots,
        'cleanup path',
      )
      return api.planCleanup(options)
    },
    async executeCleanup(options) {
      assertPathsUnderAllowedRoots(
        options.entries.map((entry) => entry.path),
        allowedRoots,
        'cleanup path',
      )
      throw new Error('Native cleanup is disabled in the Kunkun backend; use host-mediated trash instead.')
    },
  }
}
