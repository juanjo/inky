import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { toast } from "svelte-sonner";
import { EditorView } from "@codemirror/view";
import type { ThemeName, TreeNode, ViewMode } from "./types";
import { WELCOME_DOC, MERMAID_TEMPLATE } from "./templates";
import { extractToc, type TocEntry } from "./markdown";
import { lockSync } from "./scrollsync";
import { newId, type CommentThread } from "./comments";

export type { CommentThread };
export type CommentFilter = "all" | "open" | "resolved";

export interface CommentDraft {
  quote: string;
  prefix: string;
  suffix: string;
}

export type { TocEntry };

const THEMES: ThemeName[] = ["light", "dark", "book"];

export const FONT_SCALES = [0.85, 0.92, 1, 1.1, 1.2, 1.35, 1.5, 1.7] as const;

export const READING_WIDTHS = {
  default: { label: "Default", value: "58rem" },
  wide: { label: "Wide", value: "66rem" },
  xwide: { label: "Wider", value: "76rem" },
  xxwide: { label: "Widest", value: "88rem" },
  full: { label: "Full", value: "100%" },
} as const;
export type ReadingWidth = keyof typeof READING_WIDTHS;

export interface SearchBackend {
  /** Recompute matches for a query; returns the match count. */
  update(query: string): number;
  /** Scroll to and highlight match `i` (0-based). */
  goto(i: number): void;
  clear(): void;
}

class AppState {
  libraryRoot = $state("");
  tree = $state<TreeNode[]>([]);
  currentPath = $state<string | null>(null);
  content = $state("");
  savedContent = $state("");
  theme = $state<ThemeName>("light");
  viewMode = $state<ViewMode>("split");
  fontScale = $state(1);
  readingWidth = $state<ReadingWidth>("default");
  tocVisible = $state(false);
  activeHeadingId = $state<string | null>(null);
  syncScroll = $state(true);
  searchOpen = $state(false);
  sidebarVisible = $state(true);
  focusMode = $state(false);
  quickOpenVisible = $state(false);
  librarySearchVisible = $state(false);
  /** Query the in-document search bar should run once it opens. */
  pendingSearchQuery: string | null = null;
  /** URL of the app-hosted MCP server while it's running. */
  mcpUrl = $state<string | null>(null);

  /** Disk mtime of the open document at last read/write (conflict detection). */
  #docMtime: number | null = null;

  commentsVisible = $state(false);
  commentThreads = $state<CommentThread[]>([]);
  commentFilter = $state<CommentFilter>("open");
  commentDraft = $state<CommentDraft | null>(null);
  activeThreadId = $state<string | null>(null);
  /** Flat-text position of each open thread's anchor in the preview. */
  threadOrder = $state<Record<string, number>>({});
  /** Source position of each open thread's anchor in the editor. */
  editorThreadPos: Record<string, { from: number; to: number }> = {};

  openCommentCount = $derived(this.commentThreads.filter((t) => !t.resolved).length);

  #preFocus: { viewMode: ViewMode; sidebar: boolean; toc: boolean } | null = null;

  /** Set by the Editor component while mounted; used for TOC + search. */
  editorView: EditorView | null = null;
  searchBackends: { editor?: SearchBackend; preview?: SearchBackend } = {};

  dirty = $derived(this.currentPath !== null && this.content !== this.savedContent);
  docName = $derived(this.currentPath?.split("/").pop() ?? "");
  /** .mmd files are standalone mermaid diagrams. */
  isMermaidDoc = $derived(this.docName.toLowerCase().endsWith(".mmd"));
  toc = $derived<TocEntry[]>(
    this.currentPath && !this.isMermaidDoc ? extractToc(this.content) : [],
  );
  wordCount = $derived(
    this.currentPath && !this.isMermaidDoc ? (this.content.match(/\S+/g)?.length ?? 0) : 0,
  );
  readingMinutes = $derived(Math.max(1, Math.round(this.wordCount / 220)));
  /** Flat list of every document, for the quick-open switcher. */
  flatDocs = $derived.by(() => {
    const out: { name: string; path: string; rel: string }[] = [];
    const prefix = this.libraryRoot.length + 1;
    const walk = (nodes: TreeNode[]) => {
      for (const n of nodes) {
        if (n.isDir) walk(n.children);
        else out.push({ name: n.name, path: n.path, rel: n.path.slice(prefix) });
      }
    };
    walk(this.tree);
    return out;
  });

