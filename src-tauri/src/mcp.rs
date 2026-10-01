//! MCP server exposing the Inky library to agents. Tool names, parameters and
//! messages match the previous JS server so existing agent prompts keep working.

use crate::library::{self, CommentMsg, CommentThread, EntryKind, Library, CONTEXT_CHARS};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use std::future::Future;
use std::net::SocketAddr;
use tokio_util::sync::CancellationToken;

pub const TOOL_SUMMARY: &str = "list_documents, read_document, write_document, patch_document, \
rename_document, move_document, create_folder, delete_document, search_documents, \
list_versions, read_version, list_comments, create_comment, reply_to_comment, resolve_comment, \
open_document";

const INSTRUCTIONS: &str = "Inky is the user's markdown library. Paths are relative to the library root. \
Prefer patch_document over write_document for edits. Use open_document to show the user a document \
(e.g. one you just wrote) in the Inky app. Comment threads marked open usually need an answer; \
resolve a thread only after addressing it.";

const DOC_URI_PREFIX: &str = "inky://doc/";

/// Shows a document in the Inky app (swapped out in tests).
pub type Opener = std::sync::Arc<dyn Fn(&std::path::Path) -> Result<(), String> + Send + Sync>;

/// Hand the file to Inky through macOS `open`, exactly like a Finder
/// double-click — works from the in-app server and from `Inky --mcp` alike.
fn open_in_app(path: &std::path::Path) -> Result<(), String> {
    let status = std::process::Command::new("open")
        .args(["-b", "com.inky.app"])
        .arg(path)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("Could not open the document in Inky — is Inky installed?".into())
    }
}

#[derive(Clone)]
pub struct InkyMcp {
    lib: Library,
    opener: Opener,
    tool_router: ToolRouter<Self>,
}

fn ok(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text.into())])
}

fn reply(result: Result<String, String>) -> CallToolResult {
    match result {
        Ok(text) => ok(text),
        Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
    }
}

