#!/usr/bin/env bash
# Phase 2: Merge duplicate referenced network rows onto one canonical row.
# See README.md for the content key, canonical selection, and skipped groups.
#
# Usage: ./phase2-dedupe-referenced.sh <app-api-db-url> [<smith-db-url>] [--commit]
#
#   <app-api-db-url>  Connection string for the App API postgres DB (read-only)
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
echo "==> Fetching App API network_smith refs..."
fetch_app_api_refs "$APP_API_DB_URL"
echo "==> Got $APP_API_REF_COUNT App API refs."

if [[ "$COMMIT" == "--commit" ]]; then
    FINAL_STATEMENT="COMMIT;"
    echo "==> Running in COMMIT mode."
else
    FINAL_STATEMENT="ROLLBACK;"
    echo "==> Running in DRY-RUN mode (ROLLBACK). Pass --commit to apply."
fi

echo "==> Running phase 2 deduplication on Smith DB..."

{
    cat <<'SQL'
BEGIN;

CREATE TEMP TABLE _refs (network_wifi_id int, network_smith_id int);
SQL
    copy_block "_refs (network_wifi_id, network_smith_id)" "$APP_API_REF_CSV"
    emit_referenced
    cat <<'SQL'

-- Left to phase 3: collapsing these needs App API writes.
CREATE TEMP TABLE _skip_grps AS
SELECT grp
FROM _referenced
WHERE grp_size > 1
GROUP BY grp
HAVING SUM(CASE WHEN app_refs > 0 THEN 1 ELSE 0 END) > 1;

CREATE TEMP TABLE _canonical AS
SELECT DISTINCT ON (grp)
    grp,
    id AS canonical_id
FROM _referenced
WHERE grp_size > 1
  AND grp NOT IN (SELECT grp FROM _skip_grps)
ORDER BY
    grp,
    app_refs DESC,
    smith_refs DESC,
    id ASC;

CREATE TEMP TABLE _to_remap AS
SELECT r.id AS old_id, c.canonical_id
FROM _referenced r
JOIN _canonical c ON c.grp = r.grp
WHERE r.id != c.canonical_id;

-- Device-only rows inside skipped groups: no App API writes required. Any
-- bridged row in the group is equally valid, so the target is ordered the way
-- phase 3 orders its canonical. Parking on the lowest id instead would inflate
-- an arbitrary row, and once that row outweighed the group's real leader phase 3
-- would canonicalize on it.
CREATE TEMP TABLE _skipped_device_remap AS
SELECT
    r.id AS old_id,
    (SELECT a.id FROM _referenced a WHERE a.grp = r.grp AND a.app_refs > 0
     ORDER BY a.smith_refs DESC, a.id ASC LIMIT 1) AS canonical_id
FROM _referenced r
WHERE r.grp IN (SELECT grp FROM _skip_grps)
  AND r.app_refs = 0;

-- ── Report ───────────────────────────────────────────────────────────────────

\echo ''
\echo '=== SKIPPED GROUPS (App API rows kept as-is; device-only rows remapped) ==='
SELECT
    r.grp,
    r.id,
    r.ssid,
    r.hidden,
    r.type,
    r.sec,
    r.ident,
    (r.psk IS NOT NULL) AS psk,
    r.app_refs,
    r.smith_refs,
    CASE
        WHEN m.canonical_id IS NOT NULL THEN 'remap → ' || m.canonical_id::text
        ELSE 'keep (App API ref)'
    END AS action
FROM _referenced r
LEFT JOIN _skipped_device_remap m ON m.old_id = r.id
WHERE r.grp IN (SELECT grp FROM _skip_grps)
ORDER BY r.grp, r.app_refs DESC, r.id;

\echo ''
\echo '=== REMAPPING PLAN (old_id → canonical_id) ==='
SELECT
    m.old_id,
    m.canonical_id,
    r.ssid,
    r.hidden,
    r.type,
    r.sec,
    r.ident,
    (r.psk IS NOT NULL) AS psk,
    r.app_refs AS old_app_refs,
    r.smith_refs AS old_smith_refs
FROM _to_remap m
JOIN _referenced r ON r.id = m.old_id
ORDER BY m.canonical_id, m.old_id;

\echo ''
\echo '=== CANONICAL ROWS (kept, with incoming merges) ==='
SELECT
    c.canonical_id AS id,
    r.ssid,
    r.hidden,
    r.type,
    r.sec,
    r.ident,
    (r.psk IS NOT NULL) AS psk,
    r.app_refs,
    r.smith_refs,
    (SELECT string_agg(old_id::text, ',' ORDER BY old_id)
     FROM _to_remap WHERE canonical_id = c.canonical_id) AS merging_ids
FROM _canonical c
JOIN _referenced r ON r.id = c.canonical_id
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

-- ON DELETE RESTRICT, and the PK can collide. See README.md.
INSERT INTO network_reference (holder, external_key, network_id)
SELECT nr.holder, nr.external_key, r.canonical_id
FROM network_reference nr JOIN _to_remap r ON nr.network_id = r.old_id
ON CONFLICT DO NOTHING;

DELETE FROM network_reference nr USING _to_remap r WHERE nr.network_id = r.old_id;

INSERT INTO network_reference (holder, external_key, network_id)
SELECT nr.holder, nr.external_key, m.canonical_id
FROM network_reference nr JOIN _skipped_device_remap m ON nr.network_id = m.old_id
ON CONFLICT DO NOTHING;

DELETE FROM network_reference nr USING _skipped_device_remap m WHERE nr.network_id = m.old_id;

-- Keys outside psk would be dropped with the row. See README.md.
CREATE TEMP TABLE _creds_fold AS
SELECT canonical_id, jsonb_object_agg(e.key, e.value ORDER BY o.id) AS merged
FROM (
    SELECT old_id, canonical_id FROM _to_remap
    UNION ALL
    SELECT old_id, canonical_id FROM _skipped_device_remap
) r
JOIN network o ON o.id = r.old_id
CROSS JOIN LATERAL jsonb_each(o.credentials) e
GROUP BY canonical_id;

\echo ''
\echo '=== CREDENTIALS FOLDED INTO SURVIVORS (keys gained) ==='
SELECT f.canonical_id AS id, n.ssid,
       (SELECT string_agg(k, ',' ORDER BY k)
        FROM jsonb_object_keys(f.merged) k
        WHERE NOT n.credentials ? k) AS gained
FROM _creds_fold f JOIN network n ON n.id = f.canonical_id
WHERE EXISTS (SELECT 1 FROM jsonb_object_keys(f.merged) k WHERE NOT n.credentials ? k)
ORDER BY f.canonical_id;

UPDATE network k SET credentials = f.merged || k.credentials
FROM _creds_fold f WHERE k.id = f.canonical_id;

DELETE FROM network WHERE id IN (SELECT old_id FROM _to_remap)
   OR id IN (SELECT old_id FROM _skipped_device_remap);

\echo ''
SELECT COUNT(*) AS remaining_rows FROM network;

SQL
    printf '%s\n' "$FINAL_STATEMENT"
} | smith_psql -v ON_ERROR_STOP=1

echo "==> Done."
