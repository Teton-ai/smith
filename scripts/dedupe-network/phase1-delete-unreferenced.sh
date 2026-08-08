#!/usr/bin/env bash
# Phase 1: Delete network rows with zero references anywhere.
# See README.md for the delete condition, the watermark, and the race notes.
#
# Usage: ./phase1-delete-unreferenced.sh <app-api-db-url> [<smith-db-url>] [--commit]
#
#   <app-api-db-url>  Connection string for the App API postgres DB
#                     e.g. postgres://user:pass@host:5432/dbname
#
#   <smith-db-url>    Smith postgres DB. Connection string or docker://<container>.
#                     Defaults to docker://smith-postgres.
#
#   --commit          Apply. Without it the script dry-runs (ROLLBACK).

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

DEV_SMITH_DB_URL="docker://smith-postgres"

APP_API_DB_URL="${1:-}"
if [[ -z "$APP_API_DB_URL" ]]; then
    echo "Usage: $0 <app-api-db-url> [<smith-db-url>] [--commit]" >&2
    exit 1
fi

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

smith_psql() { db_psql "$SMITH_DB_URL" "$@"; }

echo "==> Smith DB: $SMITH_DB_URL"

# Before the ref fetch, deliberately.
WATERMARK=$(smith_watermark "$SMITH_DB_URL")
echo "==> Watermark: network.id <= $WATERMARK (rows created after this are never deleted)"

echo "==> Fetching App API network_smith refs..."
fetch_app_api_refs "$APP_API_DB_URL"
echo "==> Got $APP_API_REF_COUNT App API refs."

echo "==> Re-fetching refs to narrow the bridge-write race..."
refresh_app_api_refs "$APP_API_DB_URL"
echo "==> $APP_API_REF_COUNT refs after union."

DANGLING_BEFORE=$(dangling_bridges "$SMITH_DB_URL" "$APP_API_REF_CSV")

if [[ "$COMMIT" == "--commit" ]]; then
    FINAL_STATEMENT="COMMIT;"
    echo "==> Running in COMMIT mode."
else
    FINAL_STATEMENT="ROLLBACK;"
    echo "==> Running in DRY-RUN mode (ROLLBACK). Pass --commit to apply."
fi

echo "==> Running phase 1 cleanup on Smith DB..."
smith_psql -v ON_ERROR_STOP=1 <<SQL
BEGIN;

\echo '=== EMPTY PASSWORDS NORMALIZED TO NULL ==='
SELECT COUNT(*) AS rows_normalized FROM network WHERE password = '' AND id <= $WATERMARK;

UPDATE network SET password = NULL WHERE password = '' AND id <= $WATERMARK;

CREATE TEMP TABLE _app_api_refs (network_wifi_id int, network_smith_id int);
\copy _app_api_refs (network_wifi_id, network_smith_id) FROM STDIN WITH (FORMAT csv)
$APP_API_REF_CSV
\.

CREATE TEMP TABLE _to_delete AS
SELECT n.id, n.ssid, n.is_network_hidden AS hidden, n.password IS NOT NULL AS has_pwd,
       n.credentials ->> 'psk' AS psk, n.network_type::text AS type,
       n.security_type AS sec, n.identity AS identval
FROM network n
WHERE NOT EXISTS (SELECT 1 FROM _app_api_refs a WHERE a.network_smith_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device WHERE network_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device WHERE current_network_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device_configured_network WHERE network_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM device_network_intent WHERE network_id = n.id)
  AND NOT EXISTS (SELECT 1 FROM network_reference WHERE network_id = n.id)
  AND n.id <= $WATERMARK;

CREATE TEMP TABLE _to_keep AS
SELECT n.id, n.ssid, n.is_network_hidden AS hidden, n.password IS NOT NULL AS has_pwd,
    n.credentials ->> 'psk' AS psk, n.network_type::text AS type,
    n.security_type AS sec, n.identity AS identval,
    (SELECT string_agg(DISTINCT a.network_wifi_id::text, ',' ORDER BY a.network_wifi_id::text)
     FROM _app_api_refs a WHERE a.network_smith_id = n.id) AS app_wifi_ids,
    (SELECT count(*) FROM device WHERE network_id = n.id) AS d_network_id_count,
    (SELECT count(*) FROM device WHERE current_network_id = n.id) AS d_current_network_count,
    (SELECT count(*) FROM device_configured_network WHERE network_id = n.id) AS dcn_count,
    (SELECT count(*) FROM device_network_intent WHERE network_id = n.id) AS dni_count
FROM network n
WHERE EXISTS (SELECT 1 FROM _app_api_refs a WHERE a.network_smith_id = n.id)
   OR EXISTS (SELECT 1 FROM device WHERE network_id = n.id)
   OR EXISTS (SELECT 1 FROM device WHERE current_network_id = n.id)
   OR EXISTS (SELECT 1 FROM device_configured_network WHERE network_id = n.id)
   OR EXISTS (SELECT 1 FROM device_network_intent WHERE network_id = n.id)
   OR EXISTS (SELECT 1 FROM network_reference WHERE network_id = n.id);

-- dup_group uses the same content key as phases 2 to 4, so a group shown here
-- is the group those phases will act on.
CREATE TEMP TABLE _groups AS
SELECT ssid, hidden, psk, type, sec, identval,
    dense_rank() OVER (ORDER BY ssid, hidden, psk, type, sec, identval) AS grp
FROM (SELECT ssid, hidden, psk, type, sec, identval FROM _to_delete
      UNION ALL SELECT ssid, hidden, psk, type, sec, identval FROM _to_keep) combined
GROUP BY ssid, hidden, psk, type, sec, identval
HAVING COUNT(*) > 1;

\echo '=== ROWS TO DELETE ==='
SELECT d.id, d.ssid, d.hidden, d.has_pwd, COALESCE(g.grp::text, '') AS dup_group
FROM _to_delete d
LEFT JOIN _groups g ON g.ssid IS NOT DISTINCT FROM d.ssid AND g.hidden = d.hidden
     AND g.psk IS NOT DISTINCT FROM d.psk AND g.type = d.type
     AND g.sec IS NOT DISTINCT FROM d.sec AND g.identval IS NOT DISTINCT FROM d.identval
ORDER BY dup_group, d.ssid, d.id;

\echo '=== ROWS TO KEEP ==='
SELECT k.id, k.ssid, k.hidden, k.has_pwd, k.app_wifi_ids,
    k.d_network_id_count, k.d_current_network_count, k.dcn_count, k.dni_count,
    COALESCE(g.grp::text, '') AS dup_group
FROM _to_keep k
LEFT JOIN _groups g ON g.ssid IS NOT DISTINCT FROM k.ssid AND g.hidden = k.hidden
     AND g.psk IS NOT DISTINCT FROM k.psk AND g.type = k.type
     AND g.sec IS NOT DISTINCT FROM k.sec AND g.identval IS NOT DISTINCT FROM k.identval
ORDER BY dup_group, k.ssid, k.id;

-- Driven from the staged set so the report above is exactly what gets deleted.
DELETE FROM network n WHERE EXISTS (SELECT 1 FROM _to_delete d WHERE d.id = n.id);

SELECT COUNT(*) AS remaining_rows FROM network;

$FINAL_STATEMENT
SQL

verify_no_new_dangling "$APP_API_DB_URL" "$SMITH_DB_URL" "$DANGLING_BEFORE"

echo "==> Done."