// --- parameter shapes (field docs become the JSON-schema descriptions) ------

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct PathParams {
    /// Library-relative path, e.g. 'Notes/ideas.md'
    pub path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WriteParams {
    /// Library-relative path, e.g. 'Meetings/2026-08-31 standup.md'
    pub path: String,
    /// Full document content (markdown or mermaid source)
    pub content: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct PatchParams {
    /// Library-relative document path
    pub path: String,
    /// Exact text to replace (must occur exactly once)
    pub old_text: String,
    /// Replacement text
    pub new_text: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ReadVersionParams {
    /// Library-relative document path
    pub path: String,
    /// Version file name from list_versions
    pub version: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct RenameParams {
    /// Library-relative document path
    pub path: String,
    /// New file name, e.g. 'Better title.md'
    pub new_name: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct MoveParams {
    /// Library-relative document path
    pub path: String,
    /// Library-relative destination folder ('' for the root)
    pub target_folder: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct FolderParams {
    /// Library-relative folder path, e.g. 'Projects/Inky'
    pub path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    /// Text to search for
    pub query: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommentFilter {
    All,
    Open,
    Resolved,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ListCommentsParams {
    /// Library-relative document path
    pub path: String,
    /// Default: all
    #[serde(default)]
    pub filter: Option<CommentFilter>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct CreateCommentParams {
    /// Library-relative document path
    pub path: String,
    /// Exact text from the document to anchor the comment to
    pub quote: String,
    /// The comment
    pub text: String,
    /// Author label shown in Inky (default: Claude)
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ReplyParams {
    /// Library-relative document path
    pub path: String,
    /// Thread id from list_comments
    pub thread_id: String,
    /// The reply
    pub text: String,
    /// Author label shown in Inky (default: Claude)
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ResolveParams {
    /// Library-relative document path
    pub path: String,
    /// Thread id from list_comments
    pub thread_id: String,
    /// Default true; false reopens
    #[serde(default)]
    pub resolved: Option<bool>,
}

fn render_thread(t: &CommentThread) -> String {
    let status = if t.resolved { "resolved" } else { "open" };
    let quote: String = if t.quote.chars().count() > 120 {
        format!("{}…", t.quote.chars().take(120).collect::<String>())
    } else {
        t.quote.clone()
    };
    let msgs = t
        .comments
        .iter()
        .map(|m| {
            format!(
                "    [{}] {}",
                m.author.as_deref().unwrap_or("User"),
                m.text.replace('\n', "\n    ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("- {} ({status})\n  quote: \"{quote}\"\n{msgs}", t.id)
}

#[tool_router]
impl InkyMcp {
    pub fn new(lib: Library) -> Self {
        Self {
            lib: lib.with_agent_origin(),
            opener: std::sync::Arc::new(open_in_app),
            tool_router: Self::tool_router(),
        }
    }

    pub fn with_opener(mut self, opener: Opener) -> Self {
        self.opener = opener;
        self
    }

    #[tool(
        name = "open_document",
        annotations(title = "Show a document in Inky", read_only_hint = true),
        description = "Open a document from the Inky library in the Inky app so the user sees it (launches Inky if needed). Use it after writing or editing a document the user should look at. The user can go back to what they were reading with ⌘[."
    )]
    async fn open_document(&self, Parameters(p): Parameters<PathParams>) -> CallToolResult {
        let result = self.lib.resolve(&p.path).and_then(|abs| {
            if !abs.is_file() || !library::is_doc(&abs) {
                return Err(format!("Not a document in the Inky library: {}", p.path));
            }
            (self.opener)(&abs)?;
            Ok(format!("Opened {} in Inky", p.path))
        });
        reply(result)
    }

    #[tool(
        name = "list_documents",
        annotations(title = "List Inky documents"),
        description = "List every document and folder in the user's Inky library. Paths are relative to the library root. Documents are markdown (.md) or standalone mermaid diagrams (.mmd)."
    )]
    async fn list_documents(&self) -> CallToolResult {
        let listing = self
            .lib
            .walk()
            .into_iter()
            .map(|e| {
                let icon = match e.kind {
                    EntryKind::Folder => "📁",
                    EntryKind::Mermaid => "🧜",
                    EntryKind::Markdown => "📄",
                };
                format!("{icon} {}", e.rel)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let listing = if listing.is_empty() {
            "(library is empty)".to_string()
        } else {
            listing
        };
        ok(format!("Library root: {}\n\n{listing}", self.lib.root().display()))
    }

    #[tool(
        name = "read_document",
        annotations(title = "Read an Inky document"),
        description = "Read the raw markdown (or mermaid) source of a document in the Inky library."
    )]
    async fn read_document(&self, Parameters(p): Parameters<PathParams>) -> CallToolResult {
        reply(self.lib.read(&p.path))
    }

    #[tool(
        name = "write_document",
        annotations(title = "Create or update an Inky document"),
        description = "Write a markdown document into the Inky library. Creates the document (and any missing folders) if it does not exist, otherwise overwrites it. Use a .md extension for markdown and .mmd for standalone mermaid diagrams. Mermaid code fences inside .md files render as diagrams in Inky."
    )]
    async fn write_document(&self, Parameters(p): Parameters<WriteParams>) -> CallToolResult {
        if !library::is_doc(std::path::Path::new(&p.path)) {
            return reply(Err(format!(
                "Unsupported extension — use one of: {}",
                library::DOC_EXTENSIONS
                    .iter()
                    .map(|e| format!(".{e}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        reply(
            self.lib
                .write(&p.path, &p.content)
                .map(|_| format!("Saved {} ({} chars)", p.path, p.content.chars().count())),
        )
    }

    #[tool(
        name = "patch_document",
        annotations(title = "Edit part of an Inky document"),
        description = "Replace an exact text snippet inside a document. Prefer this over write_document for edits — it fails safely if the document changed since you read it. old_text must appear exactly once (include surrounding context to disambiguate)."
    )]
    async fn patch_document(&self, Parameters(p): Parameters<PatchParams>) -> CallToolResult {
        reply(
            self.lib
                .patch(&p.path, &p.old_text, &p.new_text)
                .map(|_| format!("Patched {}", p.path)),
        )
    }

    #[tool(
        name = "list_versions",
        annotations(title = "List a document's saved versions"),
        description = "List snapshots of a document from its hidden history (kept automatically before content-changing overwrites, at most one per 10 minutes). Use read_version to fetch one — e.g. to report what changed, or to recover lost text."
    )]
    async fn list_versions(&self, Parameters(p): Parameters<PathParams>) -> CallToolResult {
        reply(self.lib.list_versions(&p.path).map(|versions| {
            if versions.is_empty() {
                return format!("No saved versions for {}", p.path);
            }
            versions
                .iter()
                .map(|v| format!("{}  (saved {})", v.name, library::iso_from_unix_millis(v.modified_ms)))
                .collect::<Vec<_>>()
                .join("\n")
        }))
    }

    #[tool(
        name = "read_version",
        annotations(title = "Read a saved version of a document"),
        description = "Read the full content of one snapshot from a document's history. Get version names from list_versions. To restore it, write the content back with write_document."
    )]
    async fn read_version(&self, Parameters(p): Parameters<ReadVersionParams>) -> CallToolResult {
        reply(self.lib.read_version(&p.path, &p.version))
    }

    #[tool(
        name = "rename_document",
        annotations(title = "Rename an Inky document"),
        description = "Rename a document in place (comments follow the document). new_name keeps the original extension if none is given."
    )]
    async fn rename_document(&self, Parameters(p): Parameters<RenameParams>) -> CallToolResult {
        if p.new_name.contains('/') {
            return reply(Err("new_name must not contain '/'".into()));
        }
        reply(
            self.lib
                .rename(&p.path, &p.new_name)
                .map(|t| format!("Renamed to {}", self.lib.relative(&t))),
        )
    }

    #[tool(
        name = "move_document",
        annotations(title = "Move an Inky document"),
        description = "Move a document into another folder of the library (folders are created if missing; comments follow the document)."
    )]
    async fn move_document(&self, Parameters(p): Parameters<MoveParams>) -> CallToolResult {
        let folder = if p.target_folder.is_empty() { "." } else { p.target_folder.as_str() };
        reply(
            self.lib
                .move_into(&p.path, folder)
                .map(|t| format!("Moved to {}", self.lib.relative(&t))),
        )
    }

    #[tool(
        name = "create_folder",
        annotations(title = "Create a folder in the Inky library"),
        description = "Create a folder (and any missing parents) inside the Inky library."
    )]
    async fn create_folder(&self, Parameters(p): Parameters<FolderParams>) -> CallToolResult {
        reply(self.lib.ensure_folder(&p.path).map(|_| format!("Created folder {}", p.path)))
    }

    #[tool(
        name = "delete_document",
        annotations(title = "Delete an Inky document"),
        description = "Delete a single document from the Inky library. Folders cannot be deleted."
    )]
    async fn delete_document(&self, Parameters(p): Parameters<PathParams>) -> CallToolResult {
        let abs = match self.lib.resolve(&p.path) {
            Ok(a) => a,
            Err(e) => return reply(Err(e)),
        };
        if !abs.exists() {
            return reply(Err(format!("{} does not exist", p.path)));
        }
        if !abs.is_file() || !library::is_doc(&abs) {
            return reply(Err("Only documents can be deleted".into()));
        }
        reply(self.lib.delete(&p.path).map(|_| format!("Deleted {}", p.path)))
    }

    #[tool(
        name = "search_documents",
        annotations(title = "Search Inky documents"),
        description = "Case-insensitive full-text search across every document in the Inky library. Returns matching lines with their document path and line number."
    )]
    async fn search_documents(&self, Parameters(p): Parameters<SearchParams>) -> CallToolResult {
        let hits = self.lib.search(&p.query);
        if hits.is_empty() {
            return ok(format!("No matches for \"{}\"", p.query));
        }
        ok(hits
            .iter()
            .map(|h| {
                format!(
                    "{}:{}: {}",
                    self.lib.relative(std::path::Path::new(&h.path)),
                    h.line,
                    h.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    #[tool(
        name = "list_comments",
        annotations(title = "List a document's comment threads"),
        description = "List the comment threads on an Inky document (the user's questions and notes, Google-Docs style). Each thread has an id, a quoted text anchor, open/resolved status, and messages. Threads marked open usually need an answer."
    )]
    async fn list_comments(&self, Parameters(p): Parameters<ListCommentsParams>) -> CallToolResult {
        let filter = p.filter.unwrap_or(CommentFilter::All);
        reply(self.lib.threads(&p.path).map(|threads| {
            let threads: Vec<&CommentThread> = threads
                .iter()
                .filter(|t| match filter {
                    CommentFilter::All => true,
                    CommentFilter::Open => !t.resolved,
                    CommentFilter::Resolved => t.resolved,
                })
                .collect();
            if threads.is_empty() {
                let label = match filter {
                    CommentFilter::All => "",
                    CommentFilter::Open => "open ",
                    CommentFilter::Resolved => "resolved ",
                };
                return format!("No {label}comments on {}", p.path);
            }
            threads.iter().map(|t| render_thread(t)).collect::<Vec<_>>().join("\n\n")
        }))
    }

    #[tool(
        name = "create_comment",
        annotations(title = "Comment on a document"),
        description = "Start a new comment thread on an Inky document, anchored to an exact quote from the document's text. The quote must appear verbatim in the document. Use this to leave feedback, questions, or suggestions the user will see highlighted in Inky."
    )]
    async fn create_comment(&self, Parameters(p): Parameters<CreateCommentParams>) -> CallToolResult {
        reply((|| {
            let doc = self.lib.read(&p.path)?;
            let idx = doc
                .find(&p.quote)
                .ok_or_else(|| "Quote not found in the document — it must match the text exactly.".to_string())?;
            let before = &doc[..idx];
            let after = &doc[idx + p.quote.len()..];
            let prefix: String = before
                .chars()
                .rev()
                .take(CONTEXT_CHARS)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let suffix: String = after.chars().take(CONTEXT_CHARS).collect();
            let now = library::iso_now();
            let thread = CommentThread {
                id: library::new_id("thread"),
                quote: p.quote.clone(),
                prefix,
                suffix,
                resolved: false,
                created_at: now.clone(),
                comments: vec![CommentMsg {
                    id: library::new_id("msg"),
                    text: p.text.clone(),
                    created_at: now,
                    author: Some(p.author.clone().unwrap_or_else(|| "Claude".into())),
                }],
            };
            let mut threads = self.lib.threads(&p.path)?;
            let id = thread.id.clone();
            threads.push(thread);
            self.lib.save_threads(&p.path, &threads)?;
            Ok(format!("Created {id} on {}", p.path))
        })())
    }

    #[tool(
        name = "reply_to_comment",
        annotations(title = "Reply to a comment thread"),
        description = "Add a reply to an existing comment thread on an Inky document. Use list_comments first to get thread ids. The user sees replies in Inky's comments panel."
    )]
    async fn reply_to_comment(&self, Parameters(p): Parameters<ReplyParams>) -> CallToolResult {
        reply((|| {
            let mut threads = self.lib.threads(&p.path)?;
            let thread = threads
                .iter_mut()
                .find(|t| t.id == p.thread_id)
                .ok_or_else(|| format!("No thread {} on {}", p.thread_id, p.path))?;
            thread.comments.push(CommentMsg {
                id: library::new_id("msg"),
                text: p.text.clone(),
                created_at: library::iso_now(),
                author: Some(p.author.clone().unwrap_or_else(|| "Claude".into())),
            });
            self.lib.save_threads(&p.path, &threads)?;
            Ok(format!("Replied to {}", p.thread_id))
        })())
    }

    #[tool(
        name = "resolve_comment",
        annotations(title = "Resolve or reopen a comment thread"),
        description = "Mark a comment thread on an Inky document as resolved (or reopen it). Only resolve a thread after actually addressing it — e.g. after replying or updating the document."
    )]
    async fn resolve_comment(&self, Parameters(p): Parameters<ResolveParams>) -> CallToolResult {
        reply((|| {
            let mut threads = self.lib.threads(&p.path)?;
            let thread = threads
                .iter_mut()
                .find(|t| t.id == p.thread_id)
                .ok_or_else(|| format!("No thread {} on {}", p.thread_id, p.path))?;
            thread.resolved = p.resolved.unwrap_or(true);
            let verb = if thread.resolved { "Resolved" } else { "Reopened" };
            self.lib.save_threads(&p.path, &threads)?;
            Ok(format!("{verb} {}", p.thread_id))
        })())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for InkyMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("inky", env!("CARGO_PKG_VERSION")))
        .with_instructions(INSTRUCTIONS)
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        async move {
            let resources = self
                .lib
                .walk()
                .into_iter()
                .filter(|e| e.kind != EntryKind::Folder)
                .map(|e| {
                    let mut r = Resource::new(format!("{DOC_URI_PREFIX}{}", e.rel), e.rel.clone());
                    r.mime_type = Some("text/markdown".into());
                    r
                })
                .collect();
            Ok(ListResourcesResult::with_all_items(resources))
        }
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, McpError>> + Send + '_ {
        async move {
            let mut t = ResourceTemplate::new(format!("{DOC_URI_PREFIX}{{+path}}"), "document");
            t.title = Some("Inky documents".into());
            t.description = Some("Markdown documents in the user's Inky library".into());
            t.mime_type = Some("text/markdown".into());
            Ok(ListResourceTemplatesResult::with_all_items(vec![t]))
        }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResponse, McpError>> + Send + '_ {
        async move {
            let rel = request
                .uri
                .strip_prefix(DOC_URI_PREFIX)
                .ok_or_else(|| McpError::resource_not_found(format!("unknown resource {}", request.uri), None))?;
            let text = self
                .lib
                .read(&percent_decode(rel))
                .map_err(|e| McpError::resource_not_found(e, None))?;
            let mut contents = ResourceContents::text(text, request.uri.clone());
            if let ResourceContents::TextResourceContents { mime_type, .. } = &mut contents {
                *mime_type = Some("text/markdown".into());
            }
            Ok(ReadResourceResult::new(vec![contents]).into())
        }
    }
}

