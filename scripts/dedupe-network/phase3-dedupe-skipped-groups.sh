#!/usr/bin/env bash
# Phase 3: Deduplicate groups that phase 2 could not handle because every row
# in the group is referenced by a different App API network_smith entry.
#
# For each such group (same ssid/hidden/password, multiple App API refs):
#   1. Pick canonical Smith ID = min(id) among App API-referenced rows.
#   2. Update App API network_smith.network_smith_id for non-canonical rows
#      to point to the canonical ID.
#   3. Remap Smith FKs and delete non-canonical Smith rows.
#
# App API is updated first. If the Smith step fails after App API commits, the
# canonical row still exists in Smith so the state is consistent.
#
# Run phases 1 and 2 before this script.
#
# Usage: ./phase3-dedupe-skipped-groups.sh [<app-api-db-url>] [<smith-db-url>] [--commit]
#
#   <app-api-db-url>  Connection string for the App API postgres DB (read + write).
#                     Defaults to local dev: postgres://postgres:postgres@localhost:5433/postgres
#                     (the DB spun up by setup-test-app-api-db.sh)
#
#   <smith-db-url>    Connection string for the Smith postgres DB.
#                     Defaults to local dev: postgres://postgres:postgres@localhost:5432/postgres
#
#   --commit          Actually commit the changes. Without this flag the script
#                     runs in dry-run mode (ROLLBACK on both DBs) and only prints
#                     what would change.

set -euo pipefail

DEV_APP_API_DB_URL="postgres://postgres:postgres@localhost:5433/postgres"
DEV_SMITH_DB_URL="postgres://postgres:postgres@localhost:5432/postgres"

# Parse args: both DBs are optional and default to dev; --commit can appear anywhere
APP_API_DB_URL="$DEV_APP_API_DB_URL"
SMITH_DB_URL="$DEV_SMITH_DB_URL"
COMMIT=""

for arg in "$@"; do
    if [[ "$arg" == "--commit" ]]; then
        COMMIT="--commit"
    elif [[ -z "${_APP_API_SET:-}" ]]; then
        APP_API_DB_URL="$arg"
        _APP_API_SET=1
    else
        SMITH_DB_URL="$arg"
    fi
done

echo "==> Smith DB:   $SMITH_DB_URL"
echo "==> App API DB: $APP_API_DB_URL"

echo "==> Fetching App API network_smith refs..."
APP_API_ROWS=$(psql "$APP_API_DB_URL" -t -A -F $'\t' -c \
    "SELECT network_wifi_id, network_smith_id FROM network_smith WHERE network_smith_id IS NOT NULL ORDER BY network_wifi_id;")

if [[ -z "$APP_API_ROWS" ]]; then
    echo "ERROR: no rows returned from App API network_smith table" >&2
    exit 1
fi

ROW_COUNT=$(echo "$APP_API_ROWS" | wc -l | tr -d ' ')
echo "==> Got $ROW_COUNT App API refs."

VALUES=$(echo "$APP_API_ROWS" | awk -F'\t' '{printf "(%s,%s),", $1, $2}' | sed 's/,$//')

# Compute the remap (old_smith_id -> canonical_smith_id) for skipped groups.
# This is a read-only query on Smith; no transaction needed.
echo "==> Computing skipped group remapping from Smith DB..."
REMAP_TSV=$(psql "$SMITH_DB_URL" -t -A -F $'\t' <<SQL
WITH
_refs (network_wifi_id, network_smith_id) AS (VALUES $VALUES),
all_refs AS (
    SELECT
        n.id,
        n.ssid,
        n.is_network_hidden AS hidden,
        n.password,
        dense_rank() OVER (ORDER BY n.ssid, n.is_network_hidden, n.password) AS grp,
        count(*) OVER (PARTITION BY n.ssid, n.is_network_hidden, n.password) AS grp_size,
        (SELECT COUNT(*) FROM _refs a WHERE a.network_smith_id = n.id) AS app_ref_count
    FROM network n
    WHERE EXISTS (SELECT 1 FROM _refs a WHERE a.network_smith_id = n.id)
       OR EXISTS (SELECT 1 FROM device WHERE network_id = n.id)
       OR EXISTS (SELECT 1 FROM device WHERE current_network_id = n.id)
       OR EXISTS (SELECT 1 FROM device_configured_network WHERE network_id = n.id)
       OR EXISTS (SELECT 1 FROM device_network_intent WHERE network_id = n.id)
),
skip_grps AS (
    SELECT grp
    FROM all_refs
    WHERE grp_size > 1
    GROUP BY grp
    HAVING SUM(CASE WHEN app_ref_count > 0 THEN 1 ELSE 0 END) > 1
),
canonical AS (
    SELECT DISTINCT ON (grp) grp, id AS canonical_id
    FROM all_refs
    WHERE grp IN (SELECT grp FROM skip_grps) AND app_ref_count > 0
    ORDER BY grp, id ASC
)
SELECT r.id::text, c.canonical_id::text
FROM all_refs r
JOIN canonical c ON c.grp = r.grp
WHERE r.grp IN (SELECT grp FROM skip_grps)
  AND r.app_ref_count > 0
  AND r.id != c.canonical_id;
SQL
)

