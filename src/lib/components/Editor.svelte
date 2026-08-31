<script lang="ts">
  import { onMount } from "svelte";
  import {
    EditorView,
    keymap,
    lineNumbers,
    highlightActiveLine,
    highlightActiveLineGutter,
    drawSelection,
    placeholder,
    Decoration,
    type DecorationSet,
  } from "@codemirror/view";
  import { EditorState, StateEffect, StateField, RangeSetBuilder } from "@codemirror/state";
  import { ViewPlugin, type ViewUpdate } from "@codemirror/view";
  import { invoke } from "@tauri-apps/api/core";
  import { toast } from "svelte-sonner";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
  import { languages } from "@codemirror/language-data";
  import { syntaxHighlighting, HighlightStyle } from "@codemirror/language";
  import { tags } from "@lezer/highlight";
  import { app, type CommentDraft } from "$lib/state.svelte";
  import { registerScroller } from "$lib/scrollsync";
  import { locateQuote } from "$lib/comments";
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import CommentHoverCard from "./CommentHoverCard.svelte";

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let applyingExternal = false;

  const mdHighlight = HighlightStyle.define([
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
    { tag: [tags.function(tags.variableName), tags.className], color: "var(--hl-title)" },
  ]);

  // --- search highlighting -------------------------------------------------
  interface Hits {
    ranges: { from: number; to: number }[];
    active: number;
  }
  const setHits = StateEffect.define<Hits>();
  const hitMark = Decoration.mark({ class: "cm-search-hit" });
  const activeHitMark = Decoration.mark({ class: "cm-search-hit cm-search-hit-active" });
  const hitsField = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(deco, tr) {
      deco = deco.map(tr.changes);
      for (const e of tr.effects) {
        if (e.is(setHits)) {
          deco = Decoration.set(
            e.value.ranges.map((r, i) =>
              (i === e.value.active ? activeHitMark : hitMark).range(r.from, r.to),
            ),
          );
        }
      }
      return deco;
    },
    provide: (f) => EditorView.decorations.from(f),
  });

  let hitRanges: { from: number; to: number }[] = [];

  function searchUpdate(query: string): number {
    hitRanges = [];
    if (view && query) {
      const doc = view.state.doc.toString().toLowerCase();
      const q = query.toLowerCase();
      let i = 0;
      while ((i = doc.indexOf(q, i)) !== -1 && hitRanges.length < 2000) {
        hitRanges.push({ from: i, to: i + q.length });
        i += q.length;
      }
    }
    view?.dispatch({ effects: setHits.of({ ranges: hitRanges, active: -1 }) });
    return hitRanges.length;
  }

  function searchGoto(i: number) {
    const r = hitRanges[i];
    if (!view || !r) return;
    view.dispatch({
      effects: [
        setHits.of({ ranges: hitRanges, active: i }),
        EditorView.scrollIntoView(r.from, { y: "center" }),
      ],
    });
  }

  function searchClear() {
    hitRanges = [];
    view?.dispatch({ effects: setHits.of({ ranges: [], active: -1 }) });
  }

  // --- comment highlights in the source ------------------------------------
  const commentMark = Decoration.mark({ class: "cm-comment-hl" });
  const commentMarkActive = Decoration.mark({ class: "cm-comment-hl cm-comment-hl-active" });
  const commentsPlugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(v: EditorView) {
        this.decorations = this.compute(v);
      }
      update(u: ViewUpdate) {
        this.decorations = this.compute(u.view);
      }
      compute(v: EditorView): DecorationSet {
        const text = v.state.doc.toString();
        const found: { from: number; to: number; id: string }[] = [];
        const positions: Record<string, { from: number; to: number }> = {};
        for (const t of app.commentThreads) {
          if (t.resolved) continue;
          const idx = locateQuote(text, t.quote, t.prefix, t.suffix);
          if (idx >= 0) {
            const r = { from: idx, to: idx + t.quote.length };
            found.push({ ...r, id: t.id });
            positions[t.id] = r;
          }
        }
        app.editorThreadPos = positions;
        return Decoration.set(
          found.map((f) =>
            (f.id === app.activeThreadId ? commentMarkActive : commentMark).range(f.from, f.to),
          ),
          true,
        );
      }
    },
    { decorations: (v) => v.decorations },
  );

  let hoverCard = $state<{ id: string; x: number; y: number } | null>(null);
  const hoverThread = $derived.by(() => {
    const hc = hoverCard;
    return hc ? (app.commentThreads.find((t) => t.id === hc.id) ?? null) : null;
  });

  const commentClicks = EditorView.domEventHandlers({
    click(event, v) {
      const pos = v.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos == null) return false;
      for (const [id, r] of Object.entries(app.editorThreadPos)) {
        if (pos >= r.from && pos <= r.to) {
          app.openThread(id);
          break;
        }
      }
      return false;
    },
    mousemove(event, v) {
      const pos = v.posAtCoords({ x: event.clientX, y: event.clientY });
      let found: { id: string; from: number } | null = null;
      if (pos != null) {
        for (const [id, r] of Object.entries(app.editorThreadPos)) {
          if (pos >= r.from && pos <= r.to) {
            found = { id, from: r.from };
            break;
          }
        }
      }
      if (!found) {
        hoverCard = null;
        return false;
      }
      if (hoverCard?.id !== found.id) {
        const coords = v.coordsAtPos(found.from);
        const hostRect = host.getBoundingClientRect();
        if (coords) {
          hoverCard = {
            id: found.id,
            x: Math.max(8, Math.min(coords.left - hostRect.left, hostRect.width - 280)),
            y: coords.bottom - hostRect.top + 6,
          };
        }
      }
      return false;
    },
  });

  // --- select-to-comment ---------------------------------------------------
  let selAction = $state<{ x: number; y: number; draft: CommentDraft } | null>(null);

  function updateSelAction() {
    if (!view || app.isMermaidDoc) return;
    const sel = view.state.selection.main;
    if (sel.empty || sel.to - sel.from > 1000) {
      selAction = null;
      return;
    }
    const coords = view.coordsAtPos(sel.head);
    if (!coords) {
      selAction = null;
      return;
    }
    const hostRect = host.getBoundingClientRect();
    const doc = view.state;
    selAction = {
      x: Math.max(8, Math.min(coords.left - hostRect.left, hostRect.width - 130)),
      y: Math.max(4, coords.top - hostRect.top - 38),
      draft: {
        quote: doc.sliceDoc(sel.from, sel.to),
        prefix: doc.sliceDoc(Math.max(0, sel.from - 30), sel.from),
        suffix: doc.sliceDoc(sel.to, Math.min(doc.doc.length, sel.to + 30)),
      },
    };
  }

  function createComment() {
    if (!selAction) return;
    app.startCommentDraft(selAction.draft);
    selAction = null;
  }

  // --- focus mode: dim every line outside the current paragraph ------------
  const dimLine = Decoration.line({ class: "cm-dim" });
  const focusPlugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(v: EditorView) {
        this.decorations = this.compute(v);
      }
      update(u: ViewUpdate) {
        this.decorations = this.compute(u.view);
      }
      compute(v: EditorView): DecorationSet {
        if (!app.focusMode) return Decoration.none;
        const doc = v.state.doc;
        const cur = doc.lineAt(v.state.selection.main.head).number;
        let start = cur;
        while (start > 1 && doc.line(start - 1).text.trim() !== "") start--;
        let end = cur;
        while (end < doc.lines && doc.line(end + 1).text.trim() !== "") end++;
        const builder = new RangeSetBuilder<Decoration>();
        for (const { from, to } of v.visibleRanges) {
          let pos = from;
          while (pos <= to) {
            const line = doc.lineAt(pos);
            if (line.number < start || line.number > end) {
              builder.add(line.from, line.from, dimLine);
            }
            pos = line.to + 1;
          }
        }
        return builder.finish();
      }
    },
    { decorations: (v) => v.decorations },
  );

  // --- paste images into the document --------------------------------------
  async function handleImagePaste(file: File): Promise<boolean> {
    if (!app.currentPath || !view) return false;
    const ext = (file.type.split("/")[1] ?? "png").replace("jpeg", "jpg");
    const buf = new Uint8Array(await file.arrayBuffer());
    let binary = "";
    for (let i = 0; i < buf.length; i += 0x8000) {
      binary += String.fromCharCode(...buf.subarray(i, i + 0x8000));
    }
    try {
      const rel = await invoke<string>("save_image", {
        docPath: app.currentPath,
        dataB64: btoa(binary),
        ext,
      });
      const pos = view.state.selection.main.head;
      view.dispatch({
        changes: { from: pos, insert: `![](${rel})` },
        selection: { anchor: pos + 4 },
      });
      toast.success(`Image saved to ${rel}`);
    } catch (e) {
      toast.error(`Could not save image: ${e}`);
    }
    return true;
  }

  const pasteImages = EditorView.domEventHandlers({
    paste(event) {
      const file = [...(event.clipboardData?.files ?? [])].find((f) =>
        f.type.startsWith("image/"),
      );
      if (!file) return false;
      event.preventDefault();
      handleImagePaste(file);
      return true;
    },
  });

  // --- TOC scroll spy (writing mode only; preview owns it otherwise) -------
  let spyRaf = 0;
  function onEditorScroll() {
    if (app.viewMode !== "editor" || spyRaf || !view) return;
    spyRaf = requestAnimationFrame(() => {
      spyRaf = 0;
      if (!view) return;
      const block = view.lineBlockAtHeight(view.scrollDOM.scrollTop);
      const topLine = view.state.doc.lineAt(block.from).number;
      let active: string | null = null;
      for (const entry of app.toc) {
        if (entry.line <= topLine + 1) active = entry.id;
        else break;
      }
      app.activeHeadingId = active ?? app.toc[0]?.id ?? null;
    });
  }

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: app.content,
        extensions: [
          lineNumbers(),
          history(),
          drawSelection(),
          highlightActiveLine(),
          highlightActiveLineGutter(),
          EditorView.lineWrapping,
          placeholder("Start writing…"),
          keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
          markdown({ base: markdownLanguage, codeLanguages: languages }),
          syntaxHighlighting(mdHighlight, { fallback: true }),
          hitsField,
          focusPlugin,
          commentsPlugin,
          commentClicks,
          pasteImages,
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !applyingExternal) {
              app.content = update.state.doc.toString();
              app.scheduleAutosave();
            }
            if (update.selectionSet || update.docChanged) {
              requestAnimationFrame(updateSelAction);
            }
          }),
        ],
      }),
    });
    app.editorView = view;
    app.searchBackends.editor = { update: searchUpdate, goto: searchGoto, clear: searchClear };
    const unregisterSync = registerScroller("editor", view.scrollDOM);
    view.scrollDOM.addEventListener("scroll", onEditorScroll, { passive: true });

    return () => {
      unregisterSync();
      if (app.editorView === view) app.editorView = null;
      delete app.searchBackends.editor;
      view?.destroy();
    };
  });

  // Recompute focus dimming / comment decorations when their inputs change.
  $effect(() => {
    app.focusMode;
    app.commentThreads;
    app.activeThreadId;
    view?.dispatch({});
  });

  // Push external content changes (doc switch, disk sync) into the editor.
  $effect(() => {
    const text = app.content;
    app.currentPath;
    if (view && view.state.doc.toString() !== text) {
      applyingExternal = true;
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: text },
      });
      applyingExternal = false;
    }
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="editor-host relative h-full overflow-hidden"
  bind:this={host}
  onmouseleave={() => (hoverCard = null)}
>
  {#if hoverThread && hoverCard}
    <CommentHoverCard thread={hoverThread} x={hoverCard.x} y={hoverCard.y} />
  {/if}
  {#if selAction}
    <button
      class="absolute z-20 flex items-center gap-1.5 rounded-full border bg-popover px-3 py-1.5 text-xs font-medium shadow-md transition-colors hover:bg-accent"
      style="left: {selAction.x}px; top: {selAction.y}px"
      onmousedown={(e) => e.preventDefault()}
      onclick={createComment}
    >
      <MessageSquarePlus class="size-3.5" /> Comment
    </button>
  {/if}
</div>
