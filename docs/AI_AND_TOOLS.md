# AI And Tools

## Goal

AI should be useful from the launcher without becoming mandatory for startup, search, or app launching.

## Query Modes

| Mode | Trigger example | Behavior |
| --- | --- | --- |
| Ask | `ask why is the bar missing` | Send prompt to selected provider and stream an answer. |
| Tool | `tool get system info` | Match a tool, show arguments, then execute. |
| Fix | `fix window manager bar` | Suggest repair actions from known scripts and tools. |
| Explain | `explain this error` | Explain clipboard, selected text, file, or command output. |
| Summarize | `summarize clipboard` | Summarize local text with configured provider. |
| Remember | `remember ...` | Store a local memory note. |
| Recall | `recall ...` | Search local memory notes. |

Deterministic launcher results rank above AI guesses unless the user explicitly uses an AI prefix.

## Provider Settings

- `id`
- `label`
- `base_url`
- `model`
- `api_key_env`
- `local_only`
- `enabled`
- `timeout_ms`
- `supports_streaming`
- `supports_tools`

Provider classes:

- OpenAI-compatible HTTP endpoint.
- Local llama.cpp-compatible HTTP endpoint.
- Local NPU/Genie shim endpoint.
- Optional cloud provider.
- Mock provider for tests and offline UI work.

## Tool Manifest

```json
{
  "name": "get_system_info",
  "description": "Collect basic system information.",
  "keywords": ["system", "diagnostics", "health"],
  "platforms": ["windows", "linux"],
  "runner": {
    "kind": "process",
    "command": "python",
    "args": ["get_system_info.py"]
  },
  "parameters": {
    "type": "object",
    "properties": {},
    "additionalProperties": false
  },
  "safety": {
    "level": "read",
    "requires_confirmation": false,
    "requires_admin": false
  },
  "timeout_ms": 30000
}
```

Safety levels:

- `read`: reads local data or public network data.
- `write`: writes files, notes, memory, or config.
- `execute`: runs a process or script.
- `admin`: needs elevation or changes system state.
- `network`: sends user content to a remote service.

Write, execute, admin, and network actions require confirmation by default.

