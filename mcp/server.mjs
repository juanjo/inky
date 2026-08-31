#!/usr/bin/env node
/**
 * Inky MCP server — lets agents read and write documents in the Inky library.
 *
 * The library folder is resolved from (in order):
 *   1. the INKY_LIBRARY environment variable
 *   2. the Inky app's config file (shared with the desktop app)
 *   3. ~/Documents/Inky
 *
 * Register with Claude Code:
 *   claude mcp add inky -- node /path/to/inky/mcp/server.mjs
 */
import { McpServer, ResourceTemplate } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import fs from "node:fs/promises";
import fsSync from "node:fs";
import os from "node:os";
import path from "node:path";

const DOC_EXTENSIONS = [".md", ".markdown", ".mmd"];

function configFile() {
  const home = os.homedir();
  switch (process.platform) {
    case "darwin":
      return path.join(home, "Library", "Application Support", "com.inky.app", "config.json");
    case "win32":
      return path.join(process.env.APPDATA ?? path.join(home, "AppData", "Roaming"), "com.inky.app", "config.json");
    default:
      return path.join(process.env.XDG_CONFIG_HOME ?? path.join(home, ".config"), "com.inky.app", "config.json");
  }
}

async function libraryRoot() {
  if (process.env.INKY_LIBRARY) return process.env.INKY_LIBRARY;
  try {
    const config = JSON.parse(await fs.readFile(configFile(), "utf8"));
    if (config.library) return config.library;
  } catch {
    // No config yet — the app hasn't run. Fall through to the default.
  }
  return path.join(os.homedir(), "Documents", "Inky");
}

/** Resolve a library-relative path and refuse anything that escapes the root. */
async function resolveInLibrary(relPath) {
  const root = await libraryRoot();
  const resolved = path.resolve(root, relPath ?? ".");
  if (resolved !== root && !resolved.startsWith(root + path.sep)) {
    throw new Error(`Path escapes the Inky library: ${relPath}`);
  }
  return { root, resolved };
}

function isDoc(name) {
  return DOC_EXTENSIONS.includes(path.extname(name).toLowerCase());
}

