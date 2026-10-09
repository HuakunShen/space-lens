/** Renderer-independent, bounded two-level membrane physics seeded by ECharts geometry. */
import type { BubbleCircle } from '../bubbles.ts'
import { area, centroid, circlePoints, inside, nearest, reach, resample, type Point } from './geometry.ts'

export const MAX_MEMBRANES = 64
export const MIN_MEMBRANE_RADIUS = 14
export const PARENT_VERTEX_COUNT = 40
export const CHILD_VERTEX_COUNT = 24
const FIXED_STEP = 1 / 120
const WALL_MARGIN = 5
const CONSTRAINT_ITERATIONS = 5
const REST_SOLVE_BUDGET_MS = 24
const REST_SOLVE_MAX_STEPS = 60

interface BodyBase {
  circle: BubbleCircle
  seed: Point
  center: Point
  local: Point
  parentBody: MembraneBody | null
}
interface MembraneBody extends BodyBase {
  kind: 'membrane'
  points: Point[]
  previous: Point[]
  seedPoints: Point[]
  originalPoints: Point[]
  gradients: Point[]
  area: number
  edge: number[]
  bend: number[]
  floor: number
  kids: MembraneBody[]
  passengers: PassengerBody[]
}
interface PassengerBody extends BodyBase {
  kind: 'passenger'
}
type Body = MembraneBody | PassengerBody

export interface SoftBubbleSnapshot {
  id: string
  kind: Body['kind']
  parentId: string | null
  circle: BubbleCircle
  points: Point[]
  cx: number
  cy: number
  r: number
}

function clamp(value: number, low: number, high: number): number {
  return Math.max(low, Math.min(high, value))
}

function createMembrane(circle: BubbleCircle, offset: number, count: number): MembraneBody {
  const seed = { x: circle.x + offset, y: circle.y + offset }
  const points = circlePoints(seed.x, seed.y, circle.r, count)
  return {
    kind: 'membrane',
    circle,
    seed,
    center: { ...seed },
    local: { x: 0, y: 0 },
    parentBody: null,
    points,
    previous: points.map((point) => ({ ...point })),
    seedPoints: points.map((point) => ({ ...point })),
    originalPoints: points.map((point) => ({ ...point })),
    gradients: points.map(() => ({ x: 0, y: 0 })),
    area: area(points),
    edge: points.map(() => 2 * circle.r * Math.sin(Math.PI / count)),
    bend: points.map(() => 2 * circle.r * Math.sin((3 * Math.PI) / count)),
    floor: Infinity,
    kids: [],
    passengers: [],
  }
}

export class SoftBubbleWorld {
  private readonly bodies: Body[] = []
  private readonly membranes: MembraneBody[] = []
  private readonly parents: MembraneBody[] = []
  private readonly byId = new Map<string, Body>()
  private dragging: { body: MembraneBody; point: Point } | null = null
  private active = false
  private releaseSteps = 0
  private pendingDragStep = false

  private compression: Map<string, Point> | null = null
  private readonly offset: number