  #autosaveTimer: ReturnType<typeof setTimeout> | null = null;

  async init() {
    const storedTheme = localStorage.getItem("inky.theme") as ThemeName | null;
    if (storedTheme && THEMES.includes(storedTheme)) this.theme = storedTheme;
    const storedView = localStorage.getItem("inky.viewMode") as ViewMode | null;
    if (storedView) this.viewMode = storedView;
    const storedScale = parseFloat(localStorage.getItem("inky.fontScale") ?? "");
    if (FONT_SCALES.includes(storedScale as (typeof FONT_SCALES)[number])) {
      this.fontScale = storedScale;
    }
    const storedWidth = localStorage.getItem("inky.readingWidth") as ReadingWidth | null;
    if (storedWidth && storedWidth in READING_WIDTHS) this.readingWidth = storedWidth;
    this.tocVisible = localStorage.getItem("inky.tocVisible") === "true";
    this.syncScroll = localStorage.getItem("inky.syncScroll") !== "false";
    this.sidebarVisible = localStorage.getItem("inky.sidebar") !== "false";
    this.applyTheme();
    this.applyReadingPrefs();

    this.libraryRoot = await invoke<string>("library_root");
    await this.refreshTree();

    if (this.tree.length === 0) {
      const path = await invoke<string>("create_doc", {
        dir: this.libraryRoot,
        name: "Welcome to Inky",
        ext: "md",
        content: WELCOME_DOC,
      });
      await this.refreshTree();
      await this.openDoc(path);
    } else {
      const last = localStorage.getItem("inky.lastDoc");
      if (last) await this.openDoc(last, { silent: true });
    }

    // Pick up documents created outside the app (e.g. via the MCP server).
    window.addEventListener("focus", () => this.syncFromDisk());
    // Never let a debounced autosave die with the focus.
    window.addEventListener("blur", () => {
      if (this.dirty) this.save();
    });
    // Red-button close: flush unsaved changes first.
    getCurrentWindow().onCloseRequested(async (event) => {
      if (this.dirty) {
        event.preventDefault();
        await this.save();
        getCurrentWindow().destroy();
      }
    });

    this.syncMenuChecks();
    // Silent startup update check (no-op until an update endpoint is live).
    this.checkForUpdates(false);
    if (localStorage.getItem("inky.mcpAutostart") === "true") {
      this.startMcpServer(false);
    }
    // Keep the MCP status light honest even if the process dies externally.
    setInterval(async () => {
      if (this.mcpUrl && !(await invoke<boolean>("mcp_status").catch(() => false))) {
        this.mcpUrl = null;
        toast.error("MCP server stopped unexpectedly");
      }
    }, 10_000);
  }

  /** ⌘Q — save, then exit. */
  async quitApp() {
    if (this.dirty) await this.save();
    await invoke("quit_app");
  }

  async startMcpServer(announce: boolean) {
    try {
      const url = await invoke<string>("start_mcp", { port: 26317 });
      this.mcpUrl = url;
      localStorage.setItem("inky.mcpAutostart", "true");
      if (announce) {
        toast.success(`MCP server running at ${url}`, {
          duration: 12000,
          action: {
            label: "Copy setup command",
            onClick: () => writeText(`claude mcp add --transport http inky ${url}`),
          },
        });
      }
    } catch (e) {
      this.mcpUrl = null;
      toast.error(`Could not start MCP server: ${e}`);
    }
  }

