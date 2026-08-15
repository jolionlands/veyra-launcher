# Veyra

Veyra is a native keyboard launcher and AI command surface for Windows and Linux. It is designed to be fast like Keypirinha, discoverable like Flow Launcher, and portable across Windows/Linux x64 and ARM64 without Electron.

Current status: working native launcher with profile loading, command/web-search/catalog import, startup catalog indexing with a cached platform scan, indexed search, launch history, settings pages, local trusted plugin/tool loading, process and HTTP AI providers with warm-start support, snippets, quick commands (Aurora wallpaper, komorebi tiling, admin/elevate), and captured AI answers/actions.

## Goals

- Native Rust app with a polished command palette.
- Cross-compilable for Windows ARM64, Windows x64, Linux ARM64, and Linux x64.
- Local-first AI provider and tool routing.
- Built-in settings, system tools, web search, file catalog profiles, and command migration.
- Acrylic/glass visual direction on Windows where supported.
- Local process plugins, profile tool manifests, and external plugin protocol over JSON-RPC stdio.

## Workspace

```text
crates/
  veyra-app/       GUI executable
  veyra-core/      catalog, scoring, config, actions
  veyra-ai/        providers and tool manifest schemas
  veyra-plugin/    local plugin/tool manifest runtime
  veyra-platform/  platform integration boundary
  veyra-import/    migration parsers for existing launcher profiles
  veyra-protocol/  external plugin JSON-RPC schemas
docs/
```

## Profile Layout

Config is loaded from:

- Windows: `%APPDATA%\Veyra`
- Linux: `~/.config/veyra`
- Portable: `./portable/` beside the executable

Supported profile files (all optional):

- `config.toml` - startup, hotkeys, appearance
- `commands.toml` - commands and web search entries
- `plugins.toml` - local plugin/process bridge entries
- `tools/*.json` - optional tool manifests
- `catalogs.toml` - file catalog profiles
- `ai.toml` - AI section and provider config
- `history.json` - launch history used for local ranking boosts
- `ai-chat-log.jsonl` - append-only AI request/response log
- `ai-chats/` - saved Markdown AI chat snapshots
- `platform_catalog_cache.json` - generated cache of the platform catalog scan (1-hour TTL; safe to delete)

The settings UI can open and create the five TOML files (`config.toml`, `commands.toml`, `plugins.toml`, `catalogs.toml`, `ai.toml`) from built-in templates. The AI chat log and saved chat snapshots are openable from the AI panel and the Diagnostics page.

## Catalog Sources at Startup

- Veyra scans startup sources and appends discovered items to the catalog:
  - PATH executables (environment-expanded)
  - Windows Start Menu shortcuts (`.lnk`)
  - Windows App Paths registry entries (`HKLM`/`HKCU`)
  - Program Files and Windows Apps
  - Desktop shortcuts
  - enabled file catalog profiles from `catalogs.toml` or imported `commands.toml`
- On Windows, executables are filtered by `PATHEXT` (or default suffixes); on Linux, files must be executable.
- The Windows platform scan is cached in `platform_catalog_cache.json` (1-hour TTL); `Refresh catalogs` invalidates the cache.
- Items are deduplicated by normalized executable name and canonical path; first match wins.
- File catalogs honor `recursive`, `max_depth`, `include_patterns`, `exclude_patterns`, and `follow_symlinks`.
- A built-in seed catalog adds Windows system items (Notepad, Calculator, Paint, Snipping Tool, File Explorer, CMD, PowerShell, Windows Terminal, Control Panel), `ms-settings:` deep links, Documents/Downloads/Desktop folders, and Google/DuckDuckGo/Bing web searches with `{query}` substitution.

Catalogs are rebuilt from the same startup scan path when profile reload is requested:

- `Ctrl+R` in the launcher.
- `Reload profile` on the General or Diagnostics settings pages.
- `Refresh catalogs` on the Catalogs settings page.

The launcher registers `Win+Shift+F23` for Copilot-key keyboards and keeps `Alt+Space` as a fallback global toggle. On Windows, Veyra also installs a low-level Copilot-key hook while it is running so the shell does not take the key first. GUI builds use the Windows subsystem, and spawned commands are started without a console window. On Windows, Veyra applies a native DWM backdrop (acrylic/glass), rounded corners, and immersive dark mode where supported.

## Run

```powershell
cargo run -p veyra-app
```

## Import Existing Profile

```powershell
cargo run -p veyra-import -- keypirinha --source C:\Tools\Keypirinha --dry-run
cargo run -p veyra-import -- keypirinha --source C:\Tools\Keypirinha --profile "$env:APPDATA\Veyra" --force
```

