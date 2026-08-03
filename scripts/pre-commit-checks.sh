#!/usr/bin/env bash

# Run every command even when one fails, then print one summary and block the
# commit if any check was unsuccessful. The package managers may update their
# manifest/lockfiles; stage those dependency files for this same commit only.
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

run_check "npm install" npm install
run_check "bun install" bun install
run_check "Stage dependency files" git add -- package.json package-lock.json bun.lock
run_check "Oxlint" npm run lint:oxlint
run_check "Oxfmt" npm run format:check
run_check "TypeScript" npm run typecheck
run_check "Cargo check" cargo check --manifest-path src-tauri/Cargo.toml
run_check "Rust Analyzer analysis" bash -c 'cd src-tauri && rust-analyzer analysis-stats .'
run_check "Cargo test" cargo test --manifest-path src-tauri/Cargo.toml

printf '\nSummary\n'
printf '%-24s %s\n' "Check" "Result"
printf '%-24s %s\n' "------------------------" "------------"
for index in "${!check_names[@]}"; do
  printf '%-24s %s\n' "${check_names[$index]}" "${check_results[$index]}"
done

if (( failures > 0 )); then
  printf '\n%d check(s) failed; blocking commit.\n' "$failures" >&2
  exit 1
fi

printf '\nAll checks passed.\n'