  constructor(circles: readonly BubbleCircle[], offset = 0, rest: readonly SoftBubbleSnapshot[] = []) {
    this.offset = offset
    const restById = new Map(rest.map((body) => [body.id, body]))
    const parentCircles = new Map<string, BubbleCircle>()
    const circleById = new Map(circles.map((circle) => [circle.id, circle]))
    // Structural IDs work for scanned paths and synthetic aggregate buckets alike.
    for (const circle of circles) {
      for (const child of circle.node.children) {
        if (circleById.has(child.id)) parentCircles.set(child.id, circle)
      }
    }
    for (const child of circles) {
      if (parentCircles.has(child.id)) continue
      let parent: BubbleCircle | null = null
      for (const candidate of circles) {
        if (
          candidate.r <= child.r ||
          Math.hypot(candidate.x - child.x, candidate.y - child.y) + child.r > candidate.r + 0.01
        )
          continue
        if (!parent || candidate.r < parent.r) parent = candidate
      }
      if (parent) parentCircles.set(child.id, parent)
    }
    const top = circles.filter((circle) => !parentCircles.has(circle.id))
    const selected = new Set(top.map((circle) => circle.id))
    const eligible = circles
      .filter((circle) => parentCircles.has(circle.id) && circle.r >= MIN_MEMBRANE_RADIUS)
      .sort((left, right) => right.r - left.r)
    // All first-level bodies remain interactive, even in views exceeding the nominal budget.
    for (const circle of eligible) {
      if (selected.size >= MAX_MEMBRANES) break
      selected.add(circle.id)
    }
    for (const circle of circles) {
      const body: Body = selected.has(circle.id)
        ? createMembrane(circle, offset, parentCircles.has(circle.id) ? CHILD_VERTEX_COUNT : PARENT_VERTEX_COUNT)
        : {
            kind: 'passenger',
            circle,
            seed: { x: circle.x + offset, y: circle.y + offset },
            center: { x: circle.x + offset, y: circle.y + offset },
            local: { x: 0, y: 0 },
            parentBody: null,
          }
      const baseline = restById.get(circle.id)
      if (baseline) {
        body.seed = { x: baseline.cx, y: baseline.cy }
        body.center = { ...body.seed }
        if (body.kind === 'membrane') {
          body.points = baseline.points.map((point) => ({ ...point }))
          body.previous = baseline.points.map((point) => ({ ...point }))
          body.seedPoints = baseline.points.map((point) => ({ ...point }))
          body.originalPoints = circlePoints(circle.x + offset, circle.y + offset, circle.r, body.points.length)
          body.gradients = body.points.map(() => ({ x: 0, y: 0 }))
          body.area = area(body.points)
          const lengths = (skip: number) => body.points.map((p, i) => {
            const q = body.points[(i + skip) % body.points.length]
            return Math.hypot(p.x - q.x, p.y - q.y)
          })
          body.edge = lengths(1)
          body.bend = lengths(3)
        }
      }
      this.bodies.push(body)
      this.byId.set(circle.id, body)
      if (body.kind === 'membrane') this.membranes.push(body)
    }
    for (const body of this.bodies) {
      const parentCircle = parentCircles.get(body.circle.id)
      const parent = parentCircle ? this.byId.get(parentCircle.id) : undefined
      if (parent?.kind === 'membrane') {
        body.parentBody = parent
        body.local = { x: body.seed.x - parent.seed.x, y: body.seed.y - parent.seed.y }
        parent.floor = Math.min(parent.floor, body.seed.y - body.circle.r - 1)
        if (body.kind === 'membrane') parent.kids.push(body)
        else parent.passengers.push(body)
      } else if (body.kind === 'membrane') this.parents.push(body)
    }
  }

  /** Solve POC-style pressure once; ECharts holds these contours without an idle RAF. */
  compressedRest(pressure = 0.55): SoftBubbleSnapshot[] {
    if (!this.parents.length || pressure <= 0) return this.snapshot()
    const center = centroid(this.parents.map((body) => body.seed))
    const factor = 1 - clamp(pressure, 0, 1) * 0.25
    this.compression = new Map(this.parents.map((body) => [body.circle.id, {
      x: center.x + (body.seed.x - center.x) * factor,
      y: center.y + (body.seed.y - center.y) * factor,
    }]))
    // Start at the compressed anchors so even a dense view gets contact
    // deformation in its first step. Warm-up has a small UI-thread budget;
    // dragging still uses the full solver and its frame-time downgrade.
    for (const body of this.bodies) {
      const parent = body.parentBody ?? (body.kind === 'membrane' ? body : null)
      const target = parent ? this.compression.get(parent.circle.id) : undefined
      if (!parent || !target) continue
      const dx = target.x - parent.seed.x
      const dy = target.y - parent.seed.y
      body.center = { x: body.seed.x + dx, y: body.seed.y + dy }
      if (body.kind === 'membrane') {
        for (let i = 0; i < body.points.length; i++) {
          body.points[i].x += dx
          body.points[i].y += dy
          Object.assign(body.previous[i], body.points[i])
        }
      }
    }
    this.active = true
    const deadline = performance.now() + REST_SOLVE_BUDGET_MS
    for (let step = 0; step < REST_SOLVE_MAX_STEPS; step++) {
      this.step()
      if (performance.now() >= deadline) break
    }
    this.compression = null
    this.active = false
    return this.snapshot()
  }