/// Undo `%XX` escapes a client may apply to a resource URI before reading it.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

// --- transports -------------------------------------------------------------

/// Serve MCP over stdin/stdout until the client closes the pipe. Runs on its own
/// runtime because it is used from `main` before (instead of) Tauri.
/// Returns the process exit code. Only JSON-RPC goes to stdout.
pub fn serve_stdio_blocking() -> i32 {
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Inky MCP: cannot start runtime: {e}");
            return 1;
        }
    };
    let code = rt.block_on(async {
        let lib = match Library::from_env() {
            Ok(lib) => lib,
            Err(e) => {
                eprintln!("Inky MCP: cannot open the library: {e}");
                return 1;
            }
        };
        eprintln!(
            "Inky MCP server running on stdio\n  Library: {}\n  Tools:   {TOOL_SUMMARY}\n\n\
             This process is meant to be launched by an MCP client (it waits for JSON-RPC on stdin).",
            lib.root().display()
        );
        let service = match InkyMcp::new(lib).serve(rmcp::transport::stdio()).await {
            Ok(s) => s,
            // The client went away before initialising — not an error worth a non-zero exit.
            Err(_) => return 0,
        };
        match service.waiting().await {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Inky MCP: {e}");
                1
            }
        }
    });
    // Don't wait for the blocking stdin reader: a client that keeps the pipe
    // open after the session ends would otherwise hang us on runtime drop.
    rt.shutdown_background();
    code
}

