# Hippo / Silt Integration Plan

## What are Hippo and Silt?

- **Hippo** (`C:\Users\kalli\Development\zig-projects\hippo`) is a temporal graph + vector memory store. It exposes nodes, edges, recall/search, reflection, and consolidation.
- **Silt** (`C:\Users\kalli\Development\zig-projects\silt`) is a filesystem indexer that feeds Hippo. It scans directories and turns files/dirs/tags into `fs_file`, `fs_dir`, and `fs_tag` nodes.

Both are Zig projects, both build successfully on Windows with Zig 0.16.0, and both are part of the same personal ecosystem as Veyra.

## Interfaces available for Veyra

### Hippo HTTP server

```bash
hippo serve --dir <store> --port 7345
```

Useful endpoints for Veyra:

| Endpoint | Use case |
|----------|----------|
| `GET /api/recall?query=...&k=5&hybrid=1` | Semantic + BM25 memory search |
| `GET /api/nodes?topic_substring=...&limit=10` | Fast substring lookup (no embeddings needed) |
| `GET /api/timeline?limit=20` | Recent memory |
| `GET /api/node/{id}` | Show node + edges |
| `GET /api/stats` | Store stats |

### Hippo CLI

```bash
hippo store --topic "..." --content "..." --kind fact
hippo recall --query "..." --k 5 --pretty
hippo link --from 12 --to 34 --kind related
```

### Hippo MCP server

```bash
hippo mcp --dir <store>
```

Exposes tools like `recall`, `store`, `walk`, `stats`, `fs_query` over JSON-RPC on stdin/stdout.

### Silt CLI

```bash
silt scan --hippo-dir <store> C:\Users\kalli\Documents
silt tag --hippo-dir <store>
silt daemon --hippo-dir <store>
```

## Recommended integration

### Phase 1 — Bridge plugin (no Veyra core changes)

A Python JSON-RPC bridge is provided at `scripts/hippo-veyra-bridge.py`. It:

- Implements Veyra's stdio JSON-RPC protocol.
- Calls Hippo `/api/nodes?topic_substring=...` when you type `recall <query>`, `mem <query>`, or `hippo <query>`.
- Falls back to `/api/recall?fts=1` if no substring hits.
- Returns results as Veyra tool-call items (copy content to clipboard).
- Also provides static entries for "Recall from memory" and "Recent memory".

Add to `%APPDATA%\Veyra\plugins.toml`:

```toml
[[plugins]]
id = "hippo"
label = "Hippo Memory"
kind = "json_rpc_stdio"
command = "python"
args = ["C:\\Users\\kalli\\Development\\tools\\veyra-launcher\\scripts\\hippo-veyra-bridge.py"]
keywords = ["hippo", "memory", "recall", "notes"]
enabled = true
```

Point it at a non-default Hippo server via environment:

```powershell
$env:HIPPO_HOST = "127.0.0.1:7345"
```

### Phase 2 — AI tool manifest

Create `%APPDATA%\Veyra\tools\hippo.json`:

```json
{
  "name": "hippo_recall",
  "description": "Recall facts, decisions, or notes from the Hippo memory store",
  "keywords": ["memory", "recall", "hippo"],
  "platforms": ["windows", "linux"],
  "runner": "Process",
  "command": "hippo",
  "args": ["recall", "--query", "{query}", "--k", "5", "--pretty"],
  "parameters": {
    "type": "object",
    "properties": {
      "query": { "type": "string", "description": "What to recall" }
    },
    "required": ["query"]
  },
  "safety": "Read",
  "timeout_ms": 10000
}
```

### Phase 3 — Native quick tool (optional)

Once stable, add a first-class `hippo_search_result()` in `veyra-app/src/main.rs` that calls `http://127.0.0.1:7345/api/nodes` directly, avoiding the Python bridge.

### Phase 4 — Silt daemon management

Veyra can start/stop `silt daemon --hippo-dir <store>` so the filesystem graph stays current. Because Silt outputs human debug text, Veyra should ignore stdout and just check that the process is alive.

## Data flow

```
Filesystem  -->  Silt  -->  Hippo store  -->  Veyra (via HTTP / bridge plugin)
   ^                                              |
   +----------- AI notes, saved facts -------------+
```

Veyra can both read from and write to Hippo:

- **Read**: recall/search memory and files.
- **Write**: save notes, decisions, or tagged facts via `hippo store`.

## Known issues / caveats

- **Embedding dependency**: Full semantic recall needs a running OpenAI-compatible embeddings endpoint. The bridge uses `topic_substring` and `fts=1` first to avoid that.
- **Single-threaded HTTP server**: `hippo serve` handles one request at a time; SSE streaming blocks other clients.
- **No concurrent writers**: only one process should write to a Hippo store at a time.
- **macOS compile issue**: `hippo` uses `std.os.linux.getpid()` in an `else` branch; macOS builds fail until fixed.
- **Silt is Windows-centric for image features**: GDI+ image analysis does not work on Linux/macOS yet.

## Security notes

- Hippo `serve` binds to `127.0.0.1` only by default.
- Without encryption, the store files are readable on disk. Use `hippo init --encrypt` for sensitive data.
- The bridge copies returned memory content to the clipboard; do not expose Hippo to untrusted clients.

## Summary

Hippo and Silt are the most integration-ready projects in the ecosystem:

- They build and run today.
- They expose clean HTTP and CLI interfaces.
- The provided bridge plugin lets Veyra query memory/files immediately.

Next step: ensure `hippo serve` is running and add the bridge plugin to `plugins.toml`.