  /** Aggregate and budgeted passenger bubbles keep ordinary ECharts interactions. */
  drag(id: string, point: Point): boolean {
    const body = this.byId.get(id)
    if (body?.kind !== 'membrane' || body.circle.isAggregate || !Number.isFinite(point.x) || !Number.isFinite(point.y))
      return false
    this.dragging = { body, point: { ...point } }
    this.active = true
    this.releaseSteps = 0
    this.pendingDragStep = true
    return true
  }

  release(): void {
    // WebKit can deliver the final move and up before RAF runs. Consume that
    // target while it still exists, so a quick gesture retains its impulse.
    if (this.dragging && this.pendingDragStep) {
      for (let step = 0; step < 3; step += 1) this.step()
    }
    this.dragging = null
    this.releaseSteps = 0
  }

  step(deltaSeconds = FIXED_STEP): void {
    if (!this.active || !Number.isFinite(deltaSeconds) || deltaSeconds <= 0) return
    this.pendingDragStep = false
    const timeScale = clamp(deltaSeconds / FIXED_STEP, 0.1, 3)
    for (const body of this.membranes) {
      const dragging = this.dragging?.body === body
      const target =
        dragging && this.dragging
          ? this.dragging.point
          : body.parentBody
            ? { x: body.parentBody.center.x + body.local.x, y: body.parentBody.center.y + body.local.y }
            : this.compression?.get(body.circle.id) ?? body.seed
      const pull = dragging ? (body.parentBody ? 0.02 : 0.016) : body.parentBody ? 0.006 : 0.0045
      this.integrate(body, target, pull, timeScale)
    }
    for (let iteration = 0; iteration < CONSTRAINT_ITERATIONS; iteration += 1) {
      for (const body of this.membranes) this.constrain(body)
      this.collideGroup(this.parents)
      for (const parent of this.membranes) {
        this.collideGroup(parent.kids)
        for (const child of parent.kids) this.contain(child, parent)
      }
    }
    if (!this.dragging && !this.compression) {
      this.releaseSteps += timeScale
      // A soft shape spring strengthens after the initial free rebound. Its final
      // endpoint is the exact packed contour, avoiding residual contact equilibrium.
      const shapePull = clamp((this.releaseSteps - 120) / 180, 0, 1) ** 3
      if (shapePull > 0) {
        for (const body of this.membranes) {
          for (let index = 0; index < body.points.length; index += 1) {
            const point = body.points[index]
            const seed = body.seedPoints[index]
            point.x += (seed.x - point.x) * shapePull
            point.y += (seed.y - point.y) * shapePull
          }
        }
      }
    }
    for (const body of this.membranes) body.center = centroid(body.points)
    for (const parent of this.membranes) this.positionPassengers(parent)
    if (!this.dragging && !this.compression && this.isAtRest()) this.resetToSeed()
  }

  isAtRest(): boolean {
    if (this.dragging) return false
    for (const body of this.membranes) {
      for (let index = 0; index < body.points.length; index += 1) {
        const point = body.points[index]
        const seed = body.seedPoints[index]
        const previous = body.previous[index]
        if (
          Math.hypot(point.x - seed.x, point.y - seed.y) > 0.35 ||
          Math.hypot(point.x - previous.x, point.y - previous.y) > 0.12
        )
          return false
      }
    }
    for (const body of this.bodies) {
      if (body.kind === 'passenger' && Math.hypot(body.center.x - body.seed.x, body.center.y - body.seed.y) > 0.35)
        return false
    }
    return true
  }