  async toggleMcpServer() {
    if (this.mcpUrl) {
      await invoke("stop_mcp");
      this.mcpUrl = null;
      localStorage.setItem("inky.mcpAutostart", "false");
      toast.success("MCP server stopped");
    } else {
      await this.startMcpServer(true);
    }
  }

  /** Push toggle/radio state into the native menu bar. */
  syncMenuChecks() {
    const checks: [string, boolean][] = [
      ["sync_scroll", this.syncScroll],
      ["focus_mode", this.focusMode],
      ["theme_light", this.theme === "light"],
      ["theme_dark", this.theme === "dark"],
      ["theme_book", this.theme === "book"],
      ...Object.keys(READING_WIDTHS).map(
        (k): [string, boolean] => [`width_${k}`, this.readingWidth === k],
      ),
    ];
    for (const [id, checked] of checks) {
      invoke("set_menu_checked", { id, checked }).catch(() => {});
    }
  }

  applyTheme() {
    document.documentElement.classList.remove(...THEMES);
    document.documentElement.classList.add(this.theme);
    localStorage.setItem("inky.theme", this.theme);
  }

  setTheme(theme: ThemeName) {
    this.theme = theme;
    this.applyTheme();
    this.syncMenuChecks();
  }

  setViewMode(mode: ViewMode) {
    this.viewMode = mode;
    localStorage.setItem("inky.viewMode", mode);
    if (this.focusMode && mode !== "editor") {
      this.focusMode = false;
      this.#preFocus = null;
      this.syncMenuChecks();
    }
  }

  applyReadingPrefs() {
    const root = document.documentElement;
    root.style.setProperty("--prose-scale", String(this.fontScale));
    root.style.setProperty("--prose-width", READING_WIDTHS[this.readingWidth].value);
  }

  adjustFontScale(step: -1 | 1) {
    const i = FONT_SCALES.indexOf(this.fontScale as (typeof FONT_SCALES)[number]);
    const next = FONT_SCALES[Math.min(FONT_SCALES.length - 1, Math.max(0, i + step))];
    this.fontScale = next;
    localStorage.setItem("inky.fontScale", String(next));
    this.applyReadingPrefs();
  }

  setReadingWidth(width: ReadingWidth) {
    this.readingWidth = width;
    localStorage.setItem("inky.readingWidth", width);
    this.applyReadingPrefs();
    this.syncMenuChecks();
  }

  toggleToc() {
    this.tocVisible = !this.tocVisible;
    if (this.tocVisible) this.commentsVisible = false;
    localStorage.setItem("inky.tocVisible", String(this.tocVisible));
  }

  toggleComments() {
    this.commentsVisible = !this.commentsVisible;
    if (this.commentsVisible) this.tocVisible = false;
    else this.commentDraft = null;
  }

  // --- comments ------------------------------------------------------------

