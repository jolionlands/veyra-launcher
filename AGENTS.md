# Veyra Agent Guide

This file is written for AI coding agents working on the Veyra project. All documentation, comments, and source identifiers in the repository are in English; write agent-facing output in English.

## Project overview

Veyra is a native keyboard launcher and AI command surface for Windows and Linux, written in Rust. It is designed to be fast like Keypirinha, discoverable like Flow Launcher, and portable across Windows/Linux x64 and ARM64 without Electron.

Current status: working native launcher with profile loading, command/web-search/catalog import, startup catalog indexing, indexed search, launch history, settings pages, local trusted plugin/tool loading, and captured AI answers/actions.

The project is a Cargo workspace with seven crates. The only GUI executable is `veyra-app`; the other crates are libraries that define the core data model, platform integration, plugin protocol, AI contracts, and import tooling.

## Repository layout

```text
Cargo.toml                 workspace definition
Cargo.lock                 pinned dependency tree
crates/
  veyra-app/               GUI executable (eframe/egui)
  veyra-core/              catalog, scoring, config, actions
  veyra-ai/                providers, tool schemas, chat protocol
  veyra-platform/          Windows/Linux platform integration
  veyra-plugin/            process plugins, tool manifests, JSON-RPC stdio host
  veyra-import/            migration parsers and import helpers
  veyra-protocol/          external plugin JSON-RPC schemas
docs/
  SPEC.md                  product and technical specification
  ARCHITECTURE.md          system design and module boundaries
  AI_AND_TOOLS.md          AI provider and tool routing contract
  MIGRATION.md             migration plan from existing launcher profiles
  CONFIG_EXAMPLES.md       TOML config examples
  BUILD_MATRIX.md          target platforms and cross-compile requirements
  ROADMAP.md               phased implementation plan
scripts/
  package-release.ps1      Windows release packager
  package-release.sh       Linux release packager
  sample-json-rpc-plugin.py  example JSON-RPC stdio plugin
  veyra-kyrphina.ps1       Kyrphina chat-panel bridge plugin
  veyra-pike.ps1           Pike coding-agent bridge plugin
.github/workflows/
  ci.yml                   push/PR CI (fmt, test, clippy, cross-platform build)
  release.yml              manual workflow_dispatch release packager
data/                      runtime SQLite caches/logs (not source)
dist/                      release artifacts
```

## Technology stack

- **Language:** Rust (workspace `rust-version = "1.95"`, `edition = "2024"`).
- **GUI:** `eframe` / `egui` (`0.34.3`).
- **Serialization:** `serde`, `serde_json`, `toml`, `toml_edit`.
- **HTTP:** `reqwest` with blocking JSON support.
- **Global hotkeys:** `global-hotkey` plus a Windows low-level keyboard hook for the Copilot key.
- **Native windowing:** `raw-window-handle` and `windows-sys` for DWM backdrops, DPI awareness, and window placement on Windows.
- **Error handling:** `thiserror` is used for library errors.

There is no `tokio` or async runtime at this time; background work is done with `std::thread` and `std::sync::mpsc`.

## Build and test commands

Run the app locally:

```powershell
cargo run -p veyra-app
```

Check the workspace:

```powershell
cargo check --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The CI job in `.github/workflows/ci.yml` runs exactly these steps before building each target.

Cross-compiled release builds:

```powershell
# Windows targets
rustup target add aarch64-pc-windows-msvc x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc -p veyra-app
cargo build --release --target aarch64-pc-windows-msvc -p veyra-app

# Linux targets (from a Linux host or container)
rustup target add aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu -p veyra-app
cargo build --release --target aarch64-unknown-linux-gnu -p veyra-app
```

Package releases locally:

```powershell
.\scripts\package-release.ps1 -Targets windows-x64,windows-arm64
```

```bash
bash scripts/package-release.sh linux-x64 linux-arm64
```

## Code style guidelines

- Follow the existing Rust style. `cargo fmt` is enforced in CI; run it before committing.
- Clippy is run with `-D warnings` in CI; do not introduce new warnings.
- Prefer `snake_case` for functions/variables and `PascalCase` for types, matching the existing codebase.
- Prefer explicit module imports; `main.rs` uses a mix of `mod` declarations and `use` for crate-local modules.
- Keep library code panic-free on bad user input. Use `Result` and `thiserror`/`Result` idioms.
- Platform-specific code lives in `veyra-platform` and behind `#[cfg(windows)]` / `#[cfg(not(windows))]` in `veyra-app::windowing` and `veyra-app::hotkeys`.
- Avoid adding unnecessary dependencies. Prefer small, focused crates.

