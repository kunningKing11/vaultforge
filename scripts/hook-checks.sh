#!/usr/bin/env bash

# Run every command in the selected Git hook mode even when one fails, then print
# one summary and block the Git operation if any check was unsuccessful.
set -uo pipefail

declare -a check_names=()
declare -a check_results=()
failures=0

run_check() {
  local name="$1"
  shift

  printf '\n==> %s\n' "$name"
  if "$@"; then
    check_names+=("$name")
    check_results+=("PASS")
  else
    local status=$?
    check_names+=("$name")
    check_results+=("FAIL (exit $status)")
    failures=$((failures + 1))
  fi
}

case "${1:-}" in
commit)
  # Bun and Cargo may update manifests or lockfiles; stage those
  # dependency files for this same commit only.
  run_check "Sync Cargo package version" bun run sync-cargo-version
  run_check "bun install" bun install
  run_check "Oxlint" bun run lint:oxlint
  run_check "Oxfmt" bun run format:check
  run_check "TypeScript" bun run typecheck
  run_check "TypeScript tests" bun test --parallel
  run_check "Cargo fmt" cargo fmt --all --manifest-path src-tauri/Cargo.toml --check
  run_check "Cargo check" cargo check --manifest-path src-tauri/Cargo.toml
  run_check "Stage dependency files" git add -- package.json bun.lock src-tauri/Cargo.toml src-tauri/Cargo.lock
  operation="commit"
  ;;
push)
  run_check "Rust Analyzer analysis" bash -c 'cd src-tauri && rust-analyzer analysis-stats .'
  run_check "Cargo test" cargo test --manifest-path src-tauri/Cargo.toml
  operation="push"
  ;;
*)
  printf 'Usage: %s {commit|push}\n' "$0" >&2
  exit 2
  ;;
esac

printf '\n%s quality-gate summary\n' "$operation"
printf '%-24s %s\n' "Check" "Result"
printf '%-24s %s\n' "------------------------" "------------"
for index in "${!check_names[@]}"; do
  printf '%-24s %s\n' "${check_names[$index]}" "${check_results[$index]}"
done

if ((failures > 0)); then
  printf '\n%d check(s) failed; blocking %s.\n' "$failures" "$operation" >&2
  exit 1
fi

printf '\nAll checks passed.\n'
