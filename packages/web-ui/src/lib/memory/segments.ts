/**
 * Segment-bar geometry and colour ramp for the Memory view (§6.2 of
 * docs/memory-cleaner-plan.md): the top groups by footprint as a share of used
 * memory, colour-mapped by rank on a pink -> purple -> teal oklch ramp so the
 * bar reads as a gradient. Identity lives in the label, never in the colour.
 *
 * Pure functions only; no Svelte, no transport.
 */
import type { ProcessGroup } from '@space-lens/contract'

export interface Segment {
  groupId: string | null
  label: string
  bytes: number
  share: number
  rank: number
}

interface Oklch {
  l: number
  c: number
  h: number
}

function srgbToLinear(channel: number): number {
  return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
}

/** sRGB hex -> OKLCH (D65). */
function hexToOklch(hex: string): Oklch {
  const packed = Number.parseInt(hex.slice(1), 16)
  const r = srgbToLinear(((packed >> 16) & 0xff) / 255)
  const g = srgbToLinear(((packed >> 8) & 0xff) / 255)
  const b = srgbToLinear((packed & 0xff) / 255)
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
  const lightness = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
  const axisB = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
  return {
    l: lightness,
    c: Math.hypot(a, axisB),
    h: ((Math.atan2(axisB, a) * 180) / Math.PI + 360) % 360,
  }
}

const PINK = hexToOklch('#f04fd0')
const PURPLE = hexToOklch('#9b5cff')
const TEAL = hexToOklch('#19d3c5')

function clamp01(value: number): number {
  return value < 0 ? 0 : value > 1 ? 1 : value
}

function lerp(from: number, to: number, t: number): number {
  return from + (to - from) * t
}

/** Interpolate hue along the shorter arc so the ramp never sweeps the long way round. */
function lerpHue(from: number, to: number, t: number): number {
  const delta = ((to - from + 540) % 360) - 180
  return (from + delta * t + 360) % 360
}

/** Colour for a rank out of `count`, as a CSS `oklch()` string: pink -> purple -> teal. */
export function segmentColor(rank: number, count: number): string {
  const t = count <= 1 ? 0 : clamp01(rank / (count - 1))
  const [from, to, local] = t <= 0.5 ? [PINK, PURPLE, t * 2] : [PURPLE, TEAL, (t - 0.5) * 2]
  const lightness = lerp(from.l, to.l, local)
  const chroma = lerp(from.c, to.c, local)
  const hue = lerpHue(from.h, to.h, local)
  return `oklch(${(lightness * 100).toFixed(2)}% ${chroma.toFixed(3)} ${hue.toFixed(1)})`
}

/**
 * Lay out the segment bar: the `max` largest groups as shares of `usedBytes`,
 * each at least `minPx` wide, with everything else (the tail beyond `max` and
 * any group thinner than `minPx`) folded into one `Other (n)` segment.
 */
export function layoutSegments(
  groups: readonly ProcessGroup[],
  usedBytes: number,
  widthPx: number,
  max = 24,
  minPx = 2,
): Segment[] {
  if (groups.length === 0 || usedBytes <= 0 || widthPx <= 0 || max <= 0 || minPx <= 0) return []
  const ordered = [...groups].sort(
    (a, b) => b.footprintBytes - a.footprintBytes || (a.groupId < b.groupId ? -1 : a.groupId > b.groupId ? 1 : 0),
  )
  const kept: ProcessGroup[] = []
  const dropped: ProcessGroup[] = []
  for (let index = 0; index < ordered.length; index += 1) {
    const group = ordered[index]!
    if (index >= max || (group.footprintBytes / usedBytes) * widthPx < minPx) {
      dropped.push(group)
      continue
    }
    kept.push(group)
  }
  const segments: Segment[] = kept.map((group, rank) => ({
    groupId: group.groupId,
    label: group.name,
    bytes: group.footprintBytes,
    share: group.footprintBytes / usedBytes,
    rank,
  }))
  if (dropped.length > 0) {
    const bytes = dropped.reduce((sum, group) => sum + group.footprintBytes, 0)
    segments.push({
      groupId: null,
      label: `Other (${dropped.length})`,
      bytes,
      share: bytes / usedBytes,
      rank: kept.length,
    })
  }
  return segments
}
