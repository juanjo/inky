/**
 * HTML → markdown conversion for in-place rich editing in reading mode.
 * Scoped to a single top-level block, so the conversion only ever has to
 * handle inline formatting, headings, lists, quotes and tables.
 */
import TurndownService from "turndown";
// @ts-expect-error no type declarations shipped
import { gfm } from "turndown-plugin-gfm";

const turndown = new TurndownService({
  headingStyle: "atx",
  codeBlockStyle: "fenced",
  bulletListMarker: "-",
  emDelimiter: "*",
  strongDelimiter: "**",
  hr: "---",
});
turndown.use(gfm);

// GFM double-tilde strikethrough (the plugin emits single tildes).
turndown.addRule("inky-strike", {
  filter: ["del", "s"],
  replacement: (content) => `~~${content}~~`,
});

// Tight "- " list markers (turndown pads to four columns) with 2-space nesting.
turndown.addRule("inky-list-item", {
  filter: "li",
  replacement: (content, node, options) => {
    content = content
      .replace(/^\n+/, "")
      .replace(/\n+$/, "\n")
      .replace(/\n/gm, "\n  ");
    let prefix = options.bulletListMarker + " ";
    const parent = node.parentNode as HTMLElement | null;
    if (parent?.nodeName === "OL") {
      const start = parent.getAttribute("start");
      const index = Array.prototype.indexOf.call(parent.children, node);
      prefix = `${start ? Number(start) + index : index + 1}. `;
    }
    return prefix + content + (node.nextSibling && !/\n$/.test(content) ? "\n" : "");
  },
});

// Task-list checkboxes (wins over the gfm plugin's rule, avoiding doubles).
// The preview renders them shadcn-style as <span class="task-box"><input…><svg…></span>;
// matching the wrapper also swallows the check icon. Bare inputs in an <li>
// (e.g. pasted HTML) keep working via the second condition.
const isChecked = (input: Element | null) =>
  !!input && ((input as HTMLInputElement).checked || input.hasAttribute("checked"));
turndown.addRule("inky-task", {
  filter: (node) =>
    (node.nodeName === "SPAN" && node.classList.contains("task-box")) ||
    (node.nodeName === "INPUT" &&
      node.getAttribute("type") === "checkbox" &&
      node.parentNode?.nodeName === "LI"),
  replacement: (_content, node) => {
    const input =
      node.nodeName === "INPUT" ? (node as Element) : (node as Element).querySelector("input");
    return isChecked(input) ? "[x] " : "[ ] ";
  },
});

// Local images are rewritten to asset: URLs for display; write the original
// markdown path back out (stored in data-md-src by the preview).
turndown.addRule("inky-images", {
  filter: "img",
  replacement: (_content, node) => {
    const el = node as HTMLElement;
    const src = el.getAttribute("data-md-src") ?? el.getAttribute("src") ?? "";
    const alt = el.getAttribute("alt") ?? "";
    return `![${alt}](${src})`;
  },
});

export function htmlToMarkdown(html: string): string {
  return (
    turndown
      .turndown(html)
      // The rendered checkbox is followed by a text space; collapse doubles.
      .replace(/^(\s*(?:[-*+]|\d+\.) \[[ xX]\]) +/gm, "$1 ")
  );
}
