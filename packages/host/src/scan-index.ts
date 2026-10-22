import { createHash } from 'node:crypto'
import { basename, sep } from 'node:path'

import type { LocalNodeState, TreeNodeSummary } from '@space-lens/contract'

/** Wire ids stay `sha1(path)` truncated to 24 hex characters (12 bytes). */
export function nodeIdOf(path: string): string {
  return createHash('sha1').update(path).digest('hex').slice(0, 24)
}

const DIGEST_BYTES = 12
const NONE = -1

const IGNORED = 1 << 0
const COLLAPSED = 1 << 1
const HAS_IS_DIRECTORY = 1 << 2
const IS_DIRECTORY = 1 << 3
const STATE_SHIFT = 4
const STATE_MASK = 3 << STATE_SHIFT
const HAS_SKIP_REASON = 1 << 6
const HAS_LOGICAL_SIZE = 1 << 7
/** The stored name is the whole path (roots, or a path that is not parent + sep + name). */
const ABSOLUTE_NAME = 1 << 8
/** Added by background discovery; never part of the original scan's children. */
const DISCOVERED = 1 << 9
const HAS_FINGERPRINT = 1 << 10
const HAS_CLEANUP = 1 << 11
const CLEANUP_IGNORED = 1 << 12

const STATES: readonly (LocalNodeState | undefined)[] = [undefined, 'complete', 'partial', 'skipped']

export interface Fingerprint {
  size: number
  mtimeMs: number
}

export interface NodeInput {
  path: string
  size: number
  ignored: boolean
  collapsed: boolean
  logicalSize?: number
  isDirectory?: boolean
  scanState?: LocalNodeState
  skipReason?: string | null
  fingerprint?: Fingerprint
}

/** What cleanup planning needs from a node: its own values or discovery's override. */
export interface CleanupView {
  path: string
  size: number
  ignored: boolean
  fingerprint: Fingerprint | undefined
}

function grow<T extends Int32Array | Uint16Array | Uint32Array | Float64Array>(array: T, capacity: number): T {
  const next = new (array.constructor as new (length: number) => T)(capacity)
  next.set(array)
  return next
}

function trim<T extends Int32Array | Uint16Array | Uint32Array | Float64Array>(array: T, length: number): T {
  return array.length === length ? array : (array.slice(0, length) as T)
}

/**
 * Columnar scan index: one row per node in typed arrays, names in a single
 * UTF-8 buffer, paths rebuilt from parent links, and an open-addressing table
 * from the 12-byte id digest to the row. A Map of per-node objects cost
 * ~550 B/node; this costs ~70 B/node plus the name bytes.
 *
 * Children are a prepend-only linked list per parent, so appending in
 * preorder needs no per-node tail pointer; reads reverse it back into
 * insertion order.
 */
export class ScanIndex {
  count = 0
  readonly roots: number[] = []
  private capacity: number
  private digests: Buffer
  private parents: Int32Array
  private firstChild: Int32Array
  private nextSibling: Int32Array
  private childCounts: Uint32Array
  private sizes: Float64Array
  private logicalSizes: Float64Array | null = null
  private flags: Uint16Array
  private nameEnds: Uint32Array
  private names: Buffer
  private nameBytes = 0
  /** Names that do not round-trip through UTF-8 (lone surrogates) stay as strings. */
  private readonly rawNames = new Map<number, string>()
  private readonly skipReasons = new Map<number, string>()
  private fingerprints: Float64Array | null = null
  private cleanup: Float64Array | null = null
  private slots: Int32Array
  private slotMask: number

  constructor(capacity = 1024) {
    this.capacity = Math.max(16, capacity)
    this.digests = Buffer.alloc(this.capacity * DIGEST_BYTES)
    this.parents = new Int32Array(this.capacity)
    this.firstChild = new Int32Array(this.capacity)
    this.nextSibling = new Int32Array(this.capacity)
    this.childCounts = new Uint32Array(this.capacity)
    this.sizes = new Float64Array(this.capacity)
    this.flags = new Uint16Array(this.capacity)
    this.nameEnds = new Uint32Array(this.capacity)
    this.names = Buffer.allocUnsafeSlow(this.capacity * 16)
    let slots = 16
    while (slots < this.capacity * 2) slots *= 2
    this.slots = new Int32Array(slots).fill(NONE)
    this.slotMask = slots - 1
  }

