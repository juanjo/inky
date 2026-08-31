// @vitest-environment node
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { spawn, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

let proc: ChildProcess;
let lib: string;
let nextId = 1;
const pending = new Map<number, (msg: unknown) => void>();

function rpc(method: string, params?: unknown): Promise<any> {
  const id = nextId++;
  const p = new Promise((resolve) => pending.set(id, resolve));
  proc.stdin!.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  return p as Promise<any>;
}

function notify(method: string) {
  proc.stdin!.write(JSON.stringify({ jsonrpc: "2.0", method }) + "\n");
}

const callTool = (name: string, args: unknown) =>
  rpc("tools/call", { name, arguments: args });

beforeAll(async () => {
  lib = fs.mkdtempSync(path.join(os.tmpdir(), "inky-mcp-test-"));
  proc = spawn("node", [path.join(import.meta.dirname, "..", "mcp", "server.mjs")], {
    env: { ...process.env, INKY_LIBRARY: lib },
    stdio: ["pipe", "pipe", "ignore"],
  });
  let buffer = "";
  proc.stdout!.on("data", (chunk: Buffer) => {
    buffer += chunk;
    let idx;
    while ((idx = buffer.indexOf("\n")) !== -1) {
      const line = buffer.slice(0, idx);
      buffer = buffer.slice(idx + 1);
      if (!line.trim()) continue;
      const msg = JSON.parse(line);
      pending.get(msg.id)?.(msg);
      pending.delete(msg.id);
    }
  });
  await rpc("initialize", {
    protocolVersion: "2025-06-18",
    capabilities: {},
    clientInfo: { name: "vitest", version: "0" },
  });
  notify("notifications/initialized");
});

afterAll(() => {
  proc?.kill();
  fs.rmSync(lib, { recursive: true, force: true });
});

describe("inky MCP server", () => {
  it("writes and reads documents", async () => {
    const w = await callTool("write_document", { path: "a/b.md", content: "# Hi\n" });
    expect(w.result.content[0].text).toContain("Saved");
    const r = await callTool("read_document", { path: "a/b.md" });
    expect(r.result.content[0].text).toBe("# Hi\n");
  });

  it("rejects paths escaping the library", async () => {
    const res = await callTool("read_document", { path: "../outside.md" });
    expect(res.result?.isError ?? !!res.error).toBe(true);
  });

  it("rejects non-document extensions", async () => {
    const res = await callTool("write_document", { path: "evil.sh", content: "x" });
    expect(res.result?.isError ?? !!res.error).toBe(true);
  });

  it("patches with exact-match safety", async () => {
    await callTool("write_document", { path: "p.md", content: "alpha beta alpha\n" });
    const ambiguous = await callTool("patch_document", {
      path: "p.md",
      old_text: "alpha",
      new_text: "x",
    });
    expect(ambiguous.result?.isError ?? !!ambiguous.error).toBe(true);
    const ok = await callTool("patch_document", {
      path: "p.md",
      old_text: "beta",
      new_text: "BETA",
    });
    expect(ok.result.content[0].text).toContain("Patched");
    const r = await callTool("read_document", { path: "p.md" });
    expect(r.result.content[0].text).toBe("alpha BETA alpha\n");
  });

  it("keeps and serves version history", async () => {
    await callTool("write_document", { path: "h.md", content: "first version\n" });
    await callTool("write_document", { path: "h.md", content: "second version\n" });
    const list = await callTool("list_versions", { path: "h.md" });
    const name = list.result.content[0].text.match(/h\.[\w.-]+\.md/)?.[0];
    expect(name).toBeTruthy();
    const old = await callTool("read_version", { path: "h.md", version: name });
    expect(old.result.content[0].text).toBe("first version\n");
    const bad = await callTool("read_version", { path: "h.md", version: "../h.md" });
    expect(bad.result?.isError ?? !!bad.error).toBe(true);
  });

  it("round-trips comments", async () => {
    await callTool("write_document", { path: "c.md", content: "hello brave world\n" });
    const created = await callTool("create_comment", {
      path: "c.md",
      quote: "brave",
      text: "why brave?",
    });
    const threadId = created.result.content[0].text.match(/thread-[\w-]+/)?.[0];
    expect(threadId).toBeTruthy();
    await callTool("reply_to_comment", { path: "c.md", thread_id: threadId, text: "because" });
    await callTool("resolve_comment", { path: "c.md", thread_id: threadId });
    const list = await callTool("list_comments", { path: "c.md", filter: "resolved" });
    expect(list.result.content[0].text).toContain("why brave?");
    expect(list.result.content[0].text).toContain("because");
  });
});
