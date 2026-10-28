#!/usr/bin/env bash
# Local development and e2e use a stable throwaway upload signing secret.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."
env_file="${1:-.env}"
touch "$env_file"
existing=$(sed -n 's/^UPLOAD_TOKEN_SECRET=//p' "$env_file" | head -1 | tr -d '"')
if [ "${#existing}" -ge 32 ]; then
  exit 0
fi
if grep -q '^UPLOAD_TOKEN_SECRET=' "$env_file"; then
  sed -i.bak '/^UPLOAD_TOKEN_SECRET=/d' "$env_file"
  rm -f "$env_file.bak"
fi
key=$(openssl rand -hex 32)
printf '\nUPLOAD_TOKEN_SECRET="%s"\n' "$key" >> "$env_file"
echo "Generated throwaway UPLOAD_TOKEN_SECRET in $env_file"
