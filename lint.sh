#!/usr/bin/env bash

set -euo pipefail

case "${1:-}" in
  "")
    bun run check
    ;;
  --fix)
    bun run lint:fix
    bun run check
    ;;
  *)
    printf 'Usage: %s [--fix]\n' "$0" >&2
    exit 2
    ;;
esac
