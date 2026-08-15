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

## Technology Stack

Implemented state (Cargo workspace, `rust-version = "1.95"`, `edition = "2024"`):

- **Language:** Rust, seven crates (`veyra-app`, `veyra-core`, `veyra-platform`, `veyra-ai`, `veyra-plugin`, `veyra-import`, `veyra-protocol`).
- **GUI:** `eframe`/`egui` (`0.34.3`). The only binary crate is `veyra-app`.
- **Concurrency:** no async runtime. Background work uses `std::thread` and `std::sync::mpsc`; the AI transport is synchronous and blocking by design, orchestrated from threads in `main.rs`.
- **HTTP client:** `reqwest` (blocking) for the OpenAI-compatible AI transport.
- **Serialization:** `serde`, `serde_json`, `toml`, `toml_edit`.
- **Global hotkeys:** `global-hotkey` (`0.8`) plus a Windows low-level keyboard hook (`WH_KEYBOARD_LL`) for the Copilot key chord.
- **Native windowing:** `raw-window-handle` (`0.6`) and `windows-sys` (`0.59`) for DWM backdrops, per-monitor DPI awareness, and window placement on Windows.
- **Error handling:** `thiserror` for library errors.

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

### Window

- Borderless, transparent, always-on-top eframe window; initial size 680×76, minimum 360×64; size is fully program-controlled (not user-resizable).
- `#![cfg_attr(windows, windows_subsystem = "windows")]` — no console window on Windows.
- Windows visual stack: DWM backdrop (acrylic by default, mica for the `dark-compact` theme, none when blur is off) → transparent clear color → egui rounded semi-transparent surface (rgba 18,20,23, alpha ceiling 176 with blur / 242 without, scaled by `opacity` clamped 0.35–1.0) → 1px border. Corner radius 8px plus `DWMWCP_ROUNDSMALL`.
- Per-monitor DPI: `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` at startup; native measurement/positioning calls run inside a per-thread DPI context; effective layout scale is `max(egui pixels_per_point, native monitor scale)`.
- The launcher appears on the monitor where the cursor is (work-area aware, respects the taskbar), centered horizontally and pinned near the top (96px compact/results, 80px AI); Settings is vertically centered. An 8-frame (~128 ms) settle loop re-centers while egui's async resize settles.
- Layout modes: compact (680×76), results (90 + `count*58`, clamped 120–520), AI capture (68% of monitor height, clamped 320–560), settings (72%×72%, clamped 560–840 × 420–720).
- The window is draggable by grabbing the surface when not typing.

### Hotkeys

Expected defaults:

- `Win+Shift+F23`: show/hide launcher on Copilot-key keyboards (default configured toggle).
- `Alt+Space`: fallback show/hide launcher hotkey.
- Both chords are **always attempted** regardless of configuration: the candidate list is `[configured, "Win+Shift+F23", "Alt+Space"]`, deduplicated. Per-candidate registration failures are surfaced as UI messages and the loop continues.
- Windows builds install a `WH_KEYBOARD_LL` hook that catches `F23` while Win+Shift is held, **swallows the keystroke** (so the shell never sees it), and signals the toggle. The `global-hotkey` registration of the chord is a redundant belt-and-suspenders path.
- In-app egui detection catches `Shift+F23` / `Alt+Space` while the window has focus, but only for chords not already registered globally (avoids double-toggle).
- **Hide guard:** the launcher refuses to hide when no global toggle is registered, so it can never become unreachable.
- `Enter`: run selected item.
- `Shift+Enter`: confirm guarded launcher actions.
- Admin/elevated execution is planned; `run_as_admin` metadata exists but is not wired through the platform executor yet.
- `Tab`: accept completion or enter action mode.
- `Ctrl+,`: settings (in-app only; not registered globally).
- `Ctrl+R`: reload profile and rebuild catalog from profile files.
- `Esc`: clear the current query, leave settings, or hide the launcher when empty.

## Settings UI

Current settings pages (8):

- General
- Appearance
- Hotkeys
- Catalogs
- Commands
- AI Providers
- Tools
- Diagnostics

The Hotkeys page is a read-only display: configured toggle, Copilot key constant, fallback constant, live "Registered" labels (e.g. `Super+Shift+F23, Alt+Space, Copilot hook`), and the settings shortcut. Planned pages include Privacy and About. Every implemented setting has a config-file equivalent.