## Testing instructions

- Unit tests live in `#[cfg(test)]` modules at the bottom of source files.
- Run the full workspace test suite with `cargo test --workspace`.
- When adding new config parsing, scoring, catalog discovery, or plugin protocol behavior, add a corresponding unit test.
- Tests may write temporary files under `std::env::temp_dir()`; they clean up with `fs::remove_dir_all(...).ok()`.
- There are no integration tests yet; the app is exercised manually by running `cargo run -p veyra-app`.

## Security considerations

- Veyra is a **local-trusted** launcher. Plugins, tools, and AI providers are configured by the user and run with the user's privileges.
- `Action::run_as_admin` exists in the data model but is **not wired through** the platform executor yet.
- Guarded actions (`requires_confirmation = true`) require `Shift+Enter` or an explicit UI `Run` confirmation. Tool manifests with safety levels `write`, `execute`, `admin`, or `network` automatically require confirmation.
- Process plugin commands and args support environment-variable expansion (`%VAR%`, `$VAR`, `${VAR}`, `~`) and `{query}` substitution. Do not run untrusted plugin configs.
- AI process providers may write prompt files to the system temp directory. These files contain the user's prompt and are cleaned up after execution.
- The `local_only` flag (general, `[ai]`, or per-provider) blocks non-local HTTP AI endpoints. Local process providers are always allowed.
- The Windows low-level Copilot-key hook (`hotkeys.rs`) suppresses the chord so the shell does not take it first; it is only active while Veyra is running.

## Workspace and module divisions

| Crate | Responsibility | Key public surface |
|-------|----------------|--------------------|
| `veyra-core` | Catalog items, search/scoring, config structs, actions | `CatalogItem`, `Action`, `SearchIndex`, `VeyraConfig` |
| `veyra-platform` | OS-specific catalog discovery and action execution | `profile_dir`, `discover_platform_catalog_items`, `discover_file_catalog_items`, `execute_action` |
| `veyra-ai` | AI provider config, tool manifest schema/validation | `ToolManifest`, `load_tool_manifests_from_directory` |
| `veyra-protocol` | JSON-RPC stdio schemas for external plugins | `JsonRpcRequest`, `JsonRpcResponse`, protocol method structs |
| `veyra-plugin` | Process plugins, JSON-RPC stdio host, tool manifest loading | `process_plugin_item`, `load_plugin_extensions`, `load_plugin_suggestions`, `execute_json_rpc_action` |
| `veyra-import` | Keypirinha profile migration | `import_keypirinha_profile`, `ImportedProfile` |
| `veyra-app` | GUI executable, settings UI, runtime orchestration, AI transport, logging, history | binary crate |

`veyra-app` is the only binary crate and depends on all library crates except `veyra-import` (which is its own CLI tool). `veyra-import` depends on `veyra-core` and `veyra-platform`.

## Runtime architecture

- `VeyraApp` holds the launcher state: query, catalog, search index, settings visibility, history, AI session, and plugin suggestion state.
- Profile loading and catalog indexing happen on background threads and send `RuntimeUpdate` messages back to the UI thread.
- AI calls and plugin suggestion calls run on background threads and send results through `mpsc` channels.
- The UI must not block on catalog scans, AI calls, icon extraction, or plugin calls.
- Failure handling:
  - Plugin failures become warnings/diagnostics.
  - Catalog errors do not crash the app.
  - AI provider failures produce visible result items and diagnostics.
  - Bad config is reported with file context where possible.
  - The launcher must still open when AI is broken.

## Profile and configuration conventions

Config is loaded from:

- Windows: `%APPDATA%\Veyra`
- Linux: `~/.config/veyra`
- Portable: `./portable/` beside the executable

