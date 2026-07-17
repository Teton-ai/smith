#!/usr/bin/env bash
# Spin up a minimal local App API DB for testing phase 3.
#
# Creates a plain postgres container on port 5433, creates a stripped-down
# network_smith table (no FK constraints), and seeds it from prod.
# Only the columns needed by the phase 3 script are populated.
#
# Usage: ./setup-test-app-api-db.sh <prod-app-api-db-url>
#
#   <prod-app-api-db-url>  Read-only connection string for the prod App API DB

set -euo pipefail

PROD_URL="${1:-}"
if [[ -z "$PROD_URL" ]]; then
    echo "Usage: $0 <prod-app-api-db-url>" >&2
    exit 1
fi

LOCAL_URL="postgres://postgres:postgres@localhost:5433/postgres"
CONTAINER_NAME="smith-test-app-api"

echo "==> Starting test App API postgres on port 5433..."
docker run -d \
    --name "$CONTAINER_NAME" \
    -e POSTGRES_PASSWORD=postgres \
    -p 5433:5432 \
    postgres:16

echo "==> Waiting for postgres to be ready..."
until docker exec "$CONTAINER_NAME" pg_isready -U postgres -q; do
    sleep 1
done

echo "==> Creating network_smith table (no FK constraints)..."
psql "$LOCAL_URL" <<SQL
CREATE TABLE network_smith (
    id               BIGINT PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    network_wifi_id  BIGINT NOT NULL UNIQUE,
    network_smith_id TEXT,
    error_message    TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
SQL

echo "==> Seeding network_smith from prod..."
ROW_COUNT=$(psql "$PROD_URL" -t -A -c "SELECT COUNT(*) FROM network_smith;")
echo "==> $ROW_COUNT rows to copy..."

psql "$PROD_URL" -c "\copy (SELECT id, network_wifi_id, network_smith_id, error_message, created_at, updated_at FROM network_smith ORDER BY id) TO STDOUT" \
    | psql "$LOCAL_URL" -c "\copy network_smith (id, network_wifi_id, network_smith_id, error_message, created_at, updated_at) FROM STDIN"

# Sync the IDENTITY sequence to avoid conflicts on future inserts
MAX_ID=$(psql "$LOCAL_URL" -t -A -c "SELECT MAX(id) FROM network_smith;")
psql "$LOCAL_URL" -c "SELECT setval(pg_get_serial_sequence('network_smith', 'id'), $MAX_ID);" > /dev/null

echo "==> Done. Local App API DB is ready at: $LOCAL_URL"
echo "==> To tear it down: docker rm -f $CONTAINER_NAME"
