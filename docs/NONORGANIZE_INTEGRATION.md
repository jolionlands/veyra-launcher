# Nonorganize Integration Plan

## What is Nonorganize?

Nonorganize (in `C:\Users\kalli\Development\zig-projects\Nonorganize`) is a Zig-based file indexer/organizer with:

- A local HTTP API (`serve`) on `localhost:8080` exposing `/stats`, `/search?q=...`, and `/similar?file=...`.
- An MCP server (`mcp`) over stdin/stdout with tools like `search_files`, `organize_files`, `find_similar`, `get_rules`, `add_rule`.
- A browser extension that posts webpage context to `localhost:8080` (currently not wired to a server route).
- A long-running watch/daemon mode for automatic organization.

> **Current blocker:** The Zig implementation does not currently compile on Zig 0.16.0 (`zig build` segfaults). The pre-built root binary is a Linux x86-64 Go ELF, so it cannot run on Windows. Before any integration is live, Nonorganize itself needs to be buildable/runnable on the target platform.

## How Veyra can talk to Nonorganize

Veyra has three extension mechanisms that map cleanly to Nonorganize:

| Veyra mechanism | Nonorganize interface | Use case |
|-----------------|----------------------|----------|
| JSON-RPC stdio plugin (`plugins.toml`) | HTTP API on `localhost:8080` | Search files and open them from Veyra. |
| AI tool manifest (`tools/*.json`) | MCP server `nonorganize mcp` | Let the AI call `search_files`, `organize_files`, etc. |
| Shell command entries (`commands.toml`) | CLI | Run `nonorganize organize`, `watch`, etc. |

## Recommended integration

### Phase 1 — Bridge plugin (no Veyra core changes)

A Python JSON-RPC bridge is provided at `scripts/nonorganize-veyra-bridge.py`. It:

- Implements Veyra's stdio JSON-RPC protocol (`initialize`, `catalog`, `suggest`, `execute`, `shutdown`).
- Calls Nonorganize's HTTP `/search` endpoint when you type `no <query>` or `non <query>`.
- Returns file results as Veyra `open_file` actions.
- Also provides static entries for "Search files" and "Show stats".

Add to `%APPDATA%\Veyra\plugins.toml`:

```toml
[[plugins]]
id = "nonorganize"
label = "Nonorganize Bridge"
kind = "json_rpc_stdio"
command = "python"
args = ["C:\\Users\\kalli\\Development\\tools\\veyra-launcher\\scripts\\nonorganize-veyra-bridge.py"]
keywords = ["nonorganize", "files", "search", "organize"]
enabled = true
```

Set the Nonorganize host if it is not on `127.0.0.1:8080`:

```toml
[plugin_env]
NONORGANIZE_HOST = "127.0.0.1:8080"
```

(Note: Veyra does not currently have a dedicated `[plugin_env]` section; pass env through the OS or wrap the bridge in a batch file.)

### Phase 2 — AI tool manifest

Create `%APPDATA%\Veyra\tools\nonorganize.json`:

```json
{
  "name": "nonorganize_search",
  "description": "Search the Nonorganize file index",
  "keywords": ["files", "search", "nonorganize"],
  "platforms": ["windows", "linux"],
  "runner": "Process",
  "command": "nonorganize",
  "args": ["search", "--query", "{query}"],
  "parameters": {
    "type": "object",
    "properties": {
      "query": { "type": "string", "description": "Search query" }
    },
    "required": ["query"]
  },
  "safety": "Read",
  "timeout_ms": 5000
}
```

This lets Veyra's AI invoke Nonorganize as a tool. When Nonorganize exposes JSON output, this becomes fully useful.

### Phase 3 — Native Veyra quick tool (optional)

Once Nonorganize is stable, a first-class quick tool can be added to `veyra-app/src/main.rs` (similar to `aurora_search_result`) that queries `/search` directly. This avoids starting a Python bridge per keystroke.

## Known issues to resolve in Nonorganize first

1. **Build failure** — `zig build` segfaults on Zig 0.16.0. This is the primary blocker.
2. **No Windows support** — X11 hotkey code and Linux-only prebuilt binary.
3. **No JSON output from CLI** — `printJsonOutput` is unimplemented, so AI/tool invocation via CLI is limited.
4. **Orphaned browser extension** — POSTs to `localhost:8080` but the Zig server has no receiving route.
5. **YAML vs JSON config mismatch** — docs say YAML; parser is JSON-only.
6. **Single-threaded HTTP server** — concurrent Veyra queries could queue/block.

## Security notes

- Nonorganize's HTTP server currently has **no authentication** by default. Only bind to `localhost`.
- Running `organize_files` can move/delete files. Tool manifests should use `"safety": "Write"` or `"Execute"` so Veyra asks for confirmation.
- The bridge runs with the user's privileges and can open any file returned by Nonorganize.

## Summary

Yes, Nonorganize can be plugged into Veyra. The cleanest path is the provided JSON-RPC bridge over Nonorganize's HTTP API, followed by AI tool manifests once CLI JSON output works. The main prerequisite is getting Nonorganize to build and run reliably on the target platform.
