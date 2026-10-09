/** Keep hue families while guaranteeing readable white text on every surface. */
export function chartSurfaceColor(source: string): string {
  const hsl = /^hsl\(([\d.]+),\s*([\d.]+)%,\s*([\d.]+)%\)$/.exec(source)
  let rgb: number[]
  if (hsl) {
    const hue = Number(hsl[1]) / 60
    const saturation = Number(hsl[2]) / 100
    const lightness = Number(hsl[3]) / 100
    const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation
    const second = chroma * (1 - Math.abs((hue % 2) - 1))
    const offset = lightness - chroma / 2
    const components =
      hue < 1
        ? [chroma, second, 0]
        : hue < 2
          ? [second, chroma, 0]
          : hue < 3
            ? [0, chroma, second]
            : hue < 4
              ? [0, second, chroma]
              : hue < 5
                ? [second, 0, chroma]
                : [chroma, 0, second]
    rgb = components.map((component) => (component + offset) * 255)
  } else if (/^#[\da-f]{6}$/i.test(source)) {
    rgb = [1, 3, 5].map((start) => Number.parseInt(source.slice(start, start + 2), 16))
  } else {
    rgb = [65, 70, 79]
  }
  // A little headroom above 4.5:1 also protects antialiased small labels.
  while (luminance(rgb) > 0.15) rgb = rgb.map((channel) => channel * 0.97)
  return hex(rgb)
}

export function chartMaterial(source: string) {
  const color = chartSurfaceColor(source)
  const darker = hex([1, 3, 5].map((start) => Number.parseInt(color.slice(start, start + 2), 16) * 0.83))
  return {
    type: 'linear' as const,
    x: 0,
    y: 0,
    x2: 0.8,
    y2: 1,
    colorStops: [
      { offset: 0, color },
      { offset: 1, color: darker },
    ],
  }
}

/** Parent circles are quiet containers; their contents carry the stronger color. */
export function bubbleMaterial(source: string, parent: boolean) {
  const color = chartSurfaceColor(source)
  const tint = parent ? 0.3 : 0.85
  return chartMaterial(
    hex([1, 3, 5].map((start) => Number.parseInt(color.slice(start, start + 2), 16) * tint + 36 * (1 - tint))),
  )
}

/** This style is shared by the static chart and its temporary soft-body overlay. */
export function bubbleStyle(source: string, parent: boolean, selected: boolean, highlighted = false) {
  return {
    fill: bubbleMaterial(source, parent),
    stroke: highlighted
      ? 'rgba(255,255,255,0.9)'
      : selected
        ? '#fff'
        : parent
          ? 'rgba(255,255,255,0.16)'
          : 'rgba(255,255,255,0.3)',
    lineWidth: highlighted ? 1.5 : selected ? 2 : 1,
  }
}

/** Convert ECharts' relative gradient coordinates into a Canvas bounding box. */
export function gradientInBounds(
  gradient: ReturnType<typeof chartMaterial>,
  box: { x: number; y: number; width: number; height: number },
) {
  return {
    start: { x: box.x + gradient.x * box.width, y: box.y + gradient.y * box.height },
    end: { x: box.x + gradient.x2 * box.width, y: box.y + gradient.y2 * box.height },
    colorStops: gradient.colorStops,
  }
}

function hex(rgb: number[]): string {
  return `#${rgb.map((channel) => Math.round(channel).toString(16).padStart(2, '0')).join('')}`
}

function luminance(rgb: number[]): number {
  const linear = rgb.map((channel) => {
    const value = channel / 255
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  })
  return (linear[0] ?? 0) * 0.2126 + (linear[1] ?? 0) * 0.7152 + (linear[2] ?? 0) * 0.0722
}
