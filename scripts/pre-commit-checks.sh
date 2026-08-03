#!/usr/bin/env bash

# Run every check even when one fails, then print one summary and block the
# commit if any check was unsuccessful.
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
