/**
 * Task-list toggling for reading mode: flip a `[ ]` / `[x]` marker inside one
 * source block. Pure string logic — the preview supplies which checkbox index
 * was clicked and how many checkboxes the rendered block contains.
 */

/** Task markers: list item (`-`, `*`, `+`, `1.`, `1)`), optional `>` quoting. */
const MARKER = /^(\s*(?:>\s*)*(?:[-*+]|\d+[.)])\s+\[)([ xX])(?=\])/gm;

/**
 * Flip the `index`-th task marker within `content[start..end)`.
 *
 * Returns the updated content, or `null` when the block's markers don't line
 * up with the rendered checkboxes (`expectedCount`) — e.g. a task-looking
 * line inside a fenced code block — so a click never ticks the wrong line.
 * The replacement is always the same length, so surrounding offsets stay valid.
 */
export function toggleTaskMarker(
  content: string,
  start: number,
  end: number,
  index: number,
  expectedCount: number,
): string | null {
  const src = content.slice(start, end);
  const markers = [...src.matchAll(MARKER)];
  if (index < 0 || index >= markers.length || markers.length !== expectedCount) return null;
  const m = markers[index];
  const abs = start + (m.index ?? 0) + m[1].length;
  const flipped = m[2] === " " ? "x" : " ";
  return content.slice(0, abs) + flipped + content.slice(abs + 1);
}
