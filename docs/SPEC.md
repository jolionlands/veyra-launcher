# Veyra Specification

## Purpose

Veyra is a fast native launcher and AI command surface for Windows and Linux. It should preserve the useful launcher workflow from existing tools while adding a modern settings surface, local-first AI, tool routing, and cross-architecture builds.

## Product Goals

- Open instantly from a global hotkey.
- Search apps, files, commands, settings, web shortcuts, history, and AI tools.
- Support natural language commands such as `open display settings`, `repair komorebi bar`, or `ask summarize this`.
- Run without cloud AI.
- Support OpenAI-compatible local servers and optional remote providers.
- Provide a settings UI with Windows Settings-like density and navigation.
- Keep config editable as TOML.
- Keep the core portable across Windows and Linux.

## Non-Goals

- Do not clone another launcher's internals.
- Do not require Electron.
- Do not require a cloud AI provider for normal launcher use.
- Do not block startup on model loading.
- Do not require admin privileges for normal launcher operation.
- Do not tie core search, scoring, config, or plugins to Windows-only APIs.

## Target Platforms

- Primary: Windows 11 ARM64
- Primary: Windows 11 x64
- Secondary: Linux x64
- Secondary: Linux ARM64

## Recommended Stack

- Language: Rust
- GUI: `eframe`/`egui`
- Async runtime: `tokio` once background work starts
- HTTP client: `reqwest` once provider calls start
- Serialization: `serde`
- Config: TOML under a user profile directory
- Plugin protocol: JSON-RPC over stdio
- Built-in plugins: native Rust modules

## User Experience

The primary surface is a centered command palette:

- Search input at top.
- Result list below.
- Optional preview panel.
- Optional bottom action bar.
- Acrylic/glass visual direction on Windows.
- Dark theme by default.
- Keyboard-first navigation.
- Mouse support.

Expected defaults:

- `Alt+Space`: show/hide launcher.
- `Enter`: run selected item.
- `Shift+Enter`: run alternate action.
- `Ctrl+Enter`: run as admin where supported.
- `Tab`: accept completion or enter action mode.
- `Esc`: clear query, then close.
- `Ctrl+,`: settings.

## Settings UI

Settings are built into the app:

- General
- Appearance
- Hotkeys
- Catalogs
- Commands
- AI Providers
- Tools
- Plugins
- Privacy
- Diagnostics
- About

Every setting must have a config-file equivalent.

## MVP

The first useful MVP is complete when it can:

- Open a native launcher window.
- Search seeded commands using local fuzzy scoring.
- Search imported user commands, Start Menu apps, PATH executables, and web shortcuts.
- Launch selected results.
- Show a polished dark/acrylic-style UI.
- Expose an AI query command using an OpenAI-compatible local endpoint.
- Build on Windows ARM64 and Windows x64.
- Document Linux build targets.

