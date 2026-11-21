import { z } from 'zod'

const PathSchema = z.string().min(1).max(16384)

/**
 * Folder browsing for the scan picker.
 *
 * The host lists one directory at a time inside the roots it already serves,
 * so a browser (or a remote workbench reached over SSH) can drill to a folder
 * instead of typing an absolute path it cannot see. Listing is never
 * recursive: sizes are the entry's own length, not a measured subtree.
 */
export const FsEntryKindSchema = z.enum(['dir', 'file', 'other'])

export type FsEntryKind = z.infer<typeof FsEntryKindSchema>

export const FsEntrySchema = z.strictObject({
  name: z.string().min(1).max(1024),
  /** Absolute path, already resolved through symlinks. */
  path: PathSchema,
  kind: FsEntryKindSchema,
  /** Entry length in bytes; null when the entry could not be stat'ed. */
  size: z.number().int().nonnegative().nullable(),
})

export type FsEntry = z.infer<typeof FsEntrySchema>

export const FsListRequestSchema = z.strictObject({
  /** Absolute folder or `~/…`; omitted means the first served root. */
  path: PathSchema.optional(),
  /** Entry cap; the host clamps it to its own maximum. */
  limit: z.number().int().positive().max(5000).optional(),
})

export type FsListRequest = z.infer<typeof FsListRequestSchema>

export const FsListResponseSchema = z.strictObject({
  /** The folder that was listed, resolved through symlinks. */
  path: PathSchema,
  /**
   * The folder above, or null at a served root. The breadcrumb uses it to
   * stop walking up where the host stops serving.
   */
  parent: PathSchema.nullable(),
  /** Roots this host serves, so the picker can jump between them. */
  roots: z.array(PathSchema),
  entries: z.array(FsEntrySchema),
  /** True when the folder held more entries than the limit. */
  truncated: z.boolean(),
})

export type FsListResponse = z.infer<typeof FsListResponseSchema>