## Quick Commands

- `snippet <keyword>` / `paste <keyword>` - insert a saved snippet (from the `snippets` config section) or a built-in snippet keyword.
- `aurora <cmd>` / `wp <cmd>` - control the Aurora wallpaper engine over its named pipe (`\\.\pipe\aurora`): next, prev, quit, status, stats, and reload-config aliases.
- `komorebi <cmd>` / `kb <cmd>` / `wm <cmd>` - drive `komorebic.exe` for window-manager control (start, stop, pause, float, monocle, max, retile, layout, focus, move, workspace, promote, minimize, close, lock, swap).
- `admin <query>` / `sudo <query>` / `elevate <query>` - run the matched item elevated.

## Next-Step Usage

1. Edit files under the active profile (or `portable/`).
2. Run `cargo run -p veyra-app`.
3. Open launcher with the Copilot key or `Alt+Space`, search profile items, then run with `Enter`.
4. Ask AI with prefixes such as `ai`, `ask`, `chat`, `kyrphina`, `llama`, `pike`, `pylon`, `npu`, `minicpm`, or `minicpm5`. Veyra captures the answer in-place; follow-ups use the same provider for the session.
5. Open settings (`Ctrl+,`) to:
   - open the profile folder,
   - open or create `config.toml`, `commands.toml`, `plugins.toml`, `catalogs.toml`, and `ai.toml`,
   - reload catalog data after edits.

## AI Providers and Tools

- Provider kinds: `open_ai_compatible` (HTTP) and `process` (local executable).
- Process providers support `command`, `args`, `env`, `prompt_template`, `stop_tokens`, `ready_marker`, `turn_marker`, `context_overflow_markers`, and `context_limit_tokens`.
- Placeholders in process args: `{prompt}`, `{prompt_file}`, `{chatml_prompt}`, `{chatml_prompt_file}`. If no placeholder is present, Veyra appends `--prompt-file <chatml-file>`. Prompt files are written to the system temp directory and cleaned up after execution.
- Warm process providers (`keep_warm = true`) are prewarmed at startup and reused across prompts; prompts are sent over stdin with a `\END` terminator.
- When a provider reports a context-exceeded error, Veyra trims the conversation history and retries.
- Recognized AI tool calls: `open_result`, `search`, `open_url`, `copy_to_clipboard`, `calculate`, `current_time` (plus aliases). See `docs/AI_AND_TOOLS.md` for the full contract.

## Check

```powershell
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
```

## Package

```powershell
.\scripts\package-release.ps1 -Targets windows-x64,windows-arm64
```

```bash
bash scripts/package-release.sh linux-x64 linux-arm64
```

The GitHub release workflow packages Windows x64, Windows ARM64, Linux x64, and Linux ARM64 archives with SHA-256 checksum files. Release artifacts under `dist/` and runtime caches under `data/` are gitignored.

## Scripts

- `scripts/package-release.ps1` / `scripts/package-release.sh` - release packagers (see Package).
- `scripts/sample-json-rpc-plugin.py` - reference JSON-RPC stdio plugin.
- `scripts/veyra-kyrphina.ps1` - Kyrphina chat-panel bridge plugin (Chat, Settings, Doctor, Install, StartLlama, StartGenie modes).
- `scripts/veyra-pike.ps1` - Pike coding-agent bridge plugin.
- `scripts/hippo-veyra-bridge.py` - JSON-RPC stdio plugin for Hippo memory search (see `docs/HIPPO_SILT_INTEGRATION.md`).
- `scripts/nonorganize-veyra-bridge.py` - JSON-RPC stdio plugin for Nonorganize file search (see `docs/NONORGANIZE_INTEGRATION.md`; requires a buildable Nonorganize on the target platform).

## Documentation

- `docs/SPEC.md` - product and technical specification
- `docs/ARCHITECTURE.md` - system design and module boundaries
- `docs/AI_AND_TOOLS.md` - AI provider and tool routing contract
- `docs/MIGRATION.md` - migration plan from an existing launcher profile
- `docs/CONFIG_EXAMPLES.md` - TOML config examples
- `docs/BUILD_MATRIX.md` - target platforms and cross-compile requirements
- `docs/ROADMAP.md` - phased implementation plan
- `docs/HIPPO_SILT_INTEGRATION.md` - Hippo/Silt memory integration plan
- `docs/NONORGANIZE_INTEGRATION.md` - Nonorganize file-search integration plan