  /**
   * Adds one node under `parent` (NONE for a root) and returns its row, or
   * NONE when its id is already present. `parentPath` must be the parent's
   * full path. Nodes must arrive parent-first.
   */
  append(input: NodeInput, parent: number, parentPath: string | null, discovered = false): number {
    const digest = createHash('sha1').update(input.path).digest()
    if (this.findDigest(digest, 0) !== NONE) return NONE
    if (this.count === this.capacity) this.reserve(this.capacity * 2)
    const row = this.count
    this.count += 1
    digest.copy(this.digests, row * DIGEST_BYTES, 0, DIGEST_BYTES)
    this.insertSlot(row)

    let flags = (input.ignored ? IGNORED : 0) | (input.collapsed ? COLLAPSED : 0) | (discovered ? DISCOVERED : 0)
    if (input.isDirectory !== undefined) flags |= HAS_IS_DIRECTORY | (input.isDirectory ? IS_DIRECTORY : 0)
    if (input.scanState !== undefined) flags |= STATES.indexOf(input.scanState) << STATE_SHIFT
    if (input.skipReason !== undefined) {
      flags |= HAS_SKIP_REASON
      if (input.skipReason !== null) this.skipReasons.set(row, input.skipReason)
    }
    if (input.logicalSize !== undefined) {
      flags |= HAS_LOGICAL_SIZE
      this.logicalSizes ??= new Float64Array(this.capacity)
      this.logicalSizes[row] = input.logicalSize
    }
    if (input.fingerprint !== undefined) {
      flags |= HAS_FINGERPRINT
      this.fingerprints ??= new Float64Array(this.capacity * 2)
      this.fingerprints[row * 2] = input.fingerprint.size
      this.fingerprints[row * 2 + 1] = input.fingerprint.mtimeMs
    }

    let name = parentPath === null ? null : childName(input.path, parentPath)
    if (name === null) {
      flags |= ABSOLUTE_NAME
      name = input.path
    }
    this.writeName(row, name)
    this.flags[row] = flags
    this.sizes[row] = input.size
    this.parents[row] = parent
    this.firstChild[row] = NONE
    this.nextSibling[row] = NONE
    if (parent === NONE) {
      if (!discovered) this.roots.push(row)
    } else if (!discovered || (this.flags[parent] & DISCOVERED) !== 0) {
      // Discovery never changes the children the original scan reported.
      this.nextSibling[row] = this.firstChild[parent]
      this.firstChild[parent] = row
      this.childCounts[parent] += 1
    }
    return row
  }

  /** Releases growth slack once the scan is complete. */
  seal(): void {
    if (this.capacity === this.count) return
    const length = Math.max(1, this.count)
    this.capacity = length
    this.digests = Buffer.from(this.digests.subarray(0, length * DIGEST_BYTES))
    this.parents = trim(this.parents, length)
    this.firstChild = trim(this.firstChild, length)
    this.nextSibling = trim(this.nextSibling, length)
    this.childCounts = trim(this.childCounts, length)
    this.sizes = trim(this.sizes, length)
    this.flags = trim(this.flags, length)
    this.nameEnds = trim(this.nameEnds, length)
    if (this.logicalSizes !== null) this.logicalSizes = trim(this.logicalSizes, length)
    if (this.fingerprints !== null) this.fingerprints = trim(this.fingerprints, length * 2)
    if (this.cleanup !== null) this.cleanup = trim(this.cleanup, length * 3)
    this.names = Buffer.from(this.names.subarray(0, this.nameBytes))
  }

  find(id: string): number {
    if (id.length !== DIGEST_BYTES * 2) return NONE
    for (let index = 0; index < id.length; index += 1) {
      const code = id.charCodeAt(index)
      if (!((code >= 48 && code <= 57) || (code >= 97 && code <= 102))) return NONE
    }
    return this.findDigest(Buffer.from(id, 'hex'), 0)
  }