if [[ -z "$REMAP_TSV" ]]; then
    echo "==> No skipped groups found. Nothing to do."
    exit 0
fi

REMAP_COUNT=$(echo "$REMAP_TSV" | wc -l | tr -d ' ')
echo "==> $REMAP_COUNT non-canonical Smith rows to remap."

# Build VALUES for the remap temp tables
REMAP_VALUES=$(echo "$REMAP_TSV" | awk -F'\t' '{printf "(%s,%s),", $1, $2}' | sed 's/,$//')

if [[ "$COMMIT" == "--commit" ]]; then
    FINAL_STATEMENT="COMMIT;"
    echo "==> Running in COMMIT mode."
else
    FINAL_STATEMENT="ROLLBACK;"
    echo "==> Running in DRY-RUN mode (ROLLBACK). Pass --commit to apply."
fi

# ── Report + Smith mutations ────────────────────────────────────────────────

echo "==> Running phase 3 on Smith DB..."
psql "$SMITH_DB_URL" <<SQL
BEGIN;

CREATE TEMP TABLE _app_api_refs (network_wifi_id int, network_smith_id int);
INSERT INTO _app_api_refs VALUES $VALUES;

CREATE TEMP TABLE _to_remap (old_id int, canonical_id int);
INSERT INTO _to_remap VALUES $REMAP_VALUES;

-- Full group view with action column
\echo ''
\echo '=== SKIPPED GROUPS: full remap plan ==='
WITH all_refs AS (
    SELECT
        n.id,
        n.ssid,
        n.is_network_hidden AS hidden,
        n.password IS NOT NULL AS has_pwd,
        (SELECT COUNT(*) FROM _app_api_refs a WHERE a.network_smith_id = n.id) AS app_ref_count,
        (SELECT COUNT(*) FROM device WHERE network_id = n.id) +
        (SELECT COUNT(*) FROM device WHERE current_network_id = n.id) +
        (SELECT COUNT(*) FROM device_configured_network WHERE network_id = n.id) +
        (SELECT COUNT(*) FROM device_network_intent WHERE network_id = n.id) AS smith_ref_count,
        dense_rank() OVER (ORDER BY n.ssid, n.is_network_hidden, n.password) AS grp
    FROM network n
    WHERE EXISTS (SELECT 1 FROM _app_api_refs a WHERE a.network_smith_id = n.id)
)
SELECT
    r.grp,
    r.id,
    r.ssid,
    r.hidden,
    r.has_pwd,
    r.app_ref_count,
    r.smith_ref_count,
    CASE WHEN m.canonical_id IS NOT NULL THEN 'remap → ' || m.canonical_id::text
         ELSE 'keep (canonical)'
    END AS action
FROM all_refs r
LEFT JOIN _to_remap m ON m.old_id = r.id
ORDER BY r.grp, CASE WHEN m.canonical_id IS NULL THEN 0 ELSE 1 END, r.id;

UPDATE device SET network_id = r.canonical_id
FROM _to_remap r WHERE device.network_id = r.old_id;

UPDATE device SET current_network_id = r.canonical_id
FROM _to_remap r WHERE device.current_network_id = r.old_id;

UPDATE device_configured_network SET network_id = r.canonical_id
FROM _to_remap r WHERE device_configured_network.network_id = r.old_id;

UPDATE device_network_intent SET network_id = r.canonical_id
FROM _to_remap r WHERE device_network_intent.network_id = r.old_id;

DELETE FROM network WHERE id IN (SELECT old_id FROM _to_remap);

\echo ''
SELECT COUNT(*) AS remaining_rows FROM network;

$FINAL_STATEMENT
SQL

# ── App API update ──────────────────────────────────────────────────────────

echo "==> Running phase 3 on App API DB..."
psql "$APP_API_DB_URL" <<SQL
BEGIN;

CREATE TEMP TABLE _to_remap (old_id int, canonical_id int);
INSERT INTO _to_remap VALUES $REMAP_VALUES;

\echo ''
\echo '=== App API network_smith rows to update ==='
SELECT
    ns.id AS network_smith_row_id,
    ns.network_wifi_id,
    ns.network_smith_id::int AS old_smith_id,
    m.canonical_id AS new_smith_id
FROM network_smith ns
JOIN _to_remap m ON m.old_id = ns.network_smith_id::int
ORDER BY ns.network_wifi_id;

UPDATE network_smith
SET network_smith_id = m.canonical_id::text
FROM _to_remap m
WHERE network_smith.network_smith_id::int = m.old_id;

\echo ''
SELECT COUNT(*) AS updated_rows FROM network_smith ns
JOIN _to_remap m ON m.canonical_id::text = ns.network_smith_id;

$FINAL_STATEMENT
SQL

echo "==> Done."
