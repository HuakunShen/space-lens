import type { HostPlatform, RevealAction } from '@space-lens/contract'

/**
 * The label for the host's own file manager.
 *
 * The platform is the *host's*, not the browser's: for the HTTP host that is
 * the server that ran the scan, which is the machine whose Finder (or
 * Explorer, or file manager) the folder lives on.
 *
 * `reveal` shows the item in its parent folder, selected — what a file
 * manager's own "Reveal in Finder" does. `open` opens a folder in the file
 * manager itself.
 */
export function fileManagerLabel(platform: HostPlatform | null | undefined, action: RevealAction = 'reveal'): string {
  const name =
    platform === 'macos'
      ? 'Finder'
      : platform === 'windows'
        ? 'Explorer'
        : platform === 'linux'
          ? 'File Manager'
          : 'file manager'
  return action === 'open' ? `Open in ${name}` : `Reveal in ${name}`
}
