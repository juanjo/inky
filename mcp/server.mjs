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
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import fs from "node:fs/promises";
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
    await fs.writeFile(resolved, content, "utf8");
    return text(`Saved ${relPath} (${content.length} chars)`);
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

const transport = new StdioServerTransport();
await server.connect(transport);
