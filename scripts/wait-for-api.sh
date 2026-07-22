#!/usr/bin/env bash
# Wait for the API to answer /health, printing the version it reports.
set -euo pipefail

API_URL="${E2E_API_URL:-http://localhost:8080}"
DEADLINE=$((SECONDS + 180))

until body=$(curl --connect-timeout 5 --max-time 10 -fsS "${API_URL}/health" 2>/dev/null); do
  if ((SECONDS >= DEADLINE)); then
    echo "API at ${API_URL} did not become healthy within 180s" >&2
    exit 1
  fi
  sleep 2
done

echo "API is up: ${body}"
