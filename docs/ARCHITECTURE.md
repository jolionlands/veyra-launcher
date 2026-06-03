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
| built-in plugins + external JSON-RPC stdio plugins            |
+---------------------------------------------------------------+
```

## Crates

```text
crates/
  veyra-app/       GUI executable
  veyra-core/      catalog, scoring, config, actions
  veyra-ai/        providers, tool schemas, chat protocol
  veyra-platform/  Windows/Linux platform integration
  veyra-protocol/  external plugin JSON-RPC schemas
```

## Boundaries

- `veyra-app` owns GUI rendering and user input.
- `veyra-core` owns catalog items, actions, scoring, config, and history.
- `veyra-ai` owns provider settings, tool manifests, and AI execution contracts.
- `veyra-platform` owns OS-specific integration behind a portable boundary.
- `veyra-protocol` owns schemas shared with external plugins.

The UI must not block on catalog scans, AI calls, icon extraction, or plugin calls.

## Failure Handling

- Plugin failures become warnings.
- Catalog errors do not crash the app.
- AI provider failures produce visible result items and diagnostics.
- Bad config is reported with file and line context where possible.
- The launcher must still open when AI is broken.

