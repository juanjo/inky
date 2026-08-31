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
  import { app } from "$lib/state.svelte";
  import { registerScroller } from "$lib/scrollsync";

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
          pasteImages,
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !applyingExternal) {
              app.content = update.state.doc.toString();
              app.scheduleAutosave();
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

  // Recompute focus-mode dimming when the toggle flips.
  $effect(() => {
    app.focusMode;
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

<div class="editor-host h-full overflow-hidden" bind:this={host}></div>
