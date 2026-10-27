#!/usr/bin/env bash

# Type-check the complete staged project, including files that import changed files.
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
snapshot_dir="$(mktemp -d "${TMPDIR:-/tmp}/vaultforge-staged-types.XXXXXXXX")"
trap 'rm -rf -- "$snapshot_dir"' EXIT

git -C "$repo_root" checkout-index --all --prefix="$snapshot_dir/"
if [[ ! -d "$repo_root/node_modules" ]]; then
  printf 'Cannot type-check staged files: node_modules is missing. Run bun install.\n' >&2
  exit 1
fi
ln -s -- "$repo_root/node_modules" "$snapshot_dir/node_modules"
cd "$snapshot_dir"
bun check --no-script
