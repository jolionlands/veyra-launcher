# Migration Plan

## Purpose

Veyra should be able to import an existing launcher profile so the user does not need to recreate every command manually.

## Source Profile

Expected source root:

```text
<keypirinha-portable-root>
```

Important files:

| Source | Destination |
| --- | --- |
| `Profile\User\Apps.ini` | `commands.toml` and built-in command catalog |
| `Profile\User\WebSearch.ini` | `commands.toml` web search entries |
| `Profile\User\FilesCatalog.ini` | `catalogs.toml` file catalog profiles |
| `Profile\User\AskAI.ini` | `ai.toml` defaults |
| `Local\Packages\AskAI\skills` | `tools/askai` migrated tool package |

## Import Rules

- Preserve command labels.
- Preserve launch commands.
- Map `auto_terminal = no` to `terminal = false`.
- Map terminal defaults conservatively for scripts and console commands.
- Convert web search aliases into `{query}` URL templates.
- Convert file catalog filters into include/exclude glob rules.
- Register migrated AskAI tools as compatibility tools.
- Never modify or delete the source profile.

## Import Command

```text
veyra import keypirinha --source <keypirinha-portable-root>
```

Expected flags:

- `--dry-run`: print the generated config without writing.
- `--profile <path>`: write to a specific Veyra profile.
- `--copy-tools`: copy tool manifests and runners into the Veyra profile.
- `--reference-tools`: reference tool manifests in place.

## Acceptance Criteria

- Current custom commands appear in Veyra search.
- Settings shortcuts open the same OS settings pages.
- Web search aliases open the same URLs.
- File catalog profiles index the same folders.
- Migrated AI tools are visible through the tool router.