  snapshot(sampleCount = 64): SoftBubbleSnapshot[] {
    return this.bodies.map((body) => {
      let points = circlePoints(body.circle.x + this.offset, body.circle.y + this.offset, body.circle.r, sampleCount)
      if (body.kind === 'membrane') {
        const displacements = resample(
          body.points.map((point, index) => ({
            x: point.x - body.originalPoints[index].x,
            y: point.y - body.originalPoints[index].y,
          })),
          sampleCount,
        )
        points = points.map((point, index) => ({
          x: point.x + displacements[index].x,
          y: point.y + displacements[index].y,
        }))
      } else points = circlePoints(body.center.x, body.center.y, body.circle.r, sampleCount)
      return {
        id: body.circle.id,
        kind: body.kind,
        parentId: body.parentBody?.circle.id ?? null,
        circle: body.circle,
        points,
        cx: body.center.x,
        cy: body.center.y,
        r: body.circle.r,
      }
    })
  }

  private resetToSeed(): void {
    for (const body of this.bodies) {
      body.center = { ...body.seed }
      if (body.kind === 'membrane') {
        for (let index = 0; index < body.points.length; index += 1) {
          Object.assign(body.points[index], body.seedPoints[index])
          Object.assign(body.previous[index], body.seedPoints[index])
        }
      }
    }
    this.active = false
  }

  private integrate(body: MembraneBody, target: Point, pull: number, timeScale: number): void {
    const center = centroid(body.points)
    const ax = clamp(target.x - center.x, -90, 90) * pull * timeScale * timeScale
    const ay = clamp(target.y - center.y, -90, 90) * pull * timeScale * timeScale
    for (let index = 0; index < body.points.length; index += 1) {
      const point = body.points[index]
      const previous = body.previous[index]
      const x = point.x
      const y = point.y
      const seed = body.seedPoints[index]
      point.x +=
        clamp((x - previous.x) * 0.88 ** timeScale, -4, 4) +
        ax +
        (center.x + seed.x - body.seed.x - x) * 0.001
      point.y +=
        clamp((y - previous.y) * 0.88 ** timeScale, -4, 4) +
        ay +
        (center.y + seed.y - body.seed.y - y) * 0.001
      previous.x = x
      previous.y = y
    }
  }

  private distance(body: MembraneBody, left: number, right: number, rest: number, stiffness: number): void {
    const a = body.points[left]
    const b = body.points[right]
    const dx = b.x - a.x
    const dy = b.y - a.y
    const length = Math.sqrt(dx * dx + dy * dy) || 1
    const correction = ((length - rest) / length) * 0.5 * stiffness
    a.x += dx * correction
    a.y += dy * correction
    b.x -= dx * correction
    b.y -= dy * correction
  }

  private constrain(body: MembraneBody): void {
    const count = body.points.length
    for (let index = 0; index < count; index += 1) this.distance(body, index, (index + 1) % count, body.edge[index], 0.78)
    for (let index = 0; index < count; index += 1) this.distance(body, index, (index + 3) % count, body.bend[index], 0.055)
    let norm = 0
    for (let index = 0; index < count; index += 1) {
      const previous = body.points[(index + count - 1) % count]
      const next = body.points[(index + 1) % count]
      const gradient = body.gradients[index]
      gradient.x = (next.y - previous.y) / 2
      gradient.y = (previous.x - next.x) / 2
      norm += gradient.x ** 2 + gradient.y ** 2
    }
    const correction = clamp((body.area - area(body.points)) / (norm + 1e-8), -0.045, 0.045) * 0.76
    for (let index = 0; index < count; index += 1) {
      body.points[index].x += body.gradients[index].x * correction
      body.points[index].y += body.gradients[index].y * correction
    }
  }

  private collideGroup(group: readonly MembraneBody[]): void {
    // Compute bounds once, rather than walking every polygon for every
    // distant pair. Refresh after nearby reactions to keep the broad phase valid.
    const circles = group.map((body) => reach(body.points))
    for (let left = 0; left < group.length; left += 1) {
      for (let right = left + 1; right < group.length; right += 1) {
        const a = circles[left]
        const b = circles[right]
        const radius = a.radius + b.radius + 2
        if ((a.x - b.x) ** 2 + (a.y - b.y) ** 2 > radius ** 2) continue
        this.contact(group[left], group[right])
        this.contact(group[right], group[left])
        circles[left] = reach(group[left].points)
        circles[right] = reach(group[right].points)
      }
    }
  }

