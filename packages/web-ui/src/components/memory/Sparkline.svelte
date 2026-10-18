<script lang="ts" module>
  /**
   * Sparkline geometry helpers. Kept in the module context so the pressure
   * card and the process drawer share one path builder and cannot drift.
   */
  export interface SparklineBounds {
    min: number
    max: number
  }

  /** Finite bounds over one or more series; a flat series gets a 1-unit window. */
  export function sparklineBounds(series: ReadonlyArray<ReadonlyArray<number | null>>): SparklineBounds {
    let min = Number.POSITIVE_INFINITY
    let max = Number.NEGATIVE_INFINITY
    for (const values of series) {
      for (const value of values) {
        if (value === null || !Number.isFinite(value)) continue
        if (value < min) min = value
        if (value > max) max = value
      }
    }
    if (!Number.isFinite(min) || !Number.isFinite(max)) return { min: 0, max: 1 }
    if (min === max) return { min: min - 1, max: max + 1 }
    return { min, max }
  }

  /**
   * One SVG path for a series, `null` values breaking the line into separate
   * subpaths. X runs over `count` samples regardless of the series length so a
   * shorter comparison series stays anchored to the right edge.
   */
  export function sparklinePath(
    values: ReadonlyArray<number | null>,
    bounds: SparklineBounds,
    count: number,
    width: number,
    height: number,
  ): string {
    const span = Math.max(count - 1, 1)
    const range = bounds.max - bounds.min
    const scaled = (value: number): number => height - ((value - bounds.min) / range) * height
    let path = ''
    let drawing = false
    for (let index = 0; index < values.length; index += 1) {
      const value = values[index] ?? null
      if (value === null || !Number.isFinite(value)) {
        drawing = false
        continue
      }
      const x = (index / span) * width
      const y = scaled(value)
      path += `${drawing ? 'L' : 'M'}${x.toFixed(2)} ${y.toFixed(2)}`
      drawing = true
    }
    return path
  }
</script>

<script lang="ts">
  /**
   * A minimal, non-interactive memory sparkline (§6.2). Pure presentation: it
   * takes arrays of samples and never fetches anything. Used + compressed are
   * drawn as two `currentColor` series so the caller controls the colours with
   * Tailwind text utilities. `prefers-reduced-motion` never matters here because
   * the sparkline has no transitions.
   */
  interface Props {
    /** Primary series, oldest first; `null` breaks the line. */
    values: ReadonlyArray<number | null>
    /** Optional secondary series drawn behind the primary (e.g. compressed bytes). */
    compare?: ReadonlyArray<number | null>
    height?: number
    /** Accessible description, e.g. `Used memory, last 10 minutes`. */
    label: string
    class?: string
  }

  let { values, compare = [], height = 40, label, class: className = '' }: Props = $props()

  const WIDTH = 100
  let count = $derived(Math.max(values.length, compare.length))
  let bounds = $derived(sparklineBounds([values, compare]))
  let primaryPath = $derived(sparklinePath(values, bounds, count, WIDTH, height))
  let comparePath = $derived(compare.length > 0 ? sparklinePath(compare, bounds, count, WIDTH, height) : '')
</script>

<div class={`text-primary ${className}`} style={`height: ${height}px`}>
  <svg
    class="h-full w-full"
    viewBox={`0 0 ${WIDTH} ${height}`}
    preserveAspectRatio="none"
    role="img"
    aria-label={label}
  >
    {#if comparePath !== ''}
      <path
        d={comparePath}
        fill="none"
        stroke="currentColor"
        stroke-width="1"
        stroke-linejoin="round"
        vector-effect="non-scaling-stroke"
        class="text-muted-foreground/50"
      />
    {/if}
    {#if primaryPath !== ''}
      <path
        d={primaryPath}
        fill="none"
        stroke="currentColor"
        stroke-width="1"
        stroke-linejoin="round"
        vector-effect="non-scaling-stroke"
      />
    {/if}
  </svg>
</div>
