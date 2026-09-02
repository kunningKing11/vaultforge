#!/usr/bin/env bash

set -euo pipefail

message_file="${1:?Expected Git commit-message file path}"
max_line_length=72
line_number=0
longest_line_length=0
ansi_sgr_pattern=$'\033''\[[0-9;]*m'

while IFS= read -r line || [[ -n "$line" ]]; do
  line_number=$((line_number + 1))

  # Git may put commented template/help lines in the message file; Git does
  # not include those in the final commit message.
  [[ "$line" == \#* ]] && continue

  visible_line="$line"
  while [[ "$visible_line" =~ $ansi_sgr_pattern ]]; do
    visible_line=${visible_line/"${BASH_REMATCH[0]}"/}
  done

  line_length=${#visible_line}
  if (( line_length > longest_line_length )); then
    longest_line_length=$line_length
  fi

  if (( line_length > max_line_length )); then
    printf 'Commit blocked: line %d is %d characters; the limit is %d.\n' \
      "$line_number" "$line_length" "$max_line_length" >&2
    exit 1
  fi
done < "$message_file"

printf 'Commit message lines are at most %d characters (%d characters).\n' \
  "$max_line_length" "$longest_line_length"
