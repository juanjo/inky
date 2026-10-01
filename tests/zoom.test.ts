import { describe, expect, it } from "vitest";
import { FONT_SCALES, clampScale, pinchScale, stepScale } from "$lib/zoom";

const MIN = FONT_SCALES[0];
const MAX = FONT_SCALES[FONT_SCALES.length - 1];

describe("zoom", () => {
  it("clamps to the preset range and rounds to two decimals", () => {
    expect(clampScale(0.1)).toBe(MIN);
    expect(clampScale(9)).toBe(MAX);
    expect(clampScale(1.23456)).toBe(1.23);
    expect(clampScale(Number.NaN)).toBe(1);
  });

  it("pinching out (negative deltaY) zooms in, pinching in zooms out", () => {
    expect(pinchScale(1, -10)).toBeGreaterThan(1);
    expect(pinchScale(1, 10)).toBeLessThan(1);
    expect(pinchScale(MAX, -50)).toBe(MAX);
  });

  it("steps to the next preset, even from a pinched in-between size", () => {
    expect(stepScale(1, 1)).toBe(1.1);
    expect(stepScale(1, -1)).toBe(0.92);
    expect(stepScale(1.04, 1)).toBe(1.1);
    expect(stepScale(1.04, -1)).toBe(1);
    expect(stepScale(MAX, 1)).toBe(MAX);
    expect(stepScale(MIN, -1)).toBe(MIN);
  });
});
