# Architecture

## High-Level Shape

```text
+---------------------------------------------------------------+
| UI Shell                                                      |
| command palette, settings, previews, notifications            |
+-------------------------------+-------------------------------+
                                |
+-------------------------------v-------------------------------+
| Launcher Core                                                 |
| query state, scoring, actions, history, config, events         |
+-----------+-------------------+--------------------+----------+
            |                   |                    |
+-----------v------+   +--------v-------+   +--------v---------+
| Catalogs         |   | Actions        |   | AI Router        |
| apps/files       |   | launch/run     |   | providers/tools  |
+-----------+------+   +--------+-------+   +--------+---------+
            |                   |                    |
+-----------v-------------------v--------------------v----------+
| Plugin Host                                                   |
| process bridge plugins, tool manifests, JSON-RPC stdio plugins|
+---------------------------------------------------------------+
```

## Crates

```text
crates/
  veyra-app/       GUI executable
  veyra-core/      catalog, scoring, config, actions
  veyra-ai/        providers, tool schemas, chat protocol
  veyra-platform/  Windows/Linux platform integration
  veyra-plugin/    process plugins, tool manifests, JSON-RPC stdio host
  veyra-import/    migration parsers and import helpers
  veyra-protocol/  external plugin JSON-RPC schemas
```

## Boundaries

- `veyra-app` owns GUI rendering and user input.
- `veyra-app::windowing` owns app-window sizing, centering, DPI awareness, and native backdrop hooks.
- `veyra-app::hotkeys` owns global toggle registration and the Windows Copilot-key hook.
- `veyra-app::ai_transport` owns OpenAI-compatible HTTP calls, local process AI calls, and warm NPU process lifecycle.
- `veyra-app::ai_logging` owns AI JSONL chat logs and markdown session snapshots.
- `veyra-app::ai_tools` owns parsed AI tool-call XML, answer display cleanup, and tool-call parameter helpers.
- `veyra-app::ai_prompt` owns model prompt formatting, tool context rendering, and conversation-context policy.
- `veyra-app::history` owns launch-history persistence, recent-result projection, and history score boosts.
- `veyra-core` owns catalog items, actions, scoring, config, and history.
- `veyra-ai` owns provider settings, tool manifests, and AI execution contracts.
- `veyra-platform` owns OS-specific integration behind a portable boundary.
- `veyra-plugin` owns local trusted plugin catalog loading and JSON-RPC stdio execution.
- `veyra-import` owns compatibility parsing for existing launcher profiles.
- `veyra-protocol` owns schemas shared with external plugins.

The UI must not block on catalog scans, AI calls, icon extraction, or plugin calls.

Current plugin execution is local trusted: `process` entries become catalog items that launch local commands/scripts with environment expansion and optional `{query}` substitution, while `json_rpc_stdio` entries are queried through the shared protocol for catalog, suggest, and execute calls. Tool manifest JSON files under the active profile also become Tool catalog items.

## Failure Handling

- Plugin failures become warnings.
- Catalog errors do not crash the app.
- AI provider failures produce visible result items and diagnostics.
- Bad config is reported with file and line context where possible.
- The launcher must still open when AI is broken.
