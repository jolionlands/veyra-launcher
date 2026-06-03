# Migration Plan

## Purpose

Veyra imports legacy launcher profiles into Veyra TOML files so users can keep their existing command setup with minimal manual work.

## Source Profile

Expected source root:

```text
<keypirinha-portable-root>
```

## Importer Direction

- Direction is Keypirinha profile -> Veyra profile.
- `Profile\User\Apps.ini` and `cmd/<name>` sections map to Veyra `commands`.
- `Profile\User\WebSearch.ini` and `site/<alias>` sections map to Veyra `web_search`.
- `Profile\User\FilesCatalog.ini` and `profile/<name>` or `catalog/<name>` sections map to Veyra `catalogs`.
- AskAI tools and other package assets are not migrated yet.

## Import Rules

1. Preserve command labels and launch commands.
2. Parse `auto_terminal`/`terminal` into Veyra `terminal`.
3. Normalize web search URL tokens to `{query}`.
4. Convert catalog paths and simple `+`/`-` filters into Veyra catalog profiles.
5. Emit only transformed TOML content; never modify the source profile.

## Import Command

```powershell
cargo run -p veyra-import -- keypirinha --source <keypirinha-portable-root> --dry-run
cargo run -p veyra-import -- keypirinha --source <keypirinha-portable-root> --profile <veyra-profile-dir> --force
```

Supported flags:

- `--source <path>`: Keypirinha portable root or folder containing `Apps.ini`/`WebSearch.ini`/`FilesCatalog.ini`.
- `--profile <path>`: Veyra profile directory; defaults to current profile directory and writes `commands.toml`.
- `--output <path>`: exact output file path.
- `--dry-run`: print generated TOML without writing.
- `--force`: overwrite an existing output file.

## Import Outputs

Current Veyra outputs:

- `commands.toml` (`commands` + `web_search` + importer-emitted `catalogs`)

Future Veyra outputs:

- `ai.toml` + tool manifests (future)
