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
  catalogs.toml
  ai.toml
```

## Profile Merge Order

Veyra merges files in this order:

1. `config.toml` (full overwrite for general/hotkeys/appearance)
2. `commands.toml` (append commands, web searches, and importer-emitted catalog profiles)
3. `catalogs.toml` (append catalog profiles)
4. `ai.toml` (AI settings; provider tables can be `[[ai.providers]]` or `[[providers]]`)

## Runtime Reload Behavior

Profile data is only loaded on startup and when explicitly requested.

- Press `Ctrl+R`, or use **Reload profile** from Settings, to re-read all profile files and rebuild catalog indexes.
- Use **Refresh catalogs** in the Catalogs view to force re-running catalog indexing after changing `catalogs.toml`.
- **Open profile file** actions in Settings create missing files using the same templates shown below:
  - `config.toml`
  - `commands.toml`
  - `catalogs.toml`
  - `ai.toml`

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
opacity = 0.92
blur = true
font_size = 15
max_results = 10
show_preview = true
```

`Win+Shift+F23` is the Copilot-key chord used by many Windows keyboards. Veyra also registers `Alt+Space` as a fallback toggle even when the configured toggle is different.

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
default_provider = "local"
local_only = false
warmup_on_startup = false

[[providers]]
id = "local"
label = "Local OpenAI-compatible"
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"
api_key_env = ""
local_only = true
enabled = true
timeout_ms = 60000
supports_streaming = true
supports_tools = true
```
