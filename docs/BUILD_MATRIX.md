# Build Matrix

## Required Targets

| Platform | Rust target | Priority |
| --- | --- | --- |
| Windows ARM64 | `aarch64-pc-windows-msvc` | P0 |
| Windows x64 | `x86_64-pc-windows-msvc` | P0 |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | P1 |
| Linux x64 | `x86_64-unknown-linux-gnu` | P1 |

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

Linux cross-compilation from Windows may require an external linker. CI should use native Linux runners or cross-build containers.

