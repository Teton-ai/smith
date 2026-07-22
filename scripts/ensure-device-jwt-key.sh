#!/usr/bin/env bash
# Ensure .env carries DEVICE_JWT_PRIVATE_KEY_PEM, which the API requires at
# boot. Generates a throwaway Ed25519 key if the variable is absent — fine for
# local development and e2e runs; production keys live in infrastructure.
# The PEM is stored double-quoted on one line with \n escapes, which both
# dotenvy (API) and docker compose expand back into newlines.
set -euo pipefail
cd "$(dirname "$0")/.."

env_file="${1:-.env}"
touch "$env_file"

existing=$(sed -n 's/^DEVICE_JWT_PRIVATE_KEY_PEM=//p' "$env_file" | head -1 | tr -d '"')
if [ -n "$existing" ] && printf '%b' "$existing" | openssl pkey -noout 2>/dev/null; then
  exit 0
fi

if grep -q '^DEVICE_JWT_PRIVATE_KEY_PEM=' "$env_file"; then
  sed -i.bak '/^DEVICE_JWT_PRIVATE_KEY_PEM=/d' "$env_file" && rm -f "$env_file.bak"
fi

key=$(openssl genpkey -algorithm Ed25519 | awk 'BEGIN{ORS="\\n"} {print}')
printf 'DEVICE_JWT_PRIVATE_KEY_PEM="%s"\n' "$key" >> "$env_file"
echo "Generated throwaway DEVICE_JWT_PRIVATE_KEY_PEM in $env_file"