  has(id: string): boolean {
    return this.find(id) !== NONE
  }

  idOf(row: number): string {
    return this.digests.toString('hex', row * DIGEST_BYTES, (row + 1) * DIGEST_BYTES)
  }

  parentOf(row: number): number {
    return this.parents[row]
  }

  sizeOf(row: number): number {
    return this.sizes[row]
  }

  childCount(row: number): number {
    return this.childCounts[row]
  }

  /** Children in the order the scan reported them. */
  childrenOf(row: number): number[] {
    const children: number[] = []
    for (let child = this.firstChild[row]; child !== NONE; child = this.nextSibling[child]) children.push(child)
    return children.reverse()
  }

  isIgnored(row: number): boolean {
    return (this.flags[row] & IGNORED) !== 0
  }

  /** `undefined` when the scanner did not report it. */
  isDirectory(row: number): boolean | undefined {
    const flags = this.flags[row]
    return (flags & HAS_IS_DIRECTORY) === 0 ? undefined : (flags & IS_DIRECTORY) !== 0
  }

  scanStateOf(row: number): LocalNodeState | undefined {
    return STATES[(this.flags[row] & STATE_MASK) >> STATE_SHIFT]
  }

  isDiscovered(row: number): boolean {
    return (this.flags[row] & DISCOVERED) !== 0
  }

  /** Distance from the node's root, counted along parent links. */
  depthOf(row: number): number {
    let depth = 0
    for (let cursor = this.parents[row]; cursor !== NONE; cursor = this.parents[cursor]) depth += 1
    return depth
  }

  /** The stored name segment: the basename, or the whole path for roots. */
  private nameOf(row: number): string {
    const raw = this.rawNames.get(row)
    if (raw !== undefined) return raw
    return this.names.toString('utf8', row === 0 ? 0 : this.nameEnds[row - 1], this.nameEnds[row])
  }

  baseNameOf(row: number): string {
    const name = this.nameOf(row)
    return (this.flags[row] & ABSOLUTE_NAME) === 0 ? name : basename(name)
  }

  pathOf(row: number): string {
    const segments: string[] = []
    let cursor = row
    while ((this.flags[cursor] & ABSOLUTE_NAME) === 0) {
      segments.push(this.nameOf(cursor))
      cursor = this.parents[cursor]
    }
    let path = this.nameOf(cursor)
    for (let index = segments.length - 1; index >= 0; index -= 1) path = joinChild(path, segments[index])
    return path
  }

  summarize(row: number): TreeNodeSummary {
    const flags = this.flags[row]
    const childCount = this.childCounts[row]
    const state = STATES[(flags & STATE_MASK) >> STATE_SHIFT]
    return {
      id: this.idOf(row),
      name: this.baseNameOf(row),
      path: this.pathOf(row),
      size: this.sizes[row],
      depth: this.depthOf(row),
      ignored: (flags & IGNORED) !== 0,
      collapsed: (flags & COLLAPSED) !== 0,
      hasChildren: childCount > 0,
      childCount,
      ...((flags & HAS_LOGICAL_SIZE) === 0 ? {} : { logicalSize: this.logicalSizes![row] }),
      ...((flags & HAS_IS_DIRECTORY) === 0 ? {} : { isDirectory: (flags & IS_DIRECTORY) !== 0 }),
      ...(state === undefined ? {} : { scanState: state }),
      ...((flags & HAS_SKIP_REASON) === 0 ? {} : { skipReason: this.skipReasons.get(row) ?? null }),
    }
  }

  /** Discovery's own measurement of an indexed node, used only for cleanup planning. */
  setCleanup(row: number, size: number, ignored: boolean, fingerprint: Fingerprint): void {
    this.cleanup ??= new Float64Array(this.capacity * 3)
    this.cleanup[row * 3] = size
    this.cleanup[row * 3 + 1] = fingerprint.size
    this.cleanup[row * 3 + 2] = fingerprint.mtimeMs
    this.flags[row] = (this.flags[row] & ~CLEANUP_IGNORED) | HAS_CLEANUP | (ignored ? CLEANUP_IGNORED : 0)
  }

