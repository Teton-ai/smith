#!/usr/bin/env bash
# Show all referenced network rows with ref counts and duplicate groups.
# Fetches App API refs live so the output is always up to date.
#
# Usage: ./network-cross-analysis.sh [<app-api-db-url>] [<smith-db-url>]
#
#   <app-api-db-url>  Defaults to local dev: postgres://postgres:postgres@localhost:5433/postgres
#   <smith-db-url>    Defaults to local dev: postgres://postgres:postgres@localhost:5432/postgres

set -euo pipefail

DEV_APP_API_DB_URL="postgres://postgres:postgres@localhost:5433/postgres"
DEV_SMITH_DB_URL="postgres://postgres:postgres@localhost:5432/postgres"

APP_API_DB_URL="${1:-$DEV_APP_API_DB_URL}"
SMITH_DB_URL="${2:-$DEV_SMITH_DB_URL}"

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

psql "$SMITH_DB_URL" <<SQL
CREATE TEMP TABLE _app_api_refs (network_wifi_id int, network_smith_id int);
INSERT INTO _app_api_refs VALUES $VALUES;

WITH referenced AS (
    SELECT
        n.id,
        n.ssid,
        n.is_network_hidden AS hidden,
        n.password,
        n.password IS NOT NULL AS has_pwd,
        string_agg(DISTINCT a.network_wifi_id::text, ',' ORDER BY a.network_wifi_id::text) AS app_wifi_ids,
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
    GROUP BY n.id, n.ssid, n.is_network_hidden, n.password
),
groups AS (
    SELECT ssid, hidden, password,
        dense_rank() OVER (ORDER BY ssid, hidden, password) AS grp
    FROM referenced
    GROUP BY ssid, hidden, password
)
SELECT
    r.id,
    r.ssid,
    r.hidden,
    r.has_pwd,
    r.app_wifi_ids,
    r.d_network_id_count,
    r.d_current_network_count,
    r.dcn_count,
    r.dni_count,
    CASE WHEN count(*) OVER (PARTITION BY r.ssid, r.hidden, r.password) > 1
        THEN g.grp::text
        ELSE ''
    END AS dup_group
FROM referenced r
JOIN groups g ON g.ssid = r.ssid AND g.hidden = r.hidden AND g.password IS NOT DISTINCT FROM r.password
ORDER BY dup_group DESC, r.ssid, r.id;
SQL
