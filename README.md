# Veyra

Veyra is a native keyboard launcher and AI command surface for Windows and Linux. It is designed to be fast like Keypirinha, discoverable like Flow Launcher, and portable across Windows/Linux x64 and ARM64 without Electron.

Current status: early workspace skeleton with a working profile loader, command/web-search/catalog import, and startup catalog indexing.

## Goals

- Native Rust app with a polished command palette.
- Cross-compilable for Windows ARM64, Windows x64, Linux ARM64, and Linux x64.
- Local-first AI provider and tool routing.
- Built-in settings, system tools, web search, file catalog profiles, and command migration.
- Acrylic/glass visual direction on Windows where supported.
- External plugin protocol over JSON-RPC stdio.

## Workspace

```text
crates/
  veyra-app/       GUI executable
  veyra-core/      catalog, scoring, config, actions
  veyra-ai/        providers and tool manifest schemas
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
- `catalogs.toml` - file catalog profiles
- `ai.toml` - AI section and provider config

## Catalog Sources at Startup

- Veyra scans startup sources and appends discovered items to the catalog:
  - PATH executables
  - Windows Start Menu shortcuts (`.lnk`)
  - enabled file catalog profiles from `catalogs.toml` or imported `commands.toml`
- On Windows, executables are filtered by `PATHEXT` (or default suffixes); on Linux/macOS, files must be executable.
- Items are deduplicated by normalized executable name and canonical path; first match wins.
- File catalogs honor `recursive`, `max_depth`, `include_patterns`, `exclude_patterns`, and `follow_symlinks`.

## Run

```powershell
cargo run -p veyra-app
```

## Import Existing Profile

```powershell
cargo run -p veyra-import -- keypirinha --source C:\Tools\Keypirinha --dry-run
cargo run -p veyra-import -- keypirinha --source C:\Tools\Keypirinha --profile "$env:APPDATA\Veyra" --force
```

## Next-Step Usage

1. Edit files under the active profile (or `portable/`).
2. Run `cargo run -p veyra-app`.
3. Open launcher with `Alt+Space`, search profile items, then run with `Enter`.
4. Open settings (`Ctrl+,`) and review `Diagnostics` for loaded/missing profile files.

## Check

```powershell
cargo fmt --all --check
cargo test --workspace
cargo check --workspace
```

## Documentation

- `docs/SPEC.md` - product and technical specification
- `docs/ARCHITECTURE.md` - system design and module boundaries
- `docs/AI_AND_TOOLS.md` - AI provider and tool routing contract
- `docs/MIGRATION.md` - migration plan from an existing launcher profile
- `docs/CONFIG_EXAMPLES.md` - TOML config examples
- `docs/BUILD_MATRIX.md` - target platforms and cross-compile requirements
- `docs/ROADMAP.md` - phased implementation plan
