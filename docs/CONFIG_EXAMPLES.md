# Config Examples

## Profile Layout

Windows:

```text
%APPDATA%\Veyra
```

Linux:

```text
~/.config/veyra
```

Portable mode:

```text
veyra.exe
portable/
  config.toml
  commands.toml
  plugins.toml
  tools/
  catalogs.toml
  ai.toml
  history.json
```

## Profile Merge Order

Veyra merges files in this order:

1. `config.toml` (full overwrite for general/hotkeys/appearance)
2. `commands.toml` (append commands, web searches, and importer-emitted catalog profiles)
3. `plugins.toml` (append local process and JSON-RPC stdio plugin entries)
4. `catalogs.toml` (append catalog profiles)
5. `ai.toml` (AI settings; provider tables can be `[[ai.providers]]` or `[[providers]]`)

## Runtime Reload Behavior

Profile data is only loaded on startup and when explicitly requested.

- Press `Ctrl+R`, or use **Reload profile** from Settings, to re-read all profile files and rebuild catalog indexes.
- Use **Refresh catalogs** in the Catalogs view to force re-running catalog indexing after changing `catalogs.toml`.
- **Open profile file** actions in Settings create missing files using the same templates shown below:
  - `config.toml`
  - `commands.toml`
  - `plugins.toml`
  - `catalogs.toml`
  - `ai.toml`

`history.json` is managed by Veyra. It records successful launches, shows recent choices when the launcher opens with an empty search, and gives repeated choices a local ranking boost for matching searches. It can be cleared from Diagnostics.

## config.toml

```toml
[general]
startup = true
local_only = false
history_limit = 5000

[hotkeys]
toggle = "Win+Shift+F23"
settings = "Ctrl+,"

[appearance]
theme = "dark-acrylic"
opacity = 0.72
blur = true
font_size = 15
max_results = 10
show_preview = true
```

`Win+Shift+F23` is the Copilot-key chord used by many Windows keyboards. Veyra also registers `Alt+Space` as a fallback toggle even when the configured toggle is different. On Windows, the running app installs a low-level Copilot-key hook so the shell does not take that chord first.

`general.local_only = true` blocks non-local HTTP AI endpoints globally. `[ai].local_only` and provider-level `local_only` can also enforce local-only behavior for AI providers.

## commands.toml

```toml
[[commands]]
id = "settings.display"
label = "Settings: Display"
command = "explorer.exe"
args = ["ms-settings:display"]
terminal = false
keywords = ["display", "monitor", "resolution"]

[[commands]]
id = "wm.repair_bar"
label = "WM: Repair Bar"
command = "%USERPROFILE%\\scripts\\repair-bar.cmd"
terminal = true
requires_confirmation = false
keywords = ["bar", "repair", "window manager"]

[[web_search]]
id = "github.code"
alias = "gh"
label = "GitHub Code"
url = "https://github.com/search?q={query}&type=code"
```

## plugins.toml

```toml
[[plugins]]
id = "sample.echo"
label = "Sample Plugin: Echo Query"
description = "Example JSON-RPC stdio plugin"
kind = "json_rpc_stdio"
command = "python"
args = ["%USERPROFILE%\\Development\\tools\\veyra-launcher\\scripts\\sample-json-rpc-plugin.py"]
keywords = ["sample", "plugin", "jsonrpc", "echo"]
enabled = false
timeout_ms = 5000

[[plugins]]
id = "kyrphina.ask"
label = "Kyrphina: Ask"
description = "Open the Kyrphina chat panel and send the typed prompt"
kind = "process"
command = "powershell.exe"
args = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File", "%USERPROFILE%\\Development\\tools\\veyra-launcher\\scripts\\veyra-kyrphina.ps1", "-Mode", "Chat", "-Query", "{query}"]
keywords = ["ai", "assistant", "kyrphina", "chat", "ask"]
enabled = true

[[plugins]]
id = "system.guardian.log"
label = "System Guardian: Log"
description = "Open the System Guardian watchdog log"
kind = "process"
command = "notepad.exe"
args = ["%USERPROFILE%\\system-guardian.log"]
keywords = ["system", "guardian", "watchdog", "health", "log"]
enabled = true
```

Plugin `kind` defaults to `process` for backward compatibility. `json_rpc_stdio` plugins are started during catalog refresh for static catalog items and can also provide dynamic `suggest` results while typing. They should speak newline-delimited JSON-RPC over stdin/stdout using the shared protocol schemas. Plugin `command` and `args` expand `%VAR%`, `$VAR`, `${VAR}`, and leading `~`. The `{query}` token is replaced with the current launcher text before process execution. Actions with `requires_confirmation = true` run only from `Shift+Enter`.

Tool manifest JSON files can also be placed in `tools/` or `plugins/` under the active profile. Valid manifests become Tool catalog items.

## catalogs.toml

```toml
[[catalogs]]
id = "dev"
label = "Development"
paths = ["%USERPROFILE%\\Development", "C:/Work"]
include_patterns = ["*.md", "*.toml"]
exclude_patterns = ["**\\node_modules\\**"]
recursive = true
follow_symlinks = false
max_depth = 6
enabled = true
```

`catalogs.toml` also accepts `[[profiles]]` as an alias for `[[catalogs]]`.

Enabled profiles are indexed at startup. File and folder items open through the platform shell.

## ai.toml

```toml
[ai]
enabled = true
default_provider = "minicpm5_npu"
local_only = true
warmup_on_startup = false

[[providers]]
id = "minicpm5_npu"
label = "MiniCPM5-1B NPU 16K"
kind = "process"
command = "%USERPROFILE%\\Development\\npu-projects\\npu-chat\\npu_chat.exe"
args = ["%USERPROFILE%\\models\\qualcomm-genie\\minicpm5-1b\\bundle-kvint4-cl16k\\minicpm5_1b_instruct-genie-kvint4-qualcomm_snapdragon_x_plus_8_core", "--temp", "0.3", "--top-k", "40", "--top-p", "0.9", "--seed", "42"]
keep_warm = true
model = "openbmb/MiniCPM5-1B"
local_only = true
enabled = true
timeout_ms = 120000
supports_streaming = false
supports_tools = false
context_limit_tokens = 16384

[[providers]]
id = "pike"
label = "Pike coding agent"
kind = "process"
command = "powershell.exe"
args = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File", "%USERPROFILE%\\Development\\tools\\veyra-launcher\\scripts\\veyra-pike.ps1", "-PromptFile", "{prompt_file}", "-NoProjectContext", "-Tools", "read,grep,find", "-MaxTurns", "40"]
keep_warm = false
model = "pike default"
local_only = true
enabled = false
timeout_ms = 300000
supports_streaming = false
supports_tools = true
context_limit_tokens = 8000

[[providers]]
id = "pylon"
label = "Pylon router"
kind = "open_ai_compatible"
base_url = "http://127.0.0.1:8088/v1"
model = "pylon-deepseek-v4-flash"
api_key_env = "PYLON_API_KEY"
local_only = true
enabled = false
timeout_ms = 120000
supports_streaming = true
supports_tools = true

[[providers]]
id = "local_http"
label = "Local OpenAI-compatible"
kind = "open_ai_compatible"
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"
api_key_env = ""
local_only = true
enabled = true
timeout_ms = 60000
supports_streaming = true
supports_tools = true
```

Process providers support prompt placeholders in `args`. If no placeholder is present, Veyra appends `--prompt-file <chatml-file>` for NPU-style runners. If a placeholder is present, Veyra substitutes `{prompt}`, `{prompt_file}`, `{chatml_prompt}`, or `{chatml_prompt_file}` and does not append the default argument.