/// A running app-hosted HTTP server. Dropping the handle does not stop it; call `stop()`.
#[derive(Debug)]
pub struct HttpHandle {
    pub addr: SocketAddr,
    token: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl HttpHandle {
    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.addr)
    }

    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }

    pub fn stop(&self) {
        self.token.cancel();
    }
}

/// Bind `127.0.0.1:port` (0 = any free port) and serve streamable-HTTP MCP at `/mcp`
/// on the current tokio runtime.
pub async fn serve_http(lib: Library, port: u16) -> Result<HttpHandle, String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            format!("port {port} is already in use — is another Inky (or something else) listening?")
        } else {
            format!("cannot listen on 127.0.0.1:{port}: {e}")
        }
    })?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let token = CancellationToken::new();
    let service = StreamableHttpService::new(
        move || Ok(InkyMcp::new(lib.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_cancellation_token(token.child_token()),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let shutdown = token.clone();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await;
    });
    Ok(HttpHandle { addr, token, task })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, ReadResourceRequestParams};

    struct Fixture {
        _dir: tempfile::TempDir,
        lib: Library,
        client: rmcp::service::RunningService<rmcp::RoleClient, ()>,
        server: tokio::task::JoinHandle<()>,
    }

    async fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let (server_io, client_io) = tokio::io::duplex(1 << 16);
        let handler = InkyMcp::new(lib.clone());
        let server = tokio::spawn(async move {
            let s = handler.serve(server_io).await.unwrap();
            s.waiting().await.ok();
        });
        let client = ().serve(client_io).await.unwrap();
        Fixture { _dir: dir, lib, client, server }
    }

    async fn call(f: &Fixture, name: &str, args: serde_json::Value) -> (bool, String) {
        let r = f
            .client
            .call_tool(
                CallToolRequestParams::new(name.to_string())
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        let text = r
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .map(|t| t.text.clone())
            .collect::<String>();
        (r.is_error == Some(true), text)
    }

    #[tokio::test]
    async fn lists_every_tool() {
        let f = fixture().await;
        let tools = f.client.list_tools(None).await.unwrap();
        let mut names: Vec<String> = tools.tools.iter().map(|t| t.name.to_string()).collect();
        names.sort();
        let mut expected: Vec<String> = TOOL_SUMMARY.split(',').map(|s| s.trim().to_string()).collect();
        expected.sort();
        assert_eq!(names, expected);
        f.server.abort();
    }

    /// A server whose `open_document` records paths instead of launching Inky.
    async fn fixture_recording_opens() -> (Fixture, std::sync::Arc<std::sync::Mutex<Vec<std::path::PathBuf>>>) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let opened = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = opened.clone();
        let handler = InkyMcp::new(lib.clone()).with_opener(std::sync::Arc::new(move |p: &std::path::Path| {
            log.lock().unwrap().push(p.to_path_buf());
            Ok(())
        }));
        let (server_io, client_io) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let s = handler.serve(server_io).await.unwrap();
            s.waiting().await.ok();
        });
        let client = ().serve(client_io).await.unwrap();
        (Fixture { _dir: dir, lib, client, server }, opened)
    }

    #[tokio::test]
    async fn open_document_shows_a_library_document_in_the_app() {
        let (f, opened) = fixture_recording_opens().await;
        f.lib.write("Notes/a b.md", "x").unwrap();
        let (err, msg) = call(&f, "open_document", serde_json::json!({"path": "Notes/a b.md"})).await;
        assert!(!err, "{msg}");
        assert!(msg.contains("Notes/a b.md"), "{msg}");
        assert_eq!(*opened.lock().unwrap(), vec![f.lib.root().join("Notes/a b.md")]);
        f.server.abort();
    }

    #[tokio::test]
    async fn open_document_refuses_anything_but_existing_library_documents() {
        let (f, opened) = fixture_recording_opens().await;
        f.lib.write("a.md", "x").unwrap();
        std::fs::write(f.lib.root().join("notes.txt"), "t").unwrap();
        for path in ["../outside.md", "/etc/hosts", "notes.txt", "missing.md", "", "."] {
            let (err, msg) = call(&f, "open_document", serde_json::json!({ "path": path })).await;
            assert!(err, "{path:?} should be refused, got {msg}");
        }
        assert!(opened.lock().unwrap().is_empty(), "nothing may be opened");
        f.server.abort();
    }

    #[tokio::test]
    async fn writes_reads_lists_and_searches() {
        let f = fixture().await;
        let (err, msg) = call(
            &f,
            "write_document",
            serde_json::json!({"path": "a/b.md", "content": "# Hi\nneedle\n"}),
        )
        .await;
        assert!(!err && msg.contains("Saved a/b.md"));
        let (_, body) = call(&f, "read_document", serde_json::json!({"path": "a/b.md"})).await;
        assert_eq!(body, "# Hi\nneedle\n");
        let (_, listing) = call(&f, "list_documents", serde_json::json!({})).await;
        assert!(listing.contains("📁 a/") && listing.contains("📄 a/b.md"), "{listing}");
        let (_, hits) = call(&f, "search_documents", serde_json::json!({"query": "NEEDLE"})).await;
        assert_eq!(hits, "a/b.md:2: needle");
        f.server.abort();
    }

    #[tokio::test]
    async fn rejects_escapes_and_bad_extensions_as_tool_errors() {
        let f = fixture().await;
        let (err, msg) = call(&f, "read_document", serde_json::json!({"path": "../outside.md"})).await;
        assert!(err && msg.contains("escapes"), "{msg}");
        let (err, msg) = call(
            &f,
            "write_document",
            serde_json::json!({"path": "evil.sh", "content": "x"}),
        )
        .await;
        assert!(err && msg.contains("Unsupported extension"), "{msg}");
        f.server.abort();
    }

    #[tokio::test]
    async fn patches_with_exact_match_safety() {
        let f = fixture().await;
        call(
            &f,
            "write_document",
            serde_json::json!({"path": "p.md", "content": "alpha beta alpha\n"}),
        )
        .await;
        let (err, msg) = call(
            &f,
            "patch_document",
            serde_json::json!({"path": "p.md", "old_text": "alpha", "new_text": "x"}),
        )
        .await;
        assert!(err && msg.contains("occurs 2 times"));
        let (err, _) = call(
            &f,
            "patch_document",
            serde_json::json!({"path": "p.md", "old_text": "beta", "new_text": "gamma"}),
        )
        .await;
        assert!(!err);
        assert_eq!(f.lib.read("p.md").unwrap(), "alpha gamma alpha\n");
        f.server.abort();
    }

    #[tokio::test]
    async fn manages_structure() {
        let f = fixture().await;
        call(&f, "write_document", serde_json::json!({"path": "m.md", "content": "x"})).await;
        let (_, msg) = call(
            &f,
            "rename_document",
            serde_json::json!({"path": "m.md", "new_name": "renamed"}),
        )
        .await;
        assert_eq!(msg, "Renamed to renamed.md");
        let (_, msg) = call(
            &f,
            "move_document",
            serde_json::json!({"path": "renamed.md", "target_folder": "Archive"}),
        )
        .await;
        assert_eq!(msg, "Moved to Archive/renamed.md");
        let (_, msg) = call(&f, "create_folder", serde_json::json!({"path": "Projects/Inky"})).await;
        assert_eq!(msg, "Created folder Projects/Inky");
        let (err, msg) = call(&f, "delete_document", serde_json::json!({"path": "Archive"})).await;
        assert!(err && msg.contains("Only documents"), "{msg}");
        let (err, msg) = call(&f, "delete_document", serde_json::json!({"path": "Archive/renamed.md"})).await;
        assert!(!err && msg == "Deleted Archive/renamed.md");
        f.server.abort();
    }

    #[tokio::test]
    async fn versions_are_reachable() {
        let f = fixture().await;
        call(&f, "write_document", serde_json::json!({"path": "v.md", "content": "one"})).await;
        call(&f, "write_document", serde_json::json!({"path": "v.md", "content": "two"})).await;
        let (_, list) = call(&f, "list_versions", serde_json::json!({"path": "v.md"})).await;
        let name = list.split_whitespace().next().unwrap().to_string();
        assert!(name.starts_with("v.") && name.ends_with(".md"), "{list}");
        assert!(name.ends_with(".agent.md"), "MCP snapshots are agent-tagged: {name}");
        let (_, body) = call(
            &f,
            "read_version",
            serde_json::json!({"path": "v.md", "version": name}),
        )
        .await;
        assert_eq!(body, "one");
        f.server.abort();
    }

    #[tokio::test]
    async fn comment_threads_lifecycle() {
        let f = fixture().await;
        call(
            &f,
            "write_document",
            serde_json::json!({"path": "c.md", "content": "hello brave world"}),
        )
        .await;
        let (err, msg) = call(
            &f,
            "create_comment",
            serde_json::json!({"path": "c.md", "quote": "nope", "text": "?"}),
        )
        .await;
        assert!(err && msg.contains("Quote not found"));
        let (_, msg) = call(
            &f,
            "create_comment",
            serde_json::json!({"path": "c.md", "quote": "brave", "text": "Why brave?"}),
        )
        .await;
        let id = msg.strip_prefix("Created ").unwrap().split(' ').next().unwrap().to_string();
        assert!(id.starts_with("thread-"));
        let threads = f.lib.threads("c.md").unwrap();
        assert_eq!(threads[0].prefix, "hello ");
        assert_eq!(threads[0].suffix, " world");
        assert_eq!(threads[0].comments[0].author.as_deref(), Some("Claude"));
        call(
            &f,
            "reply_to_comment",
            serde_json::json!({"path": "c.md", "thread_id": id, "text": "Because.", "author": "GPT"}),
        )
        .await;
        let (_, listing) = call(
            &f,
            "list_comments",
            serde_json::json!({"path": "c.md", "filter": "open"}),
        )
        .await;
        assert!(
            listing.contains("[Claude] Why brave?") && listing.contains("[GPT] Because."),
            "{listing}"
        );
        let (_, msg) = call(
            &f,
            "resolve_comment",
            serde_json::json!({"path": "c.md", "thread_id": id}),
        )
        .await;
        assert_eq!(msg, format!("Resolved {id}"));
        let (_, listing) = call(
            &f,
            "list_comments",
            serde_json::json!({"path": "c.md", "filter": "open"}),
        )
        .await;
        assert_eq!(listing, "No open comments on c.md");
        f.server.abort();
    }

    #[tokio::test]
    async fn exposes_documents_as_resources() {
        let f = fixture().await;
        f.lib.write("r.md", "resource body").unwrap();
        let list = f.client.list_resources(None).await.unwrap();
        assert_eq!(list.resources[0].uri, "inky://doc/r.md");
        let read = f
            .client
            .read_resource(ReadResourceRequestParams::new("inky://doc/r.md"))
            .await
            .unwrap();
        let rmcp::model::ResourceContents::TextResourceContents { text, mime_type, .. } = &read.contents[0]
        else {
            panic!("expected text");
        };
        assert_eq!(text, "resource body");
        assert_eq!(mime_type.as_deref(), Some("text/markdown"));
        assert!(f
            .client
            .read_resource(ReadResourceRequestParams::new("inky://doc/../x"))
            .await
            .is_err());
        f.lib.write("My notes.md", "spaced").unwrap();
        let read = f
            .client
            .read_resource(ReadResourceRequestParams::new("inky://doc/My%20notes.md"))
            .await
            .unwrap();
        let rmcp::model::ResourceContents::TextResourceContents { text, .. } = &read.contents[0] else {
            panic!("expected text");
        };
        assert_eq!(text, "spaced");
        f.server.abort();
    }

    #[tokio::test]
    async fn tools_carry_titles() {
        let f = fixture().await;
        let tools = f.client.list_tools(None).await.unwrap();
        let t = tools.tools.iter().find(|t| t.name == "list_documents").unwrap();
        assert_eq!(t.annotations.as_ref().and_then(|a| a.title.as_deref()), Some("List Inky documents"));
        f.server.abort();
    }

    #[tokio::test]
    async fn http_server_answers_initialize_and_stops() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let handle = serve_http(lib, 0).await.unwrap();
        assert!(handle.is_running());
        assert!(handle.url().starts_with("http://127.0.0.1:") && handle.url().ends_with("/mcp"));
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
        let mut stream = tokio::net::TcpStream::connect(handle.addr).await.unwrap();
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("\"name\":\"inky\""), "{response}");
        handle.stop();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while handle.is_running() {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("server task should finish after stop()");
        assert!(
            tokio::net::TcpStream::connect(handle.addr).await.is_err(),
            "port released"
        );
    }

    #[tokio::test]
    async fn http_port_in_use_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let first = serve_http(lib.clone(), 0).await.unwrap();
        let err = serve_http(lib, first.addr.port()).await.unwrap_err();
        assert!(err.contains("already in use"), "{err}");
        first.stop();
    }
}