Supported profile files:

- `config.toml` — startup, hotkeys, appearance
- `commands.toml` — commands and web search entries
- `plugins.toml` — local plugin/process bridge entries
- `tools/*.json` — optional tool manifests
- `catalogs.toml` — file catalog profiles (also accepts `[[profiles]]` alias)
- `ai.toml` — AI section and provider config
- `history.json` — launch history used for local ranking boosts
- `ai-chat-log.jsonl` — append-only AI request/response log
- `ai-chats/` — saved Markdown AI chat snapshots

Merge order:

1. `config.toml` (full overwrite for general/hotkeys/appearance)
2. `commands.toml` (append commands, web searches, importer-emitted catalog profiles)
3. `plugins.toml` (append local process and JSON-RPC stdio plugin entries)
4. `catalogs.toml` (append catalog profiles)
5. `ai.toml` (AI settings; provider tables can be `[[ai.providers]]` or `[[providers]]`)

The settings UI can open these files in-place and create missing ones from built-in templates.

## AI and tool conventions

- AI prefixes route queries: `ai`, `ask`, `chat`, `llama`, `pike`, `pylon`, `npu`, `minicpm5`.
- Deterministic launcher results rank above AI guesses unless the user explicitly uses an AI prefix.
- Provider kinds: `open_ai_compatible` (HTTP), `process` (local executable).
- Process providers support placeholders in `args`: `{prompt}`, `{prompt_file}`, `{chatml_prompt}`, `{chatml_prompt_file}`. If no placeholder is present, Veyra appends `--prompt-file <chatml-file>`.
- Warm process providers (`keep_warm = true`) reuse a long-running process; prompts are sent over stdin with a `\END` terminator.
- Current recognized tool calls: `open_result`, `search`, `open_url`, `copy_to_clipboard`, `calculate`, `current_time`.
- Tool manifest JSON files under `tools/` and `plugins/` become Tool catalog items. Safety levels are `read`, `write`, `execute`, `admin`, `network`.

## Plugin protocol

External plugins speak JSON-RPC 2.0 over newline-delimited stdin/stdout. Required methods:

- `initialize` — returns plugin id/label/capabilities.
- `catalog` — returns static catalog items.
- `suggest` — returns dynamic items for the current query.
- `execute` — runs an action referenced by item and action id.
- `shutdown` — signals a clean exit.

See `crates/veyra-protocol/src/lib.rs` for the exact request/response structs and `scripts/sample-json-rpc-plugin.py` for a reference implementation.

## CI/CD and releases

- `.github/workflows/ci.yml` runs on every push and pull request: format check, tests, clippy, and cross-platform release builds.
- `.github/workflows/release.yml` is a manual `workflow_dispatch` workflow that packages Windows x64, Windows ARM64, Linux x64, and Linux ARM64 archives with SHA-256 checksum files.
- ARM64 runners use GitHub-hosted labels (`windows-11-arm`, `ubuntu-24.04-arm`).

## Common development tasks

Import an existing Keypirinha profile:

```powershell
cargo run -p veyra-import -- keypirinha --source C:\Tools\Keypirinha --dry-run
cargo run -p veyra-import -- keypirinha --source C:\Tools\Keypirinha --profile "$env:APPDATA\Veyra" --force
```

Add a new crate to the workspace:

1. Add the crate path to the `members` list in the root `Cargo.toml`.
2. Add any shared dependencies to `[workspace.dependencies]` if they are used in multiple crates.
3. Use `path = "../<crate>"` for internal dependencies.

Adding a new config section:

1. Add the struct and defaults to `crates/veyra-core/src/config.rs`.
2. Add a unit test for parsing.
3. Update the built-in template in `veyra-app` if the settings UI should be able to create the file.
4. Update `docs/CONFIG_EXAMPLES.md`.

## What not to do

- Do not introduce Electron or a web-based UI.
- Do not make cloud AI mandatory for normal launcher use.
- Do not block startup on model loading.
- Do not tie core search, scoring, config, or plugins to Windows-only APIs.
- Do not change git history or run `git commit`/`git push` unless explicitly asked.