async function walk(dir, root, out) {
  let entries;
  try {
    entries = await fs.readdir(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    if (entry.name.startsWith(".")) continue;
    const full = path.join(dir, entry.name);
    const rel = path.relative(root, full);
    if (entry.isDirectory()) {
      out.push({ path: rel + "/", type: "folder" });
      await walk(full, root, out);
    } else if (isDoc(entry.name)) {
      out.push({ path: rel, type: entry.name.toLowerCase().endsWith(".mmd") ? "mermaid" : "markdown" });
    }
  }
  return out;
}

function text(value) {
  return { content: [{ type: "text", text: value }] };
}

// --- version snapshots (same policy as the app) ----------------------------

const HISTORY_DIR = ".inky-history";
const SNAPSHOT_MIN_INTERVAL_MS = 10 * 60 * 1000;
const SNAPSHOT_KEEP = 20;

/** Before overwriting a document, keep the old version (rate-limited). */
async function snapshot(absPath) {
  let old;
  try {
    old = await fs.readFile(absPath, "utf8");
  } catch {
    return; // new file, nothing to snapshot
  }
  const dir = path.join(path.dirname(absPath), HISTORY_DIR);
  const ext = path.extname(absPath);
  const stem = path.basename(absPath, ext);
  await fs.mkdir(dir, { recursive: true });
  const mine = (await fs.readdir(dir)).filter((f) => f.startsWith(stem + ".") && f.endsWith(ext)).sort();
  const last = mine.at(-1);
  if (last) {
    const st = await fs.stat(path.join(dir, last));
    if (Date.now() - st.mtimeMs < SNAPSHOT_MIN_INTERVAL_MS) return;
  }
  const ts = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
  await fs.writeFile(path.join(dir, `${stem}.${ts}${ext}`), old, "utf8");
  for (const f of mine.slice(0, Math.max(0, mine.length - (SNAPSHOT_KEEP - 1)))) {
    await fs.rm(path.join(dir, f), { force: true });
  }
}

// --- comment sidecars (same format the app uses) ---------------------------

function sidecarFor(absPath) {
  return path.join(path.dirname(absPath), `.${path.basename(absPath)}.comments.json`);
}

async function readThreads(absPath) {
  try {
    const parsed = JSON.parse(await fs.readFile(sidecarFor(absPath), "utf8"));
    return Array.isArray(parsed.threads) ? parsed.threads : [];
  } catch {
    return [];
  }
}

async function writeThreads(absPath, threads) {
  const sc = sidecarFor(absPath);
  if (threads.length === 0) {
    await fs.rm(sc, { force: true });
    return;
  }
  await fs.writeFile(sc, JSON.stringify({ version: 1, threads }, null, 2), "utf8");
}

function newId(prefix) {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

function renderThread(t) {
  const status = t.resolved ? "resolved" : "open";
  const quote = t.quote.length > 120 ? t.quote.slice(0, 120) + "…" : t.quote;
  const msgs = t.comments
    .map((m) => `    [${m.author ?? "User"}] ${m.text.replaceAll("\n", "\n    ")}`)
    .join("\n");
  return `- ${t.id} (${status})\n  quote: "${quote}"\n${msgs}`;
}

export function createServer() {
  const server = new McpServer({ name: "inky", version: "0.1.0" });

server.registerTool(
  "list_documents",
  {
    title: "List Inky documents",
    description:
      "List every document and folder in the user's Inky library. Paths are relative to the library root. Documents are markdown (.md) or standalone mermaid diagrams (.mmd).",
    inputSchema: {},
  },
  async () => {
    const root = await libraryRoot();
    const items = await walk(root, root, []);
    const listing = items.map((i) => `${i.type === "folder" ? "📁" : i.type === "mermaid" ? "🧜" : "📄"} ${i.path}`).join("\n");
    return text(`Library root: ${root}\n\n${listing || "(library is empty)"}`);
  },
);

server.registerTool(
  "read_document",
  {
    title: "Read an Inky document",
    description: "Read the raw markdown (or mermaid) source of a document in the Inky library.",
    inputSchema: {
      path: z.string().describe("Library-relative path, e.g. 'Notes/ideas.md'"),
    },
  },
  async ({ path: relPath }) => {
    const { resolved } = await resolveInLibrary(relPath);
    return text(await fs.readFile(resolved, "utf8"));
  },
);

server.registerTool(
  "write_document",
  {
    title: "Create or update an Inky document",
    description:
      "Write a markdown document into the Inky library. Creates the document (and any missing folders) if it does not exist, otherwise overwrites it. Use a .md extension for markdown and .mmd for standalone mermaid diagrams. Mermaid code fences inside .md files render as diagrams in Inky.",
    inputSchema: {
      path: z.string().describe("Library-relative path, e.g. 'Meetings/2026-08-31 standup.md'"),
      content: z.string().describe("Full document content (markdown or mermaid source)"),
    },
  },
  async ({ path: relPath, content }) => {
    if (!isDoc(relPath)) {
      throw new Error(`Unsupported extension — use one of: ${DOC_EXTENSIONS.join(", ")}`);
    }
    const { resolved } = await resolveInLibrary(relPath);
    await fs.mkdir(path.dirname(resolved), { recursive: true });
    await snapshot(resolved);
    await fs.writeFile(resolved, content, "utf8");
    return text(`Saved ${relPath} (${content.length} chars)`);
  },
);

server.registerTool(
  "patch_document",
  {
    title: "Edit part of an Inky document",
    description:
      "Replace an exact text snippet inside a document. Prefer this over write_document for edits — it fails safely if the document changed since you read it. old_text must appear exactly once (include surrounding context to disambiguate).",
    inputSchema: {
      path: z.string().describe("Library-relative document path"),
      old_text: z.string().describe("Exact text to replace (must occur exactly once)"),
      new_text: z.string().describe("Replacement text"),
    },
  },
  async ({ path: relPath, old_text, new_text }) => {
    const { resolved } = await resolveInLibrary(relPath);
    const content = await fs.readFile(resolved, "utf8");
    const count = content.split(old_text).length - 1;
    if (count === 0) {
      throw new Error("old_text not found — the document may have changed; re-read it first.");
    }
    if (count > 1) {
      throw new Error(`old_text occurs ${count} times — include more surrounding context.`);
    }
    await snapshot(resolved);
    await fs.writeFile(resolved, content.replace(old_text, new_text), "utf8");
    return text(`Patched ${relPath}`);
  },
);

server.registerTool(
  "rename_document",
  {
    title: "Rename an Inky document",
    description:
      "Rename a document in place (comments follow the document). new_name keeps the original extension if none is given.",
    inputSchema: {
      path: z.string().describe("Library-relative document path"),
      new_name: z.string().describe("New file name, e.g. 'Better title.md'"),
    },
  },
  async ({ path: relPath, new_name }) => {
    const { resolved } = await resolveInLibrary(relPath);
    let name = new_name.trim();
    if (!isDoc(name)) name += path.extname(resolved);
    if (name.includes("/")) throw new Error("new_name must not contain '/'");
    const target = path.join(path.dirname(resolved), name);
    if (fsSync.existsSync(target)) throw new Error(`${name} already exists`);
    await fs.rename(resolved, target);
    await fs.rename(sidecarFor(resolved), sidecarFor(target)).catch(() => {});
    const rel = path.relative(await libraryRoot(), target);
    return text(`Renamed to ${rel}`);
  },
);

server.registerTool(
  "move_document",
  {
    title: "Move an Inky document",
    description:
      "Move a document into another folder of the library (folders are created if missing; comments follow the document).",
    inputSchema: {
      path: z.string().describe("Library-relative document path"),
      target_folder: z.string().describe("Library-relative destination folder ('' for the root)"),
    },
  },
  async ({ path: relPath, target_folder }) => {
    const { resolved } = await resolveInLibrary(relPath);
    const { resolved: destDir } = await resolveInLibrary(target_folder || ".");
    await fs.mkdir(destDir, { recursive: true });
    const target = path.join(destDir, path.basename(resolved));
    if (fsSync.existsSync(target)) throw new Error(`${path.basename(resolved)} already exists there`);
    await fs.rename(resolved, target);
    await fs.rename(sidecarFor(resolved), sidecarFor(target)).catch(() => {});
    const rel = path.relative(await libraryRoot(), target);
    return text(`Moved to ${rel}`);
  },
);

server.registerTool(
  "create_folder",
  {
    title: "Create a folder in the Inky library",
    description: "Create a folder (and any missing parents) inside the Inky library.",
    inputSchema: {
      path: z.string().describe("Library-relative folder path, e.g. 'Projects/Inky'"),
    },
  },
  async ({ path: relPath }) => {
    const { resolved } = await resolveInLibrary(relPath);
    await fs.mkdir(resolved, { recursive: true });
    return text(`Created folder ${relPath}`);
  },
);

server.registerTool(
  "delete_document",
  {
    title: "Delete an Inky document",
    description: "Delete a single document from the Inky library. Folders cannot be deleted.",
    inputSchema: {
      path: z.string().describe("Library-relative path of the document to delete"),
    },
  },
  async ({ path: relPath }) => {
    const { resolved } = await resolveInLibrary(relPath);
    const stat = await fs.stat(resolved);
    if (!stat.isFile() || !isDoc(resolved)) {
      throw new Error("Only documents can be deleted");
    }
    await fs.unlink(resolved);
    return text(`Deleted ${relPath}`);
  },
);

server.registerTool(
  "search_documents",
  {
    title: "Search Inky documents",
    description: "Case-insensitive full-text search across every document in the Inky library. Returns matching lines with their document path and line number.",
    inputSchema: {
      query: z.string().describe("Text to search for"),
    },
  },
  async ({ query }) => {
    const root = await libraryRoot();
    const items = await walk(root, root, []);
    const needle = query.toLowerCase();
    const hits = [];
    for (const item of items) {
      if (item.type === "folder") continue;
      const body = await fs.readFile(path.join(root, item.path), "utf8");
      body.split("\n").forEach((line, i) => {
        if (line.toLowerCase().includes(needle)) {
          hits.push(`${item.path}:${i + 1}: ${line.trim().slice(0, 200)}`);
        }
      });
      if (hits.length > 200) break;
    }
    return text(hits.length ? hits.slice(0, 200).join("\n") : `No matches for "${query}"`);
  },
);

// Expose every document as an MCP resource so clients can @-mention them.
server.registerResource(
  "document",
  new ResourceTemplate("inky://doc/{+path}", {
    list: async () => {
      const root = await libraryRoot();
      const items = await walk(root, root, []);
      return {
        resources: items
          .filter((i) => i.type !== "folder")
          .map((i) => ({
            uri: `inky://doc/${i.path}`,
            name: i.path,
            mimeType: "text/markdown",
          })),
      };
    },
  }),
  {
    title: "Inky documents",
    description: "Markdown documents in the user's Inky library",
    mimeType: "text/markdown",
  },
  async (uri, { path: relPath }) => {
    const { resolved } = await resolveInLibrary(String(relPath));
    return {
      contents: [
        { uri: uri.href, mimeType: "text/markdown", text: await fs.readFile(resolved, "utf8") },
      ],
    };
  },
);

server.registerTool(
  "list_comments",
  {
    title: "List a document's comment threads",
    description:
      "List the comment threads on an Inky document (the user's questions and notes, Google-Docs style). Each thread has an id, a quoted text anchor, open/resolved status, and messages. Threads marked open usually need an answer.",
    inputSchema: {
      path: z.string().describe("Library-relative document path"),
      filter: z.enum(["all", "open", "resolved"]).optional().describe("Default: all"),
    },
  },
  async ({ path: relPath, filter }) => {
    const { resolved } = await resolveInLibrary(relPath);
    let threads = await readThreads(resolved);
    if (filter === "open") threads = threads.filter((t) => !t.resolved);
    if (filter === "resolved") threads = threads.filter((t) => t.resolved);
    if (threads.length === 0) return text(`No ${filter ?? ""} comments on ${relPath}`.replace("  ", " "));
    return text(threads.map(renderThread).join("\n\n"));
  },
);

server.registerTool(
  "create_comment",
  {
    title: "Comment on a document",
    description:
      "Start a new comment thread on an Inky document, anchored to an exact quote from the document's text. The quote must appear verbatim in the document. Use this to leave feedback, questions, or suggestions the user will see highlighted in Inky.",
    inputSchema: {
      path: z.string().describe("Library-relative document path"),
      quote: z.string().describe("Exact text from the document to anchor the comment to"),
      text: z.string().describe("The comment"),
      author: z.string().optional().describe("Author label shown in Inky (default: Claude)"),
    },
  },
  async ({ path: relPath, quote, text: body, author }) => {
    const { resolved } = await resolveInLibrary(relPath);
    const doc = await fs.readFile(resolved, "utf8");
    const idx = doc.indexOf(quote);
    if (idx === -1) {
      throw new Error("Quote not found in the document — it must match the text exactly.");
    }
    const threads = await readThreads(resolved);
    const now = new Date().toISOString();
    const thread = {
      id: newId("thread"),
      quote,
      prefix: doc.slice(Math.max(0, idx - 30), idx),
      suffix: doc.slice(idx + quote.length, idx + quote.length + 30),
      resolved: false,
      createdAt: now,
      comments: [{ id: newId("msg"), text: body, createdAt: now, author: author ?? "Claude" }],
    };
    threads.push(thread);
    await writeThreads(resolved, threads);
    return text(`Created ${thread.id} on ${relPath}`);
  },
);

server.registerTool(
  "reply_to_comment",
  {
    title: "Reply to a comment thread",
    description:
      "Add a reply to an existing comment thread on an Inky document. Use list_comments first to get thread ids. The user sees replies in Inky's comments panel.",
    inputSchema: {
      path: z.string().describe("Library-relative document path"),
      thread_id: z.string().describe("Thread id from list_comments"),
      text: z.string().describe("The reply"),
      author: z.string().optional().describe("Author label shown in Inky (default: Claude)"),
    },
  },
  async ({ path: relPath, thread_id, text: body, author }) => {
    const { resolved } = await resolveInLibrary(relPath);
    const threads = await readThreads(resolved);
    const thread = threads.find((t) => t.id === thread_id);
    if (!thread) throw new Error(`No thread ${thread_id} on ${relPath}`);
    thread.comments.push({
      id: newId("msg"),
      text: body,
      createdAt: new Date().toISOString(),
      author: author ?? "Claude",
    });
    await writeThreads(resolved, threads);
    return text(`Replied to ${thread_id}`);
  },
);

server.registerTool(
  "resolve_comment",
  {
    title: "Resolve or reopen a comment thread",
    description:
      "Mark a comment thread on an Inky document as resolved (or reopen it). Only resolve a thread after actually addressing it — e.g. after replying or updating the document.",
    inputSchema: {
      path: z.string().describe("Library-relative document path"),
      thread_id: z.string().describe("Thread id from list_comments"),
      resolved: z.boolean().optional().describe("Default true; false reopens"),
    },
  },
  async ({ path: relPath, thread_id, resolved: flag }) => {
    const { resolved: abs } = await resolveInLibrary(relPath);
    const threads = await readThreads(abs);
    const thread = threads.find((t) => t.id === thread_id);
    if (!thread) throw new Error(`No thread ${thread_id} on ${relPath}`);
    thread.resolved = flag ?? true;
    await writeThreads(abs, threads);
    return text(`${thread.resolved ? "Resolved" : "Reopened"} ${thread_id}`);
  },
);

  return server;
}

const TOOL_SUMMARY = `list_documents, read_document, write_document, patch_document,
           rename_document, move_document, create_folder, delete_document,
           search_documents, list_comments, create_comment, reply_to_comment,
           resolve_comment`;

/** Streamable-HTTP mode (stateless): used when the Inky app hosts the server. */
async function serveHttp(port) {
  const { StreamableHTTPServerTransport } = await import(
    "@modelcontextprotocol/sdk/server/streamableHttp.js"
  );
  const http = await import("node:http");
  const httpServer = http.createServer(async (req, res) => {
    if (!req.url?.startsWith("/mcp")) {
      res.writeHead(404).end();
      return;
    }
    if (req.method !== "POST") {
      res
        .writeHead(405, { Allow: "POST", "Content-Type": "application/json" })
        .end(JSON.stringify({ jsonrpc: "2.0", error: { code: -32000, message: "Method not allowed" }, id: null }));
      return;
    }
    let body = "";
    for await (const chunk of req) body += chunk;
    try {
      const server = createServer();
      const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined });
      res.on("close", () => {
        transport.close();
        server.close();
      });
      await server.connect(transport);
      await transport.handleRequest(req, res, JSON.parse(body));
    } catch (err) {
      if (!res.headersSent) {
        res
          .writeHead(500, { "Content-Type": "application/json" })
          .end(JSON.stringify({ jsonrpc: "2.0", error: { code: -32603, message: String(err) }, id: null }));
      }
    }
  });
  httpServer.listen(port, "127.0.0.1", async () => {
    console.error(`Inky MCP server (HTTP) on http://127.0.0.1:${port}/mcp
  Library: ${await libraryRoot()}
  Tools:   ${TOOL_SUMMARY}

Register with:  claude mcp add --transport http inky http://127.0.0.1:${port}/mcp`);
  });
}

const httpFlag = process.argv.indexOf("--http");
if (httpFlag !== -1) {
  await serveHttp(Number(process.argv[httpFlag + 1]) || 26317);
} else {
  const server = createServer();
  await server.connect(new StdioServerTransport());
  // Status goes to stderr — stdout carries the MCP JSON-RPC protocol.
  console.error(`Inky MCP server running on stdio
  Library: ${await libraryRoot()}
  Tools:   ${TOOL_SUMMARY}

This process is meant to be launched by an MCP client (it waits for JSON-RPC
on stdin — that's why nothing else appears here). Register it with:

  claude mcp add --scope user inky -- node ${path.join(import.meta.dirname, "server.mjs")}

Press Ctrl+C to stop.`);
}
