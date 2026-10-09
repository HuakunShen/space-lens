/**
 * The content box a chart lays its geometry out in, in CSS pixels.
 *
 * Charts build their geometry against the box they are actually painted in
 * rather than a fixed design canvas: a fixed canvas only gets scaled by the
 * SVG, which leaves row heights and label room unchanged however large the
 * window grows — the reason a dense tree used to render as unreadable
 * slivers no matter how much space the workbench had.
 */
export interface ChartSize {
  width: number
  height: number
}