  cleanupView(row: number): CleanupView {
    const flags = this.flags[row]
    const path = this.pathOf(row)
    if ((flags & HAS_CLEANUP) !== 0) {
      const cleanup = this.cleanup!
      return {
        path,
        size: cleanup[row * 3],
        ignored: (flags & CLEANUP_IGNORED) !== 0,
        fingerprint: { size: cleanup[row * 3 + 1], mtimeMs: cleanup[row * 3 + 2] },
      }
    }
    return {
      path,
      size: this.sizes[row],
      ignored: (flags & IGNORED) !== 0,
      fingerprint:
        (flags & HAS_FINGERPRINT) === 0
          ? undefined
          : { size: this.fingerprints![row * 2], mtimeMs: this.fingerprints![row * 2 + 1] },
    }
  }

  private writeName(row: number, name: string): void {
    const start = this.nameBytes
    if (!(name as string & { isWellFormed(): boolean }).isWellFormed()) {
      this.rawNames.set(row, name)
    } else {
      const needed = start + name.length * 3
      if (needed > this.names.length) {
        let length = this.names.length * 2
        while (length < needed) length *= 2
        const names = Buffer.allocUnsafeSlow(length)
        this.names.copy(names, 0, 0, start)
        this.names = names
      }
      this.nameBytes += this.names.write(name, start, 'utf8')
    }
    this.nameEnds[row] = this.nameBytes
  }

  private reserve(capacity: number): void {
    this.capacity = capacity
    const digests = Buffer.alloc(capacity * DIGEST_BYTES)
    this.digests.copy(digests)
    this.digests = digests
    this.parents = grow(this.parents, capacity)
    this.firstChild = grow(this.firstChild, capacity)
    this.nextSibling = grow(this.nextSibling, capacity)
    this.childCounts = grow(this.childCounts, capacity)
    this.sizes = grow(this.sizes, capacity)
    this.flags = grow(this.flags, capacity)
    this.nameEnds = grow(this.nameEnds, capacity)
    if (this.logicalSizes !== null) this.logicalSizes = grow(this.logicalSizes, capacity)
    if (this.fingerprints !== null) this.fingerprints = grow(this.fingerprints, capacity * 2)
    if (this.cleanup !== null) this.cleanup = grow(this.cleanup, capacity * 3)
    if (this.slots.length < capacity * 2) {
      let slots = this.slots.length
      while (slots < capacity * 2) slots *= 2
      this.slots = new Int32Array(slots).fill(NONE)
      this.slotMask = slots - 1
      for (let row = 0; row < this.count; row += 1) this.insertSlot(row)
    }
  }

  private insertSlot(row: number): void {
    let slot = this.digests.readUInt32LE(row * DIGEST_BYTES) & this.slotMask
    while (this.slots[slot] !== NONE) slot = (slot + 1) & this.slotMask
    this.slots[slot] = row
  }

  private findDigest(digest: Uint8Array, offset: number): number {
    let slot =
      (digest[offset] | (digest[offset + 1] << 8) | (digest[offset + 2] << 16) | (digest[offset + 3] << 24)) &
      this.slotMask
    for (;;) {
      const row = this.slots[slot]
      if (row === NONE) return NONE
      if (
        this.digests.compare(digest, offset, offset + DIGEST_BYTES, row * DIGEST_BYTES, (row + 1) * DIGEST_BYTES) === 0
      )
        return row
      slot = (slot + 1) & this.slotMask
    }
  }
}

function joinChild(parent: string, name: string): string {
  return parent.endsWith(sep) ? parent + name : parent + sep + name
}

/** The segment that rebuilds `path` from `parentPath`, or null when it does not. */
function childName(path: string, parentPath: string): string | null {
  const prefixLength = parentPath.endsWith(sep) ? parentPath.length : parentPath.length + 1
  if (path.length <= prefixLength || !path.startsWith(parentPath)) return null
  if (prefixLength > parentPath.length && path[parentPath.length] !== sep) return null
  const name = path.slice(prefixLength)
  return name.includes(sep) || name.includes('/') ? null : name
}
