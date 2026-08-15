# Roadmap

## Current Status Snapshot

- Launcher MVP is usable: hotkeys, app/file/command/web catalogs, history ranking, settings, profile reload, and local trusted plugins are in place.
- AI capture is usable: local process providers, OpenAI-compatible local endpoints, MiniCPM5 NPU defaults, context display, chat logs, snapshots, deterministic clock/calculator answers, and confirmed action cards are implemented.
- Plugin/tool support is local trusted: process plugins, JSON-RPC stdio catalog/suggest/execute, and profile tool manifests are implemented.
- Known gaps: elevated/admin execution, first-class memory modes, robust XML/event parsing for model tool calls, fuller tool management plus Privacy/About settings pages, icon extraction/cache, continued `veyra-app` module extraction, and Linux desktop integration polish.

## Phase 0 - Skeleton

- Workspace skeleton.
- Core item/action data model.
- Local fuzzy search.
- Native app shell.
- Public repository.

## Phase 1 - Launcher MVP

- Global hotkey on Windows.
- User command catalog.
- PATH executable catalog.
- Start Menu app catalog.
- Launch selected result.
- Basic history.
- Theme config.

## Phase 2 - Migration

- Import existing custom commands.
- Import web search aliases.
- Import file catalog profiles.
- Add Windows Settings provider.
- Add system tools provider.
- Add scripts provider.

## Phase 3 - AI Core

- AI provider config.
- OpenAI-compatible local endpoint support.
- Ask command.
- Captured in-launcher responses.
- Local process provider with warm MiniCPM5 NPU runner.
- Tool manifest parser and profile manifest loading.
- Tool/action suggestions with confirmation.
- Offline/local-only mode.
- Streaming responses.
- Native/schema tool calling.
- Memory recall/remember modes.

## Phase 4 - Settings UI

- General page.
- Appearance page.
- Hotkeys page.
- Catalogs page.
- Commands page.
- AI providers page.
- Plugins/tools page.
- Diagnostics page.

## Phase 5 - Linux Support

- Linux app discovery via `.desktop` files.
- Linux launcher hotkey strategy.
- Linux file/catalog scanning.
- Linux process launching.
- Linux packages.

## Phase 6 - Plugin Ecosystem

- JSON-RPC stdio plugin host for local trusted catalog, suggest, and execute calls.
- Tool manifest loading from profile directories.
- Local trusted plugin permission model with confirmation for guarded actions.
- Sample Python plugin.
- Sample Rust plugin.
- Plugin diagnostics.

## Phase 7 - Packaging

- Acrylic/blur on Windows.
- Icon extraction/cache.
- Portable packages.
- Installer candidates.
- Auto-update design.
