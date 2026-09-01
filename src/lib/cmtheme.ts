import { HighlightStyle } from "@codemirror/language";
import { tags } from "@lezer/highlight";

/** Shared CodeMirror token colors, driven by the theme's CSS variables. */
export const inkyHighlightStyle = HighlightStyle.define([
  { tag: tags.heading, fontWeight: "700", color: "var(--foreground)" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strong, fontWeight: "700" },
  { tag: tags.strikethrough, textDecoration: "line-through" },
  { tag: tags.link, color: "var(--prose-link)" },
  { tag: tags.url, color: "var(--prose-link)" },
  { tag: tags.quote, color: "var(--muted-foreground)", fontStyle: "italic" },
  { tag: tags.monospace, color: "var(--hl-title)" },
  { tag: tags.meta, color: "var(--muted-foreground)" },
  { tag: tags.processingInstruction, color: "var(--muted-foreground)" },
  { tag: tags.comment, color: "var(--hl-comment)", fontStyle: "italic" },
  { tag: tags.keyword, color: "var(--hl-keyword)" },
  { tag: tags.string, color: "var(--hl-string)" },
  { tag: tags.number, color: "var(--hl-number)" },
  { tag: tags.attributeName, color: "var(--hl-attr)" },
  { tag: tags.typeName, color: "var(--hl-title)" },
  { tag: tags.propertyName, color: "var(--hl-attr)" },
  { tag: [tags.function(tags.variableName), tags.className], color: "var(--hl-title)" },
]);
