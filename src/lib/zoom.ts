/** Text-size presets for ⌘+/⌘−; pinch zoom moves freely between the ends. */
export const FONT_SCALES = [0.85, 0.92, 1, 1.1, 1.2, 1.35, 1.5, 1.7] as const;

const MIN = FONT_SCALES[0];
const MAX = FONT_SCALES[FONT_SCALES.length - 1];

export function clampScale(scale: number): number {
  if (!Number.isFinite(scale)) return 1;
  return Math.round(Math.min(MAX, Math.max(MIN, scale)) * 100) / 100;
}

/** Trackpad pinch arrives as ctrl+wheel; negative deltaY is pinching out. */
export function pinchScale(current: number, deltaY: number): number {
  return clampScale(current * Math.exp(-deltaY * 0.01));
}

/** Next preset above (`1`) or below (`-1`) the current scale. */
export function stepScale(current: number, dir: -1 | 1): number {
  const eps = 0.001;
  const next =
    dir > 0
      ? FONT_SCALES.find((s) => s > current + eps)
      : FONT_SCALES.findLast((s) => s < current - eps);
  return next ?? (dir > 0 ? MAX : MIN);
}
