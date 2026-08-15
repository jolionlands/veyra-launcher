# Build Matrix

## Required Targets

| Platform | Rust target | Priority |
| --- | --- | --- |
| Windows ARM64 | `aarch64-pc-windows-msvc` | P0 |
| Windows x64 | `x86_64-pc-windows-msvc` | P0 |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | P1 |
| Linux x64 | `x86_64-unknown-linux-gnu` | P1 |

Current local/build-state target matrix:

- `aarch64-pc-windows-msvc`: supported and documented
- `x86_64-pc-windows-msvc`: supported and documented
- `x86_64-unknown-linux-gnu`: supported and documented
- `aarch64-unknown-linux-gnu`: supported and documented

## CI Coverage

Current GitHub Actions coverage:

- `x86_64-pc-windows-msvc` on `windows-latest` (build + check)
- `aarch64-pc-windows-msvc` on `windows-11-arm` (build + check)
- `x86_64-unknown-linux-gnu` on `ubuntu-latest` (build + check)
- `aarch64-unknown-linux-gnu` on `ubuntu-24.04-arm` (build + check)

ARM64 Linux and Windows release jobs use GitHub-hosted ARM runner labels. If those labels are unavailable for a fork or account, use `cross` or a self-hosted ARM64 runner for the affected target.

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
- `veyra-launcher-windows-arm64.zip`
- `veyra-launcher-linux-x64.tar.gz`
- `veyra-launcher-linux-arm64.tar.gz`

Each artifact is uploaded with a sibling `.sha256` checksum file.

Local packaging helpers:

```powershell
.\scripts\package-release.ps1 -Targets windows-x64,windows-arm64
```

```bash
bash scripts/package-release.sh linux-x64 linux-arm64
```

Cross-compiling Linux targets from Windows may require a linker toolchain or containerized Linux build. The GitHub release workflow uses native Linux runners for Linux packages.
