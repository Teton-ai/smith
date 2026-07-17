#!/usr/bin/env bash
# Phase 2: Deduplicate referenced network rows by remapping FKs to canonical IDs,
# then deleting the non-canonical duplicates.
#
# Canonical ID selection within each duplicate group (same ssid/hidden/password):
#   1. The ID currently referenced by App API (network_smith.network_smith_id)
#   2. Fallback: the ID with the most Smith-side FK references
#   3. Tie-break: min(id)
#
# Groups where more than one row is referenced by a different App API entry cannot
# be fully merged without updating App API. However, device-only rows within those
# groups (app_ref_count=0) are remapped to the lowest App API-referenced ID in the
# group and deleted. Only the App API-referenced rows themselves are left as-is.
#
# Usage: ./phase2-dedupe-referenced.sh <app-api-db-url> [<smith-db-url>] [--commit]
#
#   <app-api-db-url>  Connection string for the App API postgres DB (read-only)
#                     e.g. postgres://user:pass@host:5432/dbname
#
#   <smith-db-url>    Connection string for the Smith postgres DB.
#                     Defaults to local dev: postgres://postgres:postgres@localhost:5432/postgres
#
#   --commit          Actually commit the changes. Without this flag the script
#                     runs in dry-run mode (ROLLBACK) and only prints what would change.

set -euo pipefail

DEV_SMITH_DB_URL="postgres://postgres:postgres@localhost:5432/postgres"

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

echo "==> Smith DB: $SMITH_DB_URL"
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

if [[ "$COMMIT" == "--commit" ]]; then
    FINAL_STATEMENT="COMMIT;"
    echo "==> Running in COMMIT mode."
else
    FINAL_STATEMENT="ROLLBACK;"
    echo "==> Running in DRY-RUN mode (ROLLBACK). Pass --commit to apply."
fi

echo "==> Running phase 2 deduplication on Smith DB..."

psql "$SMITH_DB_URL" <<SQL
BEGIN;

CREATE TEMP TABLE _app_api_refs (network_wifi_id int, network_smith_id int);
INSERT INTO _app_api_refs VALUES $VALUES;

-- All referenced rows with their group assignment and ref counts
CREATE TEMP TABLE _all_refs AS
SELECT
    n.id,
    n.ssid,
    n.is_network_hidden AS hidden,
    n.password,
    dense_rank() OVER (ORDER BY n.ssid, n.is_network_hidden, n.password) AS grp,
    count(*) OVER (PARTITION BY n.ssid, n.is_network_hidden, n.password) AS grp_size,
    (SELECT COUNT(*) FROM _app_api_refs a WHERE a.network_smith_id = n.id) AS app_ref_count,
    (SELECT COUNT(*) FROM device WHERE network_id = n.id) +
    (SELECT COUNT(*) FROM device WHERE current_network_id = n.id) +
    (SELECT COUNT(*) FROM device_configured_network WHERE network_id = n.id) +
    (SELECT COUNT(*) FROM device_network_intent WHERE network_id = n.id) AS smith_ref_count
FROM network n
WHERE EXISTS (SELECT 1 FROM _app_api_refs a WHERE a.network_smith_id = n.id)
   OR EXISTS (SELECT 1 FROM device WHERE network_id = n.id)
   OR EXISTS (SELECT 1 FROM device WHERE current_network_id = n.id)
   OR EXISTS (SELECT 1 FROM device_configured_network WHERE network_id = n.id)
   OR EXISTS (SELECT 1 FROM device_network_intent WHERE network_id = n.id);

-- Groups where more than one distinct row holds an App API ref: skip these
CREATE TEMP TABLE _skip_grps AS
SELECT grp
FROM _all_refs
WHERE grp_size > 1
GROUP BY grp
HAVING SUM(CASE WHEN app_ref_count > 0 THEN 1 ELSE 0 END) > 1;

-- Canonical ID per actionable duplicate group
CREATE TEMP TABLE _canonical AS
SELECT DISTINCT ON (grp)
    grp,
    id AS canonical_id
FROM _all_refs
WHERE grp_size > 1
  AND grp NOT IN (SELECT grp FROM _skip_grps)