## Settings Operations

- General and Diagnostics views provide reload controls (`Reload profile`, `Reload`) to rebuild runtime state after editing profile files.
- Catalogs view provides `Refresh catalogs` to force a catalog re-scan without restarting.
- The settings UI can open:
  - the active profile folder,
  - `config.toml`,
  - `commands.toml`,
  - `plugins.toml`,
  - `catalogs.toml`,
  - `ai.toml`.
- Opening any profile file path from settings creates the file with a default template when it is missing.

## AI Transport

All AI execution lives in `crates/veyra-app/src/ai_transport.rs` (synchronous, blocking; orchestrated from background threads in `main.rs`). Two provider kinds exist (`AiProviderKind` in `veyra-core`):

- **`open_ai_compatible` (HTTP):** `reqwest` blocking client, timeout `max(timeout_ms, 1000)`, body `{model, messages: [system, user], temperature: 0.2, stream: false}`. Endpoint normalization appends `/v1/chat/completions` as needed. Bearer auth from `api_key_env` when set. The `local_only` gate rejects any non-local endpoint (localhost, `::1`, `0.0.0.0`, `127.*`). **Streaming is never used** — `supports_streaming` is UI metadata only. Answers are extracted tolerantly from `/choices/0/message/content`, `/choices/0/text`, or `/message/content`.
- **`process` (local executable):** command/args/env are env-var-expanded (`%VAR%`, `$VAR`, `${VAR}`, `~`). Placeholders `{prompt}`, `{prompt_file}`, `{chatml_prompt}`, `{chatml_prompt_file}` are substituted per-arg; with no placeholder, a ChatML prompt file is written and `--prompt-file <path>` appended. Temp prompt files are always cleaned up. Windows spawns use `CREATE_NO_WINDOW`.
  - **One-shot** (`keep_warm = false`): spawn, 25 ms poll, kill on timeout, stdout/stderr drained on reader threads.
  - **Warm** (`keep_warm = true`): process-global cache keyed by id/label/command (command+args signature invalidates on config change). Startup waits for a stderr `ready_marker` (default `"ready for prompts"`). Each turn writes the prompt + `\END\n` to stdin; completion is signaled by a stderr line containing the `turn_marker` (default `"[turn "`). Dead/timed-out sessions are evicted and respawned; `Drop` kills the child. This is the contract implemented by `npu_chat.exe` (MiniCPM5 NPU).
- **Prompt templating:** custom `prompt_template` (`{system}`/`{user}`/`{prompt}`) or the default ChatML wrapper with a MiniCPM-style ` thinking\n\n response\n\n` reasoning preamble.
- **Answer cleaning:** CRLF normalization, truncation at stop tokens (defaults `<|im_end|>`, `<|endoftext|>`, `</s>`), and stripping of the ` thinking`/` response` reasoning block.
- **Failure handling:** every path returns `Result<String, String>`; the launcher never crashes on AI failure. Errors become visible result items/diagnostics. Context-exceeded errors trigger one retry with a budget-trimmed prompt (orchestrated in `main.rs`).
- **Default providers** (from the built-in `ai.toml` template): `minicpm5_npu` (warm process, `npu_chat.exe` + Qualcomm Genie/QNN bundle, 120 s timeout, 16K context, default), `llama` (HTTP `127.0.0.1:8080/v1`, enabled), `pylon` (HTTP `127.0.0.1:8088/v1`, `PYLON_API_KEY`), `pike` (one-shot PowerShell bridge, 300 s timeout, tools).
- **AI prefixes:** `ai`, `ask`, `chat`, `kyrphina` route to the default provider; `llama`, `pike`, `pylon`, `npu`, `minicpm`, `minicpm5` route to the named provider.

## History

- Launch history is a **JSON aggregate** (`history.json` in the profile dir), not SQLite. `LaunchHistory` (in `crates/veyra-app/src/history.rs`) stores per-item entries `{item_id, label, source, launch_count, last_query, last_used_unix}`, capped at 5,000 entries (`general.history_limit`, default 5000).
- Ranking boost: `120 + min(launch_count, 20) * 35`, plus `+120` when the normalized query equals `last_query`, plus `+40` for App/Command/Tool categories, capped at 900.
- Recorded on every launch; the settings UI shows entry count and total launches and offers a "Clear history" action.
- The `data/` directory in the repo is **not** Veyra's runtime state — it contains pylon-lattice control-plane SQLite stores from a local dev run and is gitignored.

