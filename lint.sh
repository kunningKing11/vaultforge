#!/bin/bash

printf "Linting \`src/\`...\n"

if [[ "$1" == "--fix" ]]; then
  printf "Running \`npx eslint --fix src/\` to automatically fix linting errors..."
  printf "Run \`bash lint.sh\` to get warnings instead.\n"
  npx eslint --fix src/
  exit $?
else
  printf "Running \`npx eslint src/\` to get linting warnings..."
  printf "Run \`bash lint.sh --fix\` to automatically fix linting errors instead.\n"
  npx eslint src/
fi

printf "\nFinished linting \`src/\`."
