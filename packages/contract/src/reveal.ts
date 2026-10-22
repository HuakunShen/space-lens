import { z } from 'zod'

/**
 * What "show me this path" means:
 *
 * - `reveal` — show the item *in its parent folder*, selected/highlighted.
 *   This is what a file manager's own "Reveal in Finder" does: the parent
 *   window opens with the item picked out.
 * - `open` — open the item itself in the file manager, so a folder shows its
 *   contents. Folders only: opening a *file* would launch it, which a disk
 *   tool must never do by accident.
 */
export const REVEAL_ACTIONS = ['reveal', 'open'] as const

export type RevealAction = (typeof REVEAL_ACTIONS)[number]

/**
 * Reveal a path the scan already reported in the *host's* own file manager
 * (Finder / Explorer / the desktop's file manager).
 *
 * The path is data, never a command: the host resolves it against the roots
 * it serves and hands it to the platform's file-manager launcher as one
 * argument. A path outside those roots is refused, never opened.
 */
export const RevealPathRequestSchema = z.strictObject({
  /** Absolute path of a scanned file or folder. */
  path: z.string().min(1),
  /** Defaults to `reveal`. */
  action: z.enum(REVEAL_ACTIONS).optional(),
})

export type RevealPathRequest = z.infer<typeof RevealPathRequestSchema>

/** The host opened the file manager; it never reports what the user did there. */
export const RevealPathResultSchema = z.strictObject({
  revealed: z.boolean(),
})

export type RevealPathResult = z.infer<typeof RevealPathResultSchema>
