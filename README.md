# Veyra

Veyra is a native keyboard launcher and AI command surface for Windows and Linux. It is designed to be fast like Keypirinha, discoverable like Flow Launcher, and portable across Windows/Linux x64 and ARM64 without Electron.

Current status: early workspace skeleton.

## Goals

- Native Rust app with a polished command palette.
- Cross-compilable for Windows ARM64, Windows x64, Linux ARM64, and Linux x64.
- Local-first AI provider and tool routing.
- Built-in settings, system tools, web search, file catalogs, and command migration.
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

## Run

```powershell
cargo run -p veyra-app
```

## Check

```powershell
cargo fmt --check
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
