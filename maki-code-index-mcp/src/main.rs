//! MCP server exposing the `maki-code-index` library as a single `index` tool.
//!
//! Transport: stdio (the standard for local MCP servers — Claude Code, Cursor,
//! Zed, Continue all launch servers as subprocesses and speak JSON-RPC over
//! stdin/stdout).
//!
//! Tool surface:
//!   - `index(path: string) -> string`
//!     Returns a compact skeleton of a source file: imports, type definitions,
//!     and function signatures with line numbers. ~70–90% fewer tokens than
//!     reading the full file.
//!
//! Configuration (env vars):
//!   - `MAKI_INDEX_MAX_FILE_SIZE` — max file size in bytes (default: 2 MiB).
//!     Requests for larger files return an error directing the caller to
//!     fall back to reading the file directly.
//!
//! Design notes:
//!   - `index_file` is a blocking, sync function. We run it on tokio's blocking
//!     pool via `spawn_blocking` so the server can keep handling other requests.
//!   - All tracing goes to stderr because stdout is reserved for the MCP
//!     JSON-RPC channel — writing anything else to stdout would corrupt it.
//!   - Paths must be absolute: MCP servers have no notion of an "agent cwd",
//!     and resolving relative paths against the server's own cwd would be
//!     surprising and error-prone.

use std::path::PathBuf;

use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;

/// Default file-size cap: 2 MiB. Large files are rejected so callers fall back
/// to incremental reads instead of loading enormous blobs into the parser.
const DEFAULT_MAX_FILE_SIZE: u64 = 2 * 1024 * 1024;

fn max_file_size_from_env() -> u64 {
    std::env::var("MAKI_INDEX_MAX_FILE_SIZE")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_MAX_FILE_SIZE)
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IndexArgs {
    /// Absolute path to the source file to index.
    pub path: String,
}

#[derive(Clone)]
pub struct IndexServer {
    max_file_size: u64,
    tool_router: ToolRouter<IndexServer>,
}

#[tool_router]
impl IndexServer {
    pub fn new(max_file_size: u64) -> Self {
        Self {
            max_file_size,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Return a compact skeleton of a source file: imports, type definitions, function signatures, and structure with their line numbers surrounded by []. ~70-90% fewer tokens than reading the full file. Use this FIRST to understand file structure before reading the full file. Supports Rust, Python, TypeScript, JavaScript, Go, Java, C, C++, C#, Ruby, PHP, Swift, Kotlin, Scala, Bash, and Lua. Returns an error on unsupported languages — fall back to reading the file directly."
    )]
    async fn index(
        &self,
        Parameters(args): Parameters<IndexArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = PathBuf::from(&args.path);
        if !path.is_absolute() {
            return Err(McpError::invalid_params(
                format!("path must be absolute, got: {}", args.path),
                None,
            ));
        }

        let max = self.max_file_size;
        let result = tokio::task::spawn_blocking(move || {
            maki_code_index::index_file(&path, max)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("join error: {e}"), None))?;

        match result {
            Ok(skeleton) => Ok(CallToolResult::success(vec![Content::text(skeleton)])),
            Err(maki_code_index::IndexError::UnsupportedLanguage(ext)) => Err(
                McpError::invalid_params(
                    format!("unsupported file type: {ext}. Read the file directly instead."),
                    None,
                ),
            ),
            Err(maki_code_index::IndexError::FileTooLarge { size, max }) => Err(
                McpError::invalid_params(
                    format!(
                        "file too large ({size} bytes, max {max}). Read the file directly with a range."
                    ),
                    None,
                ),
            ),
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }
}

#[tool_handler]
impl ServerHandler for IndexServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Use the `index` tool to obtain a compact skeleton of a source file \
                 (imports, types, function signatures + line numbers) before reading \
                 the full file. Paths must be absolute."
                    .to_string(),
            )
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logs must go to stderr — stdout is the MCP JSON-RPC channel.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let max_file_size = max_file_size_from_env();
    tracing::info!(
        max_file_size,
        "starting maki-code-index-mcp on stdio transport"
    );

    let server = IndexServer::new(max_file_size);
    let service = server.serve(stdio()).await.inspect_err(|e| {
        tracing::error!("failed to start server: {e:?}");
    })?;

    service.waiting().await?;
    Ok(())
}