ORDER BY
    grp,
    app_ref_count DESC,
    smith_ref_count DESC,
    id ASC;

-- Non-canonical rows that will have their FKs remapped and then be deleted
CREATE TEMP TABLE _to_remap AS
SELECT r.id AS old_id, c.canonical_id
FROM _all_refs r
JOIN _canonical c ON c.grp = r.grp
WHERE r.id != c.canonical_id;

-- Device-only rows inside skipped groups: remap to the lowest App API-referenced
-- ID in the same group. No App API writes required.
CREATE TEMP TABLE _skipped_device_remap AS
SELECT
    r.id AS old_id,
    (SELECT min(a.id) FROM _all_refs a WHERE a.grp = r.grp AND a.app_ref_count > 0) AS canonical_id
FROM _all_refs r
WHERE r.grp IN (SELECT grp FROM _skip_grps)
  AND r.app_ref_count = 0;

-- ── Report ───────────────────────────────────────────────────────────────────

\echo ''
\echo '=== SKIPPED GROUPS (App API rows kept as-is; device-only rows remapped) ==='
SELECT
    r.grp,
    r.id,
    r.ssid,
    r.hidden,
    r.password IS NOT NULL AS has_pwd,
    r.app_ref_count,
    r.smith_ref_count,
    CASE
        WHEN m.canonical_id IS NOT NULL THEN 'remap → ' || m.canonical_id::text
        ELSE 'keep (App API ref)'
    END AS action
FROM _all_refs r
LEFT JOIN _skipped_device_remap m ON m.old_id = r.id
WHERE r.grp IN (SELECT grp FROM _skip_grps)
ORDER BY r.grp, r.app_ref_count DESC, r.id;

\echo ''
\echo '=== REMAPPING PLAN (old_id → canonical_id) ==='
SELECT
    m.old_id,
    m.canonical_id,
    r.ssid,
    r.hidden,
    r.password IS NOT NULL AS has_pwd,
    r.app_ref_count AS old_app_refs,
    r.smith_ref_count AS old_smith_refs
FROM _to_remap m
JOIN _all_refs r ON r.id = m.old_id
ORDER BY m.canonical_id, m.old_id;

\echo ''
\echo '=== CANONICAL ROWS (kept, with incoming merges) ==='
SELECT
    c.canonical_id AS id,
    r.ssid,
    r.hidden,
    r.password IS NOT NULL AS has_pwd,
    r.app_ref_count,
    r.smith_ref_count,
    (SELECT string_agg(old_id::text, ',' ORDER BY old_id)
     FROM _to_remap WHERE canonical_id = c.canonical_id) AS merging_ids
FROM _canonical c
JOIN _all_refs r ON r.id = c.canonical_id
ORDER BY c.canonical_id;

-- ── Mutations ────────────────────────────────────────────────────────────────

UPDATE device SET network_id = r.canonical_id
FROM _to_remap r WHERE device.network_id = r.old_id;

UPDATE device SET current_network_id = r.canonical_id
FROM _to_remap r WHERE device.current_network_id = r.old_id;

UPDATE device_configured_network SET network_id = r.canonical_id
FROM _to_remap r WHERE device_configured_network.network_id = r.old_id;

UPDATE device_network_intent SET network_id = r.canonical_id
FROM _to_remap r WHERE device_network_intent.network_id = r.old_id;

UPDATE device SET network_id = m.canonical_id
FROM _skipped_device_remap m WHERE device.network_id = m.old_id;

UPDATE device SET current_network_id = m.canonical_id
FROM _skipped_device_remap m WHERE device.current_network_id = m.old_id;

UPDATE device_configured_network SET network_id = m.canonical_id
FROM _skipped_device_remap m WHERE device_configured_network.network_id = m.old_id;

UPDATE device_network_intent SET network_id = m.canonical_id
FROM _skipped_device_remap m WHERE device_network_intent.network_id = m.old_id;

DELETE FROM network WHERE id IN (SELECT old_id FROM _to_remap)
   OR id IN (SELECT old_id FROM _skipped_device_remap);

\echo ''
SELECT COUNT(*) AS remaining_rows FROM network;

$FINAL_STATEMENT
SQL

echo "==> Done."
