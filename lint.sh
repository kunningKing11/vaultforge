#!/usr/bin/env bash

set -euo pipefail

case "${1:-}" in
  "")
    npm run check
    ;;
  --fix)
    npm run lint:fix
    npm run check
    ;;
  *)
    printf 'Usage: %s [--fix]\n' "$0" >&2
    exit 2
    ;;
esac