## Aurora IPC

`crates/veyra-platform/src/aurora.rs` is a synchronous named-pipe JSON client for the Aurora wallpaper engine: it sends one JSON request to `\\.\pipe\aurora` and reads the response until the server closes the pipe (one message per connection). It powers "aurora quick results" in search (`aurora_search_result` in `main.rs`). Windows-only.

## Build and Test State

- `cargo build --release -p veyra-app` succeeds with zero warnings; the Windows x64 release binary is ~14.6 MiB.
- Test suite: **118 unit tests + 1 ignored, all green** (`cargo test --workspace`, 119 total), covering config parsing, search/scoring, catalog discovery, hotkey normalization, AI provider selection/prompt building/tool-call parsing/response eval, chat log/snapshot writing, deterministic clock/calculator/timezone answers, window layout math, launch history, plugin→catalog items, Keypirinha import parsing, and tool-manifest validation. Tests are hermetic (temp dirs, no network).
- The one ignored test (`diagnostic_real_runtime_search`) is a manual perf/diagnostic tool against the real profile.
- Known gaps: no UI tests, no integration tests (JSON-RPC stdio host, HTTP AI transport, and action execution are untested end-to-end), and no tests for `action.rs`, the import CLI, or `aurora.rs`.
- CI (`.github/workflows/ci.yml`) enforces `cargo fmt --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, then cross-target release builds (Windows x64/ARM64, Linux x64/ARM64).

## Kyrphina Import Plan

Status: **plan only — no code modified.** The plan imports kyrphina's 516 skill manifests + 516 Python runners into Veyra's tool-manifest/plugin surface with no new crate:

- **Schema mapping:** kyrphina manifest → Veyra `ToolManifest`. 496 v1 manifests pass through; 20 v2 multi-function manifests expand to one manifest per function (92 functions) → **588 generated manifests**. `safety`, `timeout_ms`, and `runner` are synthesized (safety via a keyword classifier with a user-editable `kyrphina-safety-overrides.json` escape hatch; `timeout_ms` 30 s; runner per class).
- **Runner reality:** only 92 of 516 runners are harness-compatible (`run(**params) -> dict`); 424 are CLI-style mocks (`main()` parsing `--k=v`, most returning placeholder results) and 20 are v2 CLI dispatchers. The import must classify runners (classes A/B/C) and must not claim 516 working tools.
- **Execution:** a generated `kyrphina_bridge.py` JSON-RPC stdio plugin (registered in `plugins.toml` as `json_rpc_stdio`) serves the generated manifests and shells out to the kyrphina harness per call (subprocess isolation, 30 s timeout, mock results passed through honestly). Process manifests in `tools/*.json` are the fallback surface.
- **Protocol gap:** `ExecuteParams` needs a `params: Value` field and `PROTOCOL_VERSION` bump to 2 (currently v1, no params field) so AI tool-call params reach the bridge; plus a fallback arm in `veyra-app`'s `resolve_ai_tool_suggestion` so unknown AI tool names route to imported tools.
- **Effort:** ~4.5–6 engineer-days. Docs updates planned for `MIGRATION.md`, `AI_AND_TOOLS.md`, `ROADMAP.md`.

## MVP

The first useful MVP is complete when it can:

- Open a native launcher window. ✅
- Search seeded commands using local fuzzy scoring. ✅
- Search imported user commands, Start Menu apps, PATH executables, and web shortcuts. ✅
- Launch selected results. ✅
- Show a polished dark/acrylic-style UI. ✅
- Expose captured AI answers through local process providers and OpenAI-compatible endpoints. ✅
- Capture supported AI tool/action intents and ask for confirmation before execution. ✅
- Build on Windows ARM64 and Windows x64. ✅
- Document Linux build targets. ✅

Remaining work beyond the MVP baseline: wiring `run_as_admin` through the platform executor, the kyrphina skills import (see above), and UI/integration test coverage.
