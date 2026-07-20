#!/usr/bin/env bash
# Ensure .env carries a syntactically valid AUTH0_ISSUER. The API needs one
# to construct its JWKS client at boot, even though e2e tests never exercise
# staff/Auth0 login (only device JWTs). .env.template ships a placeholder
# comment ("# FILL THIS OUT") which isn't a valid URL and panics the API on
# startup, so replace it with a throwaway value for e2e runs.
set -euo pipefail
cd "$(dirname "$0")/.."

env_file="${1:-.env}"
touch "$env_file"

if grep -qE '^AUTH0_ISSUER=https?://' "$env_file"; then
  exit 0
fi

if grep -q '^AUTH0_ISSUER=' "$env_file"; then
  sed -i.bak '/^AUTH0_ISSUER=/d' "$env_file" && rm -f "$env_file.bak"
fi

echo 'AUTH0_ISSUER=https://example.com/' >> "$env_file"
echo "Generated throwaway AUTH0_ISSUER in $env_file"
