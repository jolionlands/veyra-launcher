# Build Matrix

## Required Targets

| Platform | Rust target | Priority |
| --- | --- | --- |
| Windows ARM64 | `aarch64-pc-windows-msvc` | P0 |
| Windows x64 | `x86_64-pc-windows-msvc` | P0 |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | P1 |
| Linux x64 | `x86_64-unknown-linux-gnu` | P1 |

## CI Coverage

Current GitHub Actions coverage:

- `x86_64-pc-windows-msvc` on `windows-latest` (build + check)
- `aarch64-pc-windows-msvc` on `windows-latest` (build + check)
- `x86_64-unknown-linux-gnu` on `ubuntu-latest` (build + check)

`aarch64-unknown-linux-gnu` is not yet in CI, because the repository only uses GitHub-hosted Linux/x64 runners. Add an ARM64 Linux runner or container strategy when that coverage is needed.

## Local Target Setup

```powershell
rustup target add aarch64-pc-windows-msvc
rustup target add x86_64-pc-windows-msvc
rustup target add aarch64-unknown-linux-gnu
rustup target add x86_64-unknown-linux-gnu
```

## Build Commands

Windows ARM64:

```powershell
cargo build --release --target aarch64-pc-windows-msvc -p veyra-app
```

Windows x64:

```powershell
cargo build --release --target x86_64-pc-windows-msvc -p veyra-app
```

Linux ARM64:

```bash
cargo build --release --target aarch64-unknown-linux-gnu -p veyra-app
```

Linux x64:

```bash
cargo build --release --target x86_64-unknown-linux-gnu -p veyra-app
```

## Manual Release Workflow

The repository includes `.github/workflows/release.yml` as a manual release pipeline (`workflow_dispatch`) that currently publishes:

- `veyra-launcher-windows-x64.zip`
- `veyra-launcher-linux-x64.tar.gz`

ARM64 release artifacts can be added when a practical release path is available.

Linux cross-compilation from Windows may require an external linker. CI should use native Linux runners or cross-build containers.
