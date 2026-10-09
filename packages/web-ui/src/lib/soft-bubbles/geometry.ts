/** Pure polygon geometry shared by the membrane solver and its canvas renderer. */
export interface Point {
  x: number
  y: number
}

export interface EdgeHit extends Point {
  index: number
  t: number
  distance: number
  /** Outward normal for a positively oriented polygon. */
  nx: number
  ny: number
}

export function area(points: readonly Point[]): number {
  let result = 0
  for (let index = 0; index < points.length; index += 1) {
    const next = points[(index + 1) % points.length]
    result += points[index].x * next.y - points[index].y * next.x
  }
  return result / 2
}

/** Vertex mean, intentionally matching the solver's center of motion. */
export function centroid(points: readonly Point[]): Point {
  let x = 0
  let y = 0
  for (const point of points) {
    x += point.x
    y += point.y
  }
  return points.length ? { x: x / points.length, y: y / points.length } : { x: 0, y: 0 }
}

/** A bounding circle that contains every edge as well as every vertex. */
export function reach(points: readonly Point[]): Point & { radius: number } {
  const center = centroid(points)
  let squaredRadius = 0
  for (const point of points) {
    squaredRadius = Math.max(squaredRadius, (point.x - center.x) ** 2 + (point.y - center.y) ** 2)
  }
  return { ...center, radius: Math.sqrt(squaredRadius) }
}

export function inside(point: Point, polygon: readonly Point[]): boolean {
  let result = false
  for (let index = 0, previous = polygon.length - 1; index < polygon.length; previous = index++) {
    const a = polygon[index]
    const b = polygon[previous]
    if (a.y > point.y !== b.y > point.y && point.x < ((b.x - a.x) * (point.y - a.y)) / (b.y - a.y) + a.x) {
      result = !result
    }
  }
  return result
}

export function nearest(point: Point, polygon: readonly Point[]): EdgeHit {
  let hit: EdgeHit = { x: point.x, y: point.y, index: 0, t: 0, distance: Infinity, nx: 0, ny: 0 }
  for (let index = 0; index < polygon.length; index += 1) {
    const a = polygon[index]
    const b = polygon[(index + 1) % polygon.length]
    const dx = b.x - a.x
    const dy = b.y - a.y
    const lengthSquared = dx * dx + dy * dy
    const t = lengthSquared
      ? Math.max(0, Math.min(1, ((point.x - a.x) * dx + (point.y - a.y) * dy) / lengthSquared))
      : 0
    const x = a.x + dx * t
    const y = a.y + dy * t
    const distance = Math.sqrt((x - point.x) ** 2 + (y - point.y) ** 2)
    if (distance < hit.distance) {
      const length = Math.sqrt(lengthSquared) || 1
      hit = { x, y, index, t, distance, nx: dy / length, ny: -dx / length }
    }
  }
  return hit
}

export function circlePoints(x: number, y: number, radius: number, count = 64): Point[] {
  return Array.from({ length: Math.max(0, Math.floor(count)) }, (_, index) => {
    const angle = (index * Math.PI * 2) / count
    return { x: x + Math.cos(angle) * radius, y: y + Math.sin(angle) * radius }
  })
}

/** Interpolate by vertex index so angular correspondence is retained. */
export function resample(points: readonly Point[], count = 64): Point[] {
  if (!points.length) return []
  return Array.from({ length: Math.max(0, Math.floor(count)) }, (_, index) => {
    const position = (index * points.length) / count
    const base = Math.floor(position)
    const fraction = position - base
    const a = points[base]
    const b = points[(base + 1) % points.length]
    return { x: a.x + (b.x - a.x) * fraction, y: a.y + (b.y - a.y) * fraction }
  })
}

export function bounds(points: readonly Point[]): { x: number; y: number; width: number; height: number } {
  if (!points.length) return { x: 0, y: 0, width: 0, height: 0 }
  let x = Infinity
  let y = Infinity
  let right = -Infinity
  let bottom = -Infinity
  for (const point of points) {
    x = Math.min(x, point.x)
    y = Math.min(y, point.y)
    right = Math.max(right, point.x)
    bottom = Math.max(bottom, point.y)
  }
  return { x, y, width: right - x, height: bottom - y }
}

/** Same midpoint quadratic contour as Canvas, consumed by ECharts' morphable path. */
export function bubblePath(points: readonly Point[]): string {
  const first = points[0]
  const last = points.at(-1)
  if (!first || !last) return ''
  let path = `M${(last.x + first.x) / 2},${(last.y + first.y) / 2}`
  for (let i = 0; i < points.length; i++) {
    const p = points[i]
    const q = points[(i + 1) % points.length]
    path += `Q${p.x},${p.y},${(p.x + q.x) / 2},${(p.y + q.y) / 2}`
  }
  return `${path}Z`
}
