#!/usr/bin/env bash

set -euo pipefail

message_file="${1:?Expected Git commit-message file path}"
max_line_length=72
line_number=0

while IFS= read -r line || [[ -n "$line" ]]; do
  line_number=$((line_number + 1))

  # Git may put commented template/help lines in the message file; Git does
  # not include those in the final commit message.
  [[ "$line" == \#* ]] && continue

  line_length=${#line}
  if (( line_length > max_line_length )); then
    printf 'Commit blocked: line %d is %d characters; the limit is %d.\n' \
      "$line_number" "$line_length" "$max_line_length" >&2
    exit 1
  fi
done < "$message_file"

printf 'Commit message lines are at most %d characters.\n' "$max_line_length"
