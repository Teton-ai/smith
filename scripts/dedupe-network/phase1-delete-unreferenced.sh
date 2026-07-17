#!/usr/bin/env bash
# Phase 1: Delete network rows with zero references anywhere.
#
# Usage: ./phase1-delete-unreferenced.sh <app-api-db-url> [<smith-db-url>] [--commit]
#
#   <app-api-db-url>  Connection string for the App API postgres DB
#                     e.g. postgres://user:pass@host:5432/dbname
#
#   <smith-db-url>    Connection string for the Smith postgres DB.
#                     Defaults to local dev: postgres://postgres:postgres@localhost:5432/postgres
#
#   --commit          Actually commit the deletion. Without this flag the
#                     script runs in dry-run mode (ROLLBACK) and only prints
#                     what would be deleted.

set -euo pipefail

DEV_SMITH_DB_URL="postgres://postgres:postgres@localhost:5432/postgres"

APP_API_DB_URL="${1:-}"
if [[ -z "$APP_API_DB_URL" ]]; then
    echo "Usage: $0 <app-api-db-url> [<smith-db-url>] [--commit]" >&2
    exit 1
fi

# Second arg is either smith-db-url or --commit
if [[ "${2:-}" == "--commit" ]]; then
    SMITH_DB_URL="$DEV_SMITH_DB_URL"
    COMMIT="--commit"
elif [[ -n "${2:-}" ]]; then
    SMITH_DB_URL="${2}"
    COMMIT="${3:-}"
else
    SMITH_DB_URL="$DEV_SMITH_DB_URL"
    COMMIT=""
fi

echo "==> Smith DB: $SMITH_DB_URL"

echo "==> Fetching App API network_smith_id refs..."
APP_API_IDS=$(psql "$APP_API_DB_URL" -t -A -c \
    "SELECT network_smith_id FROM network_smith WHERE network_smith_id IS NOT NULL ORDER BY network_smith_id::int;")

if [[ -z "$APP_API_IDS" ]]; then
    echo "ERROR: no rows returned from App API network_smith table" >&2
    exit 1
fi

ID_COUNT=$(echo "$APP_API_IDS" | wc -l | tr -d ' ')
echo "==> Got $ID_COUNT App API refs."

# Build VALUES list for the temp table
VALUES=$(echo "$APP_API_IDS" | awk '{printf "(%s),", $1}' | sed 's/,$//')

if [[ "$COMMIT" == "--commit" ]]; then
    FINAL_STATEMENT="COMMIT;"
    echo "==> Running in COMMIT mode."
else
    FINAL_STATEMENT="ROLLBACK;"
    echo "==> Running in DRY-RUN mode (ROLLBACK). Pass --commit to apply."
fi

echo "==> Running phase 1 cleanup on Smith DB..."
psql "$SMITH_DB_URL" <<SQL
BEGIN;

-- Normalize empty string passwords to NULL so the dedup key (ssid, hidden, password)
-- treats open networks consistently throughout all phases.
UPDATE network SET password = NULL WHERE password = '';

CREATE TEMP TABLE _app_api_refs (network_smith_id int);
INSERT INTO _app_api_refs VALUES $VALUES;

-- Materialize both result sets upfront before printing
CREATE TEMP TABLE _to_delete AS
SELECT
    n.id,
    n.ssid,
    n.is_network_hidden AS hidden,
    n.password,
    n.password IS NOT NULL AS has_pwd,
    NULL::text AS app_wifi_ids,
    0 AS d_network_id_count,
    0 AS d_current_network_count,
    0 AS dcn_count,
    0 AS dni_count
FROM network n
WHERE NOT EXISTS (SELECT 1 FROM _app_api_refs a WHERE a.network_smith_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device WHERE network_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device WHERE current_network_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device_configured_network WHERE network_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device_network_intent WHERE network_id = n.id);

CREATE TEMP TABLE _to_keep AS
SELECT
    n.id,
    n.ssid,
    n.is_network_hidden AS hidden,
    n.password,
    n.password IS NOT NULL AS has_pwd,
    string_agg(DISTINCT a.network_smith_id::text, ',' ORDER BY a.network_smith_id::text) AS app_wifi_ids,
    count(DISTINCT d_nid.id) AS d_network_id_count,
    count(DISTINCT d_cur.id) AS d_current_network_count,
    count(DISTINCT dcn.device_id) AS dcn_count,
    count(DISTINCT dni.device_id) AS dni_count
FROM network n
LEFT JOIN _app_api_refs a ON a.network_smith_id = n.id
LEFT JOIN device d_nid ON d_nid.network_id = n.id
LEFT JOIN device d_cur ON d_cur.current_network_id = n.id
LEFT JOIN device_configured_network dcn ON dcn.network_id = n.id
LEFT JOIN device_network_intent dni ON dni.network_id = n.id
WHERE
    a.network_smith_id IS NOT NULL
    OR d_nid.id IS NOT NULL
    OR d_cur.id IS NOT NULL
    OR dcn.device_id IS NOT NULL
    OR dni.device_id IS NOT NULL
GROUP BY n.id, n.ssid, n.is_network_hidden, n.password;

-- Compute dup groups across both sets combined, keyed on (ssid, hidden, password)
CREATE TEMP TABLE _groups AS
SELECT ssid, hidden, password,
    dense_rank() OVER (ORDER BY ssid, hidden, password) AS grp
FROM (SELECT ssid, hidden, password FROM _to_delete
      UNION ALL SELECT ssid, hidden, password FROM _to_keep) combined
GROUP BY ssid, hidden, password
HAVING COUNT(*) > 1;

\echo '=== ROWS TO DELETE ==='
SELECT
    d.id, d.ssid, d.hidden, d.has_pwd, d.app_wifi_ids,
    d.d_network_id_count, d.d_current_network_count, d.dcn_count, d.dni_count,
    COALESCE(g.grp::text, '') AS dup_group
FROM _to_delete d
LEFT JOIN _groups g ON g.ssid = d.ssid AND g.hidden = d.hidden AND g.password IS NOT DISTINCT FROM d.password
ORDER BY dup_group, d.ssid, d.id;

\echo '=== ROWS TO KEEP ==='
SELECT
    k.id, k.ssid, k.hidden, k.has_pwd, k.app_wifi_ids,
    k.d_network_id_count, k.d_current_network_count, k.dcn_count, k.dni_count,
    COALESCE(g.grp::text, '') AS dup_group
FROM _to_keep k
LEFT JOIN _groups g ON g.ssid = k.ssid AND g.hidden = k.hidden AND g.password IS NOT DISTINCT FROM k.password
ORDER BY dup_group, k.ssid, k.id;

DELETE FROM network n
WHERE NOT EXISTS (SELECT 1 FROM _app_api_refs a WHERE a.network_smith_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device WHERE network_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device WHERE current_network_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device_configured_network WHERE network_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device_network_intent WHERE network_id = n.id);

SELECT COUNT(*) AS remaining_rows FROM network;

$FINAL_STATEMENT
SQL

echo "==> Done."
