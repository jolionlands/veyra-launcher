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
  plugins.toml
```

## config.toml

```toml
[general]
startup = true
local_only = false
history_limit = 5000

[hotkeys]
toggle = "Alt+Space"
settings = "Ctrl+,"

[appearance]
theme = "dark-acrylic"
opacity = 0.92
blur = true
font_size = 15
max_results = 10
show_preview = true
```

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

