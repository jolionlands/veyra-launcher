# AI And Tools

## Goal

AI should be useful from the launcher without becoming mandatory for startup, search, or app launching.

## Implemented Query Modes

| Mode | Trigger example | Behavior |
| --- | --- | --- |
| Ask | `ask why is the bar missing` | Send the prompt to the selected provider and show the captured answer in Veyra. |
| Provider ask | `llama summarize this`, `pike review this`, `pylon summarize this`, `npu count to three` | Route the prompt to a configured provider prefix instead of the default provider. |
| Action capture | `ai open WireGuard` | Resolve a supported tool/action intent, show a confirmation card, and run only after user confirmation. |
| Calculator | `ai calculate 17 * 23` | Answer directly without the model when the request is a safe arithmetic expression. |
| Clock/location | `what time is it in Tokyo?` | Answer deterministic time/location prompts without model context bleed. |
| Clipboard assist | `summarize clipboard` from the compose toolbar | Include clipboard text when the prompt explicitly asks for it or the user clicks `Clip`. |

Deterministic launcher results rank above AI guesses unless the user explicitly uses an AI prefix.

Planned but not implemented as first-class modes yet: memory `remember`/`recall`, broad `explain selected text`, full native tool-call schemas, and elevated/admin tool execution.

## Provider Settings

- `id`
- `label`
- `kind`
- `base_url`
- `model`
- `command`
- `args`
- `keep_warm`
- `api_key_env`
- `local_only`
- `enabled`
- `timeout_ms`
- `supports_streaming`
- `supports_tools`

Provider classes:

- OpenAI-compatible HTTP endpoint.
- Local llama.cpp-compatible HTTP endpoint.
- Local process provider, including the MiniCPM5 NPU `npu_chat.exe` runner.
- Pike process provider through `scripts/veyra-pike.ps1` for read-only agent-style answers.
- Pylon OpenAI-compatible router on `http://127.0.0.1:8088/v1`.
- Local NPU/Genie HTTP shim endpoint.
- Optional cloud provider.
- Mock provider for tests and offline UI work.

For `kind = "open_ai_compatible"`, Veyra sends Ask prompts directly to `{base_url}/chat/completions` when the configured base URL already ends in `/v1`; otherwise it appends `/v1/chat/completions`. For one-shot `kind = "process"`, Veyra writes a ChatML prompt file, runs `command` with `args` plus `--prompt-file <file>`, and captures stdout as the answer. If a process provider's `args` include prompt placeholders, Veyra substitutes them and does not append the default `--prompt-file`: `{prompt}` is the raw model prompt, `{prompt_file}` is a temporary raw prompt file, `{chatml_prompt}` is the MiniCPM/ChatML wrapper, and `{chatml_prompt_file}` is a temporary wrapped prompt file. When `keep_warm = true`, Veyra keeps the process alive, sends prompts over stdin with the runner's `\END` terminator, and reuses the loaded model between asks. The optional Kyrphina bridge plugin can still open the external chat panel, but it is no longer required for `ai`, `ask`, `chat`, `kyrphina`, `llama`, `pike`, `pylon`, or `npu` launcher prompts.

When `[ai].warmup_on_startup = true`, Veyra starts the default warm process provider after profile load so the first Ask query can reuse an already loaded model.

Conversation follow-ups reuse the active session provider. For example, a chat started with `llama ...` keeps using the `llama` provider until the AI session is cleared or a new top-level AI query starts.

`[general].local_only = true`, `[ai].local_only = true`, or provider-level `local_only = true` blocks non-local HTTP AI endpoints. Local process providers are allowed.

AI requests are logged to `ai-chat-log.jsonl`; saved Markdown snapshots are written under `ai-chats/`.

## Current Tool Calls

Veyra currently recognizes these model-emitted or proactively inferred calls:

- `open_result(query)`
- `search(query)`
- `open_url(url)`
- `copy_to_clipboard(text)`
- `calculate(expression)`
- `current_time(location)`

Unsupported call names are shown as unresolved instead of being executed. Guarded actions require confirmation; launcher guarded results use `Shift+Enter`, and AI action cards use their visible `Run` button.

## Tool Manifest

Profile-local manifest JSON files are loaded from `tools/` and `plugins/` under the active profile during catalog refresh. Each valid manifest becomes a Tool catalog item.

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

Write, execute, admin, and network actions require confirmation by default. In the launcher, guarded actions are confirmed with `Shift+Enter`.