  async loadComments() {
    this.commentThreads = [];
    this.activeThreadId = null;
    this.commentDraft = null;
    this.threadOrder = {};
    this.editorThreadPos = {};
    if (!this.currentPath) return;
    try {
      const raw = await invoke<string>("read_comments", { docPath: this.currentPath });
      if (raw) {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed.threads)) this.commentThreads = parsed.threads;
      }
    } catch {
      // Missing or unreadable sidecar: start empty.
    }
  }

  async saveComments() {
    if (!this.currentPath) return;
    const json = this.commentThreads.length
      ? JSON.stringify({ version: 1, threads: this.commentThreads }, null, 2)
      : "";
    try {
      await invoke("write_comments", { docPath: this.currentPath, json });
    } catch (e) {
      toast.error(`Could not save comments: ${e}`);
    }
  }

  startCommentDraft(draft: CommentDraft) {
    this.commentDraft = draft;
    this.commentsVisible = true;
    this.tocVisible = false;
    this.activeThreadId = null;
  }

  addThread(text: string) {
    if (!this.commentDraft || !text.trim()) return;
    const now = new Date().toISOString();
    const thread: CommentThread = {
      id: newId("thread"),
      quote: this.commentDraft.quote,
      prefix: this.commentDraft.prefix,
      suffix: this.commentDraft.suffix,
      resolved: false,
      createdAt: now,
      comments: [{ id: newId("msg"), text: text.trim(), createdAt: now }],
    };
    this.commentThreads = [...this.commentThreads, thread];
    this.commentDraft = null;
    this.activeThreadId = thread.id;
    this.commentFilter = this.commentFilter === "resolved" ? "open" : this.commentFilter;
    this.saveComments();
  }

  replyToThread(threadId: string, text: string) {
    if (!text.trim()) return;
    this.commentThreads = this.commentThreads.map((t) =>
      t.id === threadId
        ? {
            ...t,
            comments: [
              ...t.comments,
              { id: newId("msg"), text: text.trim(), createdAt: new Date().toISOString() },
            ],
          }
        : t,
    );
    this.saveComments();
  }

  setThreadResolved(threadId: string, resolved: boolean) {
    this.commentThreads = this.commentThreads.map((t) =>
      t.id === threadId ? { ...t, resolved } : t,
    );
    if (this.activeThreadId === threadId && resolved) this.activeThreadId = null;
    this.saveComments();
  }

  deleteThread(threadId: string) {
    this.commentThreads = this.commentThreads.filter((t) => t.id !== threadId);
    if (this.activeThreadId === threadId) this.activeThreadId = null;
    this.saveComments();
  }

  /** Focus a thread from a highlight click (opens the panel). */
  openThread(threadId: string) {
    this.activeThreadId = threadId;
    this.commentsVisible = true;
    this.tocVisible = false;
    const thread = this.commentThreads.find((t) => t.id === threadId);
    if (thread?.resolved && this.commentFilter === "open") this.commentFilter = "all";
  }

  /** Scroll the document to a thread's anchor (from a panel card click). */
  revealThread(threadId: string) {
    this.activeThreadId = threadId;
    const mark = document.querySelector<HTMLElement>(
      `mark.comment-hl[data-thread-id="${CSS.escape(threadId)}"]`,
    );
    if (mark && this.viewMode !== "editor") {
      lockSync(700);
      mark.scrollIntoView({ behavior: "smooth", block: "center" });
      return;
    }
    const pos = this.editorThreadPos[threadId];
    if (this.viewMode !== "preview" && this.editorView && pos) {
      lockSync(700);
      this.editorView.dispatch({
        selection: { anchor: pos.from },
        effects: EditorView.scrollIntoView(pos.from, { y: "center" }),
      });
    }
  }

  toggleSyncScroll() {
    this.syncScroll = !this.syncScroll;
    localStorage.setItem("inky.syncScroll", String(this.syncScroll));
    this.syncMenuChecks();
  }

  openSearch() {
    if (this.currentPath) this.searchOpen = true;
  }

  toggleSidebar() {
    this.sidebarVisible = !this.sidebarVisible;
    localStorage.setItem("inky.sidebar", String(this.sidebarVisible));
  }

  toggleFocusMode() {
    if (!this.focusMode) {
      this.#preFocus = {
        viewMode: this.viewMode,
        sidebar: this.sidebarVisible,
        toc: this.tocVisible,
      };
      this.focusMode = true;
      this.viewMode = "editor";
      this.sidebarVisible = false;
      this.tocVisible = false;
    } else {
      this.focusMode = false;
      if (this.#preFocus) {
        this.viewMode = this.#preFocus.viewMode;
        this.sidebarVisible = this.#preFocus.sidebar;
        this.tocVisible = this.#preFocus.toc;
        this.#preFocus = null;
      }
    }
    this.syncMenuChecks();
  }

  async checkForUpdates(manual: boolean) {
    try {
      const update = await check();
      if (update) {
        toast.info(`Inky ${update.version} is available`, {
          duration: 20000,
          action: {
            label: "Install & Relaunch",
            onClick: async () => {
              try {
                await update.downloadAndInstall();
                await relaunch();
              } catch (e) {
                toast.error(`Update failed: ${e}`);
              }
            },
          },
        });
      } else if (manual) {
        toast.success("You're on the latest version");
      }
    } catch (e) {
      if (manual) toast.error(`Could not check for updates: ${e}`);
    }
  }

  scrollToHeading(entry: TocEntry) {
    this.activeHeadingId = entry.id;
    // Both panes are positioned explicitly here; don't let scroll-sync echo.
    lockSync(700);
    if (this.viewMode !== "editor") {
      document.getElementById(entry.id)?.scrollIntoView({ behavior: "smooth", block: "start" });
    }
    if (this.viewMode !== "preview" && this.editorView) {
      const view = this.editorView;
      const line = Math.min(entry.line, view.state.doc.lines);
      const pos = view.state.doc.line(line).from;
      view.dispatch({
        selection: { anchor: pos },
        effects: EditorView.scrollIntoView(pos, { y: "start", yMargin: 16 }),
      });
    }
  }

  async movePath(path: string, targetDir: string) {
    try {
      const newPath = await invoke<string>("move_path", { path, targetDir });
      if (this.currentPath === path) {
        this.currentPath = newPath;
        localStorage.setItem("inky.lastDoc", newPath);
      } else if (this.currentPath?.startsWith(path + "/")) {
        this.currentPath = newPath + this.currentPath.slice(path.length);
        localStorage.setItem("inky.lastDoc", this.currentPath);
      }
      await this.refreshTree();
    } catch (e) {
      toast.error(`Move failed: ${e}`);
    }
  }

  async refreshTree() {
    try {
      this.tree = await invoke<TreeNode[]>("list_tree");
    } catch (e) {
      toast.error(`Could not read library: ${e}`);
    }
  }

  async syncFromDisk() {
    await this.refreshTree();
    if (this.currentPath && !this.dirty) {
      try {
        const disk = await invoke<string>("read_doc", { path: this.currentPath });
        if (disk !== this.savedContent) {
          this.savedContent = disk;
          this.content = disk;
        }
        this.#docMtime = await invoke<number>("doc_mtime", { path: this.currentPath }).catch(
          () => null,
        );
      } catch {
        // File disappeared from disk; keep the buffer so the user can re-save.
      }
    }
    // Pick up comment threads written by agents via the MCP server.
    await this.reloadCommentsFromDisk();
    // Reflect the hosted MCP server dying (e.g. node killed externally).
    if (this.mcpUrl && !(await invoke<boolean>("mcp_status").catch(() => false))) {
      this.mcpUrl = null;
    }
  }

  /** Refresh threads from the sidecar without touching draft/selection state. */
  async reloadCommentsFromDisk() {
    if (!this.currentPath) return;
    try {
      const raw = await invoke<string>("read_comments", { docPath: this.currentPath });
      const threads: CommentThread[] = raw ? (JSON.parse(raw).threads ?? []) : [];
      if (JSON.stringify(threads) === JSON.stringify(this.commentThreads)) return;
      // Surface externally-added messages (e.g. an agent replying via MCP).
      const known = new Set(this.commentThreads.flatMap((t) => t.comments.map((m) => m.id)));
      const fresh = threads.flatMap((t) => t.comments).filter((m) => !known.has(m.id));
      this.commentThreads = threads;
      if (fresh.length > 0) {
        const authors = [...new Set(fresh.map((m) => m.author).filter(Boolean))];
        const who = authors.length ? ` from ${authors.join(", ")}` : "";
        toast.info(`${fresh.length} new comment${fresh.length === 1 ? "" : "s"}${who}`, {
          action: {
            label: "Show",
            onClick: () => {
              this.commentsVisible = true;
              this.tocVisible = false;
            },
          },
        });
      }
    } catch {
      // Unreadable sidecar; keep current state.
    }
  }

  async openDoc(path: string, opts: { silent?: boolean } = {}) {
    if (this.dirty) await this.save();
    try {
      const text = await invoke<string>("read_doc", { path });
      this.currentPath = path;
      this.content = text;
      this.savedContent = text;
      this.#docMtime = await invoke<number>("doc_mtime", { path }).catch(() => null);
      localStorage.setItem("inky.lastDoc", path);
      getCurrentWindow().setTitle(`${this.docName.replace(/\.(md|markdown|mmd)$/i, "")} — Inky`);
      await this.loadComments();
    } catch (e) {
      if (!opts.silent) toast.error(`Could not open document: ${e}`);
    }
  }

  closeDoc() {
    this.currentPath = null;
    this.content = "";
    this.savedContent = "";
    this.commentThreads = [];
    this.commentDraft = null;
    this.activeThreadId = null;
    localStorage.removeItem("inky.lastDoc");
    getCurrentWindow().setTitle("Inky");
  }

  async save() {
    if (!this.currentPath) return;
    if (this.#autosaveTimer) {
      clearTimeout(this.#autosaveTimer);
      this.#autosaveTimer = null;
    }
    const path = this.currentPath;
    const text = this.content;
    try {
      // Warn if the file changed on disk while we were editing (an agent via
      // MCP, another machine, …). write_doc snapshots the disk version first,
      // so nothing is lost — but the user should know.
      if (this.#docMtime !== null) {
        const current = await invoke<number>("doc_mtime", { path }).catch(() => null);
        if (current !== null && current !== this.#docMtime) {
          toast.warning(
            "This document changed on disk while you were editing. Your version was saved; the other one was kept in .inky-history.",
          );
        }
      }
      await invoke("write_doc", { path, content: text });
      this.savedContent = text;
      this.#docMtime = await invoke<number>("doc_mtime", { path }).catch(() => null);
    } catch (e) {
      toast.error(`Save failed: ${e}`);
    }
  }

  scheduleAutosave() {
    if (this.#autosaveTimer) clearTimeout(this.#autosaveTimer);
    this.#autosaveTimer = setTimeout(() => this.save(), 1200);
  }

  async newDoc(dir?: string, kind: "markdown" | "mermaid" = "markdown") {
    const isMermaid = kind === "mermaid";
    try {
      const path = await invoke<string>("create_doc", {
        dir: dir ?? this.libraryRoot,
        name: isMermaid ? "Untitled diagram" : "Untitled",
        ext: isMermaid ? "mmd" : "md",
        content: isMermaid ? MERMAID_TEMPLATE : "# Untitled\n\n",
      });
      await this.refreshTree();
      await this.openDoc(path);
      return path;
    } catch (e) {
      toast.error(`Could not create document: ${e}`);
    }
  }

  async newFolder(dir: string | undefined, name: string) {
    try {
      await invoke("create_folder", { dir: dir ?? this.libraryRoot, name });
      await this.refreshTree();
    } catch (e) {
      toast.error(`Could not create folder: ${e}`);
    }
  }

  async renamePath(path: string, newName: string) {
    try {
      const newPath = await invoke<string>("rename_path", { path, newName });
      if (this.currentPath === path) {
        this.currentPath = newPath;
        localStorage.setItem("inky.lastDoc", newPath);
        getCurrentWindow().setTitle(
          `${this.docName.replace(/\.(md|markdown|mmd)$/i, "")} — Inky`,
        );
      } else if (this.currentPath?.startsWith(path + "/")) {
        this.currentPath = newPath + this.currentPath.slice(path.length);
        localStorage.setItem("inky.lastDoc", this.currentPath);
      }
      await this.refreshTree();
    } catch (e) {
      toast.error(`Rename failed: ${e}`);
    }
  }

  /** Rename the currently open document (used by the editable title bar). */
  async renameCurrentDoc(newStem: string) {
    if (!this.currentPath) return;
    const stem = newStem.trim();
    if (!stem) return;
    const ext = this.docName.match(/\.(md|markdown|mmd)$/i)?.[0] ?? ".md";
    if (stem + ext === this.docName) return;
    await this.renamePath(this.currentPath, stem + ext);
  }

  async deletePath(path: string) {
    try {
      await invoke("delete_path", { path });
      if (this.currentPath === path || this.currentPath?.startsWith(path + "/")) {
        this.closeDoc();
      }
      await this.refreshTree();
      toast.success("Moved to Trash");
    } catch (e) {
      toast.error(`Delete failed: ${e}`);
    }
  }

  async pasteAsNewDoc() {
    let text: string;
    try {
      text = await readText();
    } catch {
      toast.error("Clipboard is empty or does not contain text");
      return;
    }
    if (!text?.trim()) {
      toast.error("Clipboard is empty");
      return;
    }
    const trimmed = text.trimStart();
    const isMermaid =
      /^(graph|flowchart|sequenceDiagram|classDiagram|stateDiagram|erDiagram|gantt|pie|journey|mindmap|timeline|gitGraph|quadrantChart|xychart)/.test(
        trimmed,
      );
    const heading = trimmed.match(/^#{1,6}\s+(.+)$/m)?.[1]?.slice(0, 60);
    try {
      const path = await invoke<string>("create_doc", {
        dir: this.libraryRoot,
        name: heading ?? (isMermaid ? "Pasted diagram" : "Pasted note"),
        ext: isMermaid ? "mmd" : "md",
        content: text,
      });
      await this.refreshTree();
      await this.openDoc(path);
      toast.success("Clipboard pasted as new document");
    } catch (e) {
      toast.error(`Paste failed: ${e}`);
    }
  }

  async copyMarkdown() {
    await writeText(this.content);
    toast.success("Markdown copied");
  }

  async copyHtml(html: string) {
    await writeText(html);
    toast.success("HTML copied");
  }

  /** Ensure the preview is the only thing on the page before printing. */
  async #enterPrintLayout() {
    await this.save();
    if (this.viewMode !== "preview") {
      this.setViewMode("preview");
      // Let the preview lay out (and mermaid render) before printing.
      await new Promise((r) => setTimeout(r, 400));
    }
  }

  async exportPdf() {
    if (!this.currentPath) return;
    const defaultName = this.docName.replace(/\.(md|markdown|mmd)$/i, "") + ".pdf";
    const savePath = await save({
      defaultPath: defaultName,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!savePath) return;
    await this.#enterPrintLayout();
    try {
      await invoke("print_document", { savePath });
    } catch (e) {
      toast.error(`PDF export failed: ${e}`);
      return;
    }
    // The print operation finishes asynchronously; confirm once the file lands.
    for (let i = 0; i < 20; i++) {
      await new Promise((r) => setTimeout(r, 250));
      if (await invoke<boolean>("path_exists", { path: savePath })) {
        toast.success(`PDF saved: ${savePath.split("/").pop()}`);
        const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
        revealItemInDir(savePath);
        return;
      }
    }
    toast.error("PDF export did not complete");
  }

  async printDoc() {
    if (!this.currentPath) return;
    await this.#enterPrintLayout();
    try {
      await invoke("print_document", { savePath: null });
    } catch (e) {
      toast.error(`Print failed: ${e}`);
    }
  }

  async chooseLibrary() {
    const dir = await open({ directory: true, defaultPath: this.libraryRoot });
    if (typeof dir !== "string") return;
    try {
      this.libraryRoot = await invoke<string>("set_library_root", { path: dir });
      this.closeDoc();
      await this.refreshTree();
      toast.success(`Library: ${dir}`);
    } catch (e) {
      toast.error(`Could not switch library: ${e}`);
    }
  }
}

export const app = new AppState();
