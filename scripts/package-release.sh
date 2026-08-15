#!/usr/bin/env bash
set -euo pipefail

targets=("$@")
if [ "${#targets[@]}" -eq 0 ]; then
  targets=("linux-x64")
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="$repo_root/dist"
stage_root="$dist_dir/stage"

target_triple() {
  case "$1" in
    windows-x64) echo "x86_64-pc-windows-msvc" ;;
    windows-arm64) echo "aarch64-pc-windows-msvc" ;;
    linux-x64) echo "x86_64-unknown-linux-gnu" ;;
    linux-arm64) echo "aarch64-unknown-linux-gnu" ;;
    *) echo "Unknown target alias '$1'" >&2; return 1 ;;
  esac
}

target_binary() {
  case "$1" in
    windows-*) echo "veyra-app.exe" ;;
    linux-*) echo "veyra-app" ;;
    *) echo "Unknown target alias '$1'" >&2; return 1 ;;
  esac
}

target_archive() {
  case "$1" in
    windows-x64) echo "veyra-launcher-windows-x64.zip" ;;
    windows-arm64) echo "veyra-launcher-windows-arm64.zip" ;;
    linux-x64) echo "veyra-launcher-linux-x64.tar.gz" ;;
    linux-arm64) echo "veyra-launcher-linux-arm64.tar.gz" ;;
    *) echo "Unknown target alias '$1'" >&2; return 1 ;;
  esac
}

write_checksum() {
  local archive_path="$1"
  local archive_name
  archive_name="$(basename "$archive_path")"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$archive_path" | awk -v name="$archive_name" '{ print $1 "  " name }' > "$archive_path.sha256"
  else
    shasum -a 256 "$archive_path" | awk -v name="$archive_name" '{ print $1 "  " name }' > "$archive_path.sha256"
  fi
}

mkdir -p "$dist_dir" "$stage_root"

for target in "${targets[@]}"; do
  triple="$(target_triple "$target")"
  binary="$(target_binary "$target")"
  archive="$(target_archive "$target")"
  package_name="veyra-launcher-$target"
  stage_dir="$stage_root/$package_name"
  archive_path="$dist_dir/$archive"
  binary_path="$repo_root/target/$triple/release/$binary"

  cargo build --release --target "$triple" -p veyra-app

  if [ ! -f "$binary_path" ]; then
    echo "Missing binary: $binary_path" >&2
    exit 1
  fi

  rm -rf "$stage_dir"
  rm -f "$archive_path" "$archive_path.sha256"
  mkdir -p "$stage_dir"

  cp "$binary_path" "$stage_dir/"
  cp "$repo_root/README.md" "$stage_dir/"
  cp "$repo_root/LICENSE" "$stage_dir/"
  cp -R "$repo_root/docs" "$stage_dir/"
  cp -R "$repo_root/scripts" "$stage_dir/"

  case "$archive" in
    *.zip)
      (cd "$stage_dir" && zip -qr "$archive_path" .)
      ;;
    *.tar.gz)
      tar -czf "$archive_path" -C "$stage_root" "$package_name"
      ;;
  esac

  write_checksum "$archive_path"
  echo "Packaged $archive_path"
done