  private contact(a: MembraneBody, b: MembraneBody): void {
    const ac = reach(a.points)
    const bc = reach(b.points)
    const distance = Math.hypot(ac.x - bc.x, ac.y - bc.y)
    if (distance > a.circle.r + b.circle.r + 25 || distance > ac.radius + bc.radius) return
    let radiusSquared = bc.radius ** 2
    for (const point of a.points) {
      if ((point.x - bc.x) ** 2 + (point.y - bc.y) ** 2 > radiusSquared || !inside(point, b.points)) continue
      const hit = nearest(point, b.points)
      const length = hit.distance || 1
      const scale = Math.min(5, length + 1.6) / length
      const dx = (hit.x - point.x) * scale
      const dy = (hit.y - point.y) * scale
      const u = b.points[hit.index]
      const v = b.points[(hit.index + 1) % b.points.length]
      const weightA = 1 - hit.t
      const denominator = 1 + weightA ** 2 + hit.t ** 2
      point.x += dx / denominator
      point.y += dy / denominator
      u.x -= (dx * weightA) / denominator
      u.y -= (dy * weightA) / denominator
      v.x -= (dx * hit.t) / denominator
      v.y -= (dy * hit.t) / denominator
      radiusSquared = Math.max(
        radiusSquared,
        (u.x - bc.x) ** 2 + (u.y - bc.y) ** 2,
        (v.x - bc.x) ** 2 + (v.y - bc.y) ** 2,
      )
    }
  }

  private contain(child: MembraneBody, parent: MembraneBody): void {
    let center = centroid(parent.points)
    let safe = inside(center, parent.points) ? nearest(center, parent.points).distance - WALL_MARGIN : -1
    const floor = parent.floor + center.y - parent.seed.y
    for (const point of child.points) {
      point.y = Math.max(point.y, floor)
      if (safe > 0 && (point.x - center.x) ** 2 + (point.y - center.y) ** 2 < safe ** 2) continue
      const hit = nearest(point, parent.points)
      if (inside(point, parent.points) && hit.distance >= WALL_MARGIN) continue
      const dx = hit.x - hit.nx * WALL_MARGIN - point.x
      const dy = hit.y - hit.ny * WALL_MARGIN - point.y
      const scale = Math.min(5, Math.hypot(dx, dy)) / (Math.hypot(dx, dy) || 1)
      point.x += dx * scale * 0.95
      point.y += dy * scale * 0.95
      const a = parent.points[hit.index]
      const b = parent.points[(hit.index + 1) % parent.points.length]
      a.x -= dx * scale * 0.035 * (1 - hit.t)
      a.y -= dy * scale * 0.035 * (1 - hit.t)
      b.x -= dx * scale * 0.035 * hit.t
      b.y -= dy * scale * 0.035 * hit.t
      // Reaction moved the wall; refresh the exact guaranteed-inside disc.
      center = centroid(parent.points)
      safe = inside(center, parent.points) ? nearest(center, parent.points).distance - WALL_MARGIN : -1
    }
  }

  private positionPassengers(parent: MembraneBody): void {
    const centerInside = inside(parent.center, parent.points)
    const safeRadius = centerInside ? nearest(parent.center, parent.points).distance : -1
    for (const passenger of parent.passengers) {
      const point = { x: parent.center.x + passenger.local.x, y: parent.center.y + passenger.local.y }
      point.y = Math.max(point.y, parent.floor + parent.center.y - parent.seed.y + passenger.circle.r)
      const need = passenger.circle.r + 1
      if (safeRadius < 0 || Math.hypot(point.x - parent.center.x, point.y - parent.center.y) + need >= safeRadius) {
        const hit = nearest(point, parent.points)
        if (!inside(point, parent.points) || hit.distance < need) {
          point.x = hit.x - hit.nx * need
          point.y = hit.y - hit.ny * need
        }
      }
      passenger.center = point
    }
  }
}
