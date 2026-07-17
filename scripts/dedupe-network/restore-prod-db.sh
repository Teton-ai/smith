#!/usr/bin/env bash
# Restore a prod pg_dump to the local dev DB and apply any unreleased local migrations.
#
# Usage: ./scripts/restore-prod-db.sh <dump_file>
#
# The dump is expected to be a plain-text pg_dump from a Neon-hosted DB.
# Neon adds a \restrict meta-command at the top and assumes the public schema
# pre-exists, both of which are handled here.

set -euo pipefail

DUMP_FILE="${1:-}"
DB_URL="postgres://postgres:postgres@localhost:5432/postgres"

if [[ -z "$DUMP_FILE" ]]; then
    echo "Usage: $0 <dump_file>" >&2
    exit 1
fi

if [[ ! -f "$DUMP_FILE" ]]; then
    echo "Error: dump file not found: $DUMP_FILE" >&2
    exit 1
fi

echo "==> Dropping existing schemas..."
psql "$DB_URL" -c "DROP SCHEMA IF EXISTS public CASCADE;"
psql "$DB_URL" -c "DROP SCHEMA IF EXISTS partman CASCADE;"
psql "$DB_URL" -c "DROP SCHEMA IF EXISTS auth CASCADE;"

echo "==> Recreating public schema (Neon dumps assume it pre-exists)..."
psql "$DB_URL" -c "CREATE SCHEMA public;"

echo "==> Creating prod roles that don't exist locally..."
psql "$DB_URL" -c "CREATE ROLE fleetadmin WITH LOGIN SUPERUSER;" 2>/dev/null || true
psql "$DB_URL" -c "CREATE ROLE grafana_reader;" 2>/dev/null || true

echo "==> Restoring dump (this may take a while)..."
grep -v "^\\\\restrict" "$DUMP_FILE" \
    | psql "$DB_URL" 2>&1 \
    | grep "^ERROR" \
    | grep -v "already exists" \
    | grep -v "transaction_timeout" \
    >&2 || true

echo "==> Fixing ownership (prod tables owned by fleetadmin, local API connects as postgres)..."
psql "$DB_URL" -c "REASSIGN OWNED BY fleetadmin TO postgres;"

echo "==> Applying unreleased local migrations..."
make migrate

echo "==> Done. Local DB now has prod data with all local migrations applied."
