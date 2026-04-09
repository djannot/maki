# maki-code-index-mcp

An [MCP](https://modelcontextprotocol.io/) server that exposes
[`maki-code-index`](../maki-code-index/) as a single `index` tool. It returns a
compact skeleton of a source file — imports, type definitions, and function
signatures with line numbers — that is typically 70–90% smaller than the
original file while preserving the structure an LLM needs to navigate code.

Supports 15 languages out of the box: Rust, Python, TypeScript, JavaScript,
Go, Java, C, C++, C#, Ruby, PHP, Swift, Kotlin, Scala, Bash, Lua.

## Build

From the workspace root:

```sh
cargo build --release -p maki-code-index-mcp
```

The binary is produced at `target/release/maki-code-index-mcp`.

## Configure your MCP client

The server speaks JSON-RPC over stdio, which is what local MCP clients launch
subprocesses expect.

### Claude Code

Register the server with the `claude mcp add` command:

```sh
claude mcp add maki-index /absolute/path/to/target/release/maki-code-index-mcp
```

To scope it to the current project only, add `--scope project`; to make it
available across all projects for your user, add `--scope user` (the default
is `local`, which stores it in the current project's local settings).

To pass the optional file-size cap:

```sh
claude mcp add maki-index \
  --env MAKI_INDEX_MAX_FILE_SIZE=4194304 \
  /absolute/path/to/target/release/maki-code-index-mcp
```

### Cursor, Zed, Continue, …

Add an entry to the client's MCP server config:

```json
{
  "mcpServers": {
    "maki-index": {
      "command": "/absolute/path/to/target/release/maki-code-index-mcp"
    }
  }
}
```

The tool then appears to the agent as `mcp__maki-index__index`.

## Tool reference

### `index`

Return a compact skeleton of a source file.

**Input:**

| Field  | Type   | Required | Description                             |
|--------|--------|----------|-----------------------------------------|
| `path` | string | yes      | **Absolute** path to the source file.   |

**Why absolute-only?** MCP servers have no notion of an "agent cwd", and
resolving relative paths against the server's own working directory would be
surprising and error-prone. Have the agent resolve paths before calling.

**Errors** (returned as JSON-RPC `-32602 Invalid params`):

- `path must be absolute` — the caller passed a relative path.
- `unsupported file type: .<ext>` — no tree-sitter grammar for that extension.
  Fall back to reading the file directly.
- `file too large (<size> bytes, max <max>)` — the file exceeds the configured
  size cap. Read it directly with a range instead.

## Environment variables

| Variable                    | Default           | Description                                                                    |
|-----------------------------|-------------------|--------------------------------------------------------------------------------|
| `MAKI_INDEX_MAX_FILE_SIZE`  | `2097152` (2 MiB) | Maximum file size the server will attempt to parse, in bytes.                  |
| `RUST_LOG`                  | `info`            | Tracing filter. Logs are written to **stderr** so they don't corrupt the MCP channel. |

## Example

With the binary running under an MCP client, an agent call like:

```json
{"name": "index", "arguments": {"path": "/path/to/maki/maki-code-index/src/lib.rs"}}
```

returns something like:

```
module doc: [1-5]

imports: [7-13]
  index::{IndexError, common::LanguageExtractor, index_file, index_source}

mod: [9-11]
  pub find_symbol, pub(crate) helpers, pub index

types:
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Language [16-49]
    Rust, Python, TypeScript, JavaScript, Go, Java, C, Cpp, [8 more truncated]

impls:
  Language [51-167]
    pub from_extension(ext: &str) -> Option<Self> [52-92]
    pub ts_language(&self) -> tree_sitter::Language [94-129]
    extractor(&self) -> &dyn LanguageExtractor [131-166]
```

## Manual smoke test

You can drive the server directly with newline-delimited JSON-RPC:

```sh
printf '%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0.0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | ./target/release/maki-code-index-mcp
```

## Notes on binary size

All 15 tree-sitter grammars are statically linked by default, which makes the
release binary fairly large. If that matters, build with a reduced feature set
on `maki-code-index` — each language has its own `lang-*` feature.

## License

MIT, same as the rest of the maki workspace.
