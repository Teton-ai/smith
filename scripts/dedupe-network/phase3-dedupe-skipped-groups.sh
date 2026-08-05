#!/usr/bin/env bash
# Phase 3: Collapse the groups phase 2 skipped, by repointing the App API bridge
# rows at one canonical Smith id. This writes to a database Smith does not own.
#
# See README.md for the bridge classification, canonical selection, the ordering
# of the two non-atomic commits, and why every payload is streamed.
#
# Run phases 1 and 2 before this script.
#
# Usage: ./phase3-dedupe-skipped-groups.sh [<app-api-db-url>] [<smith-db-url>] [--commit]
#
#   <app-api-db-url>  Connection string for the App API postgres DB (read + write).
#                     Defaults to local dev: postgres://postgres:postgres@localhost:5433/postgres
#                     (the DB spun up by setup-test-app-api-db.sh)
#
#   <smith-db-url>    Smith postgres DB. Connection string or docker://<container>.
#                     Defaults to docker://smith-postgres.
#
#   --commit          Apply. Without it the script dry-runs (ROLLBACK on both).

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

DEV_APP_API_DB_URL="postgres://postgres:postgres@localhost:5433/postgres"
DEV_SMITH_DB_URL="docker://smith-postgres"

# Parse args: both DBs are optional and default to dev; --commit can appear anywhere
APP_API_DB_URL="$DEV_APP_API_DB_URL"
SMITH_DB_URL="$DEV_SMITH_DB_URL"
COMMIT=""
_APP_API_SET=""

for arg in "$@"; do
    if [[ "$arg" == "--commit" ]]; then
        COMMIT="--commit"
    elif [[ -z "$_APP_API_SET" ]]; then
        APP_API_DB_URL="$arg"
        _APP_API_SET=1
    else
        SMITH_DB_URL="$arg"
    fi
done

smith_psql() { db_psql "$SMITH_DB_URL" "$@"; }
app_api_psql() { db_psql "$APP_API_DB_URL" "$@"; }

echo "==> Smith DB:   $SMITH_DB_URL"
echo "==> App API DB: $APP_API_DB_URL"

echo "==> Fetching App API network_smith refs..."
fetch_app_api_refs "$APP_API_DB_URL"
echo "==> Got $APP_API_REF_COUNT App API refs."

echo "==> Fetching App API wifi/department ownership..."
fetch_app_api_wifi_depts "$APP_API_DB_URL"

echo "==> Fetching App API device/department ownership..."
fetch_app_api_device_depts "$APP_API_DB_URL"
echo "==> Got $APP_API_DEV_DEPT_COUNT App API devices."

# Emitted as a unit so every query sees the same inputs.
emit_inputs() {
    cat <<'SQL'
CREATE TEMP TABLE _refs (network_wifi_id int, network_smith_id int);
SQL
    copy_block "_refs (network_wifi_id, network_smith_id)" "$APP_API_REF_CSV"
    cat <<'SQL'
CREATE TEMP TABLE _wifi_dept (network_wifi_id int, department_id int);
SQL
    copy_block "_wifi_dept (network_wifi_id, department_id)" "$APP_API_WIFI_DEPT_CSV"
    cat <<'SQL'
CREATE TEMP TABLE _dev_dept (serial_number text, department_id int);
SQL
    copy_block "_dev_dept (serial_number, department_id)" "$APP_API_DEV_DEPT_CSV"
}

# ── Bridge classification ───────────────────────────────────────────────────

echo "==> Classifying App API bridges against device evidence..."
CLASS_TSV=$(
    {
        emit_inputs
        emit_referenced
        cat <<'SQL'
-- device_network_intent is excluded: intent, not evidence.
CREATE TEMP VIEW _dept_grp AS
SELECT DISTINCT dd.department_id, g.grp
FROM _dev_dept dd
JOIN device d ON d.serial_number = dd.serial_number
JOIN _referenced g ON g.id IN (d.network_id, d.current_network_id)
UNION
SELECT DISTINCT dd.department_id, g.grp
FROM _dev_dept dd
JOIN device d ON d.serial_number = dd.serial_number
JOIN device_configured_network c ON c.device_id = d.id
JOIN _referenced g ON g.id = c.network_id;

SELECT b.network_wifi_id, b.network_smith_id, g.grp,
       CASE
           WHEN wd.department_id IS NULL THEN 'UNKNOWN'
           WHEN NOT EXISTS (SELECT 1 FROM _dept_grp x
                            WHERE x.department_id = wd.department_id) THEN 'UNKNOWN'
           WHEN EXISTS (SELECT 1 FROM _dept_grp x
                        WHERE x.department_id = wd.department_id AND x.grp = g.grp) THEN 'CONFIRMED'
           ELSE 'UNSUPPORTED'
       END
FROM _refs b
JOIN _referenced g ON g.id = b.network_smith_id
LEFT JOIN _wifi_dept wd ON wd.network_wifi_id = b.network_wifi_id
ORDER BY g.grp, b.network_wifi_id;
SQL
    } | smith_psql -q -t -A -F $'\t' -v ON_ERROR_STOP=1
)

if [[ -z "$CLASS_TSV" ]]; then
    echo "ERROR: classification returned no rows; refusing to continue." >&2
    exit 1
fi

if printf '%s\n' "$CLASS_TSV" | grep -qvE '^[0-9]+'$'\t''[0-9]+'$'\t''[0-9]+'$'\t''(CONFIRMED|UNKNOWN|UNSUPPORTED)$'; then
    echo "ERROR: classification produced an unexpected row shape; refusing to continue." >&2
    exit 1
fi

CLASS_CSV=$(printf '%s\n' "$CLASS_TSV" | tr '\t' ',')

for c in CONFIRMED UNKNOWN UNSUPPORTED; do
    printf '==> %-12s %s bridges\n' "$c" \
        "$(printf '%s\n' "$CLASS_TSV" | grep -cE $'\t'"$c\$" || true)"
done

# ── Remap ───────────────────────────────────────────────────────────────────

echo "==> Computing skipped group remapping from Smith DB..."
REMAP_TSV=$(
    {
        emit_inputs
        emit_referenced
        cat <<'SQL'
CREATE TEMP TABLE _class (network_wifi_id int, network_smith_id int, grp int, class text);
SQL
        copy_block "_class (network_wifi_id, network_smith_id, grp, class)" "$CLASS_CSV"
        cat <<'SQL'
WITH
skip_grps AS (
    SELECT grp
    FROM _referenced
    WHERE grp_size > 1
    GROUP BY grp
    HAVING SUM(CASE WHEN app_refs > 0 THEN 1 ELSE 0 END) > 1
),
-- Rows carrying an UNSUPPORTED bridge stay. See README.md.
blocked AS (
    SELECT DISTINCT network_smith_id AS id FROM _class WHERE class = 'UNSUPPORTED'
),
canonical AS (
    SELECT DISTINCT ON (grp) grp, id AS canonical_id
    FROM _referenced
    WHERE grp IN (SELECT grp FROM skip_grps) AND app_refs > 0
    ORDER BY grp, smith_refs DESC, id ASC
)
SELECT r.id::text, c.canonical_id::text
FROM _referenced r
JOIN canonical c ON c.grp = r.grp
WHERE r.grp IN (SELECT grp FROM skip_grps)
  AND r.app_refs > 0
  AND r.id != c.canonical_id
  AND r.id NOT IN (SELECT id FROM blocked);
SQL
    } | smith_psql -q -t -A -F $'\t' -v ON_ERROR_STOP=1
)

if [[ -z "$REMAP_TSV" ]]; then
    echo "==> Nothing to remap. Every skipped group is either already collapsed"
    echo "    or held back by an UNSUPPORTED bridge."
    exit 0
fi

validate_int_pairs "$REMAP_TSV"

REMAP_COUNT=$(printf '%s\n' "$REMAP_TSV" | wc -l | tr -d ' ')
echo "==> $REMAP_COUNT non-canonical Smith rows to remap."

REMAP_CSV=$(printf '%s\n' "$REMAP_TSV" | tr '\t' ',')

if [[ "$COMMIT" == "--commit" ]]; then
    FINAL_STATEMENT="COMMIT;"
    echo "==> Running in COMMIT mode."
else
    FINAL_STATEMENT="ROLLBACK;"
    echo "==> Running in DRY-RUN mode (ROLLBACK). Pass --commit to apply."
fi

# ── Report. Printed before either database is touched. ─────────────────────

echo "==> Phase 3 plan:"
{
    cat <<'SQL'
BEGIN;
SQL
    emit_inputs
    emit_referenced
    cat <<'SQL'
CREATE TEMP TABLE _class (network_wifi_id int, network_smith_id int, grp int, class text);
SQL
    copy_block "_class (network_wifi_id, network_smith_id, grp, class)" "$CLASS_CSV"
    cat <<'SQL'
CREATE TEMP TABLE _to_remap (old_id int, canonical_id int);
SQL
    copy_block "_to_remap (old_id, canonical_id)" "$REMAP_CSV"
    cat <<'SQL'

CREATE TEMP VIEW _skip_grps AS
SELECT grp FROM _referenced
WHERE grp_size > 1
GROUP BY grp
HAVING SUM(CASE WHEN app_refs > 0 THEN 1 ELSE 0 END) > 1;

CREATE TEMP VIEW _plan AS
SELECT g.id, g.grp, g.ssid, g.hidden, g.type, g.sec, g.ident,
       (g.psk IS NOT NULL) AS psk, g.app_refs, g.smith_refs,
       (SELECT string_agg(DISTINCT c.class, '+' ORDER BY c.class)
        FROM _class c WHERE c.network_smith_id = g.id) AS bridge_class
FROM _referenced g
WHERE g.grp IN (SELECT grp FROM _skip_grps) AND g.app_refs > 0;

\echo ''
\echo '=== SKIPPED GROUPS: full remap plan ==='
SELECT p.grp, p.id, p.ssid, p.hidden, p.type, p.sec, p.ident, p.psk,
       p.app_refs, p.smith_refs, p.bridge_class,
       CASE WHEN m.canonical_id IS NOT NULL THEN 'remap -> ' || m.canonical_id::text
            WHEN p.bridge_class LIKE '%UNSUPPORTED%' THEN 'HELD BACK (unsupported bridge)'
            ELSE 'keep (canonical)'
       END AS action
FROM _plan p
LEFT JOIN _to_remap m ON m.old_id = p.id
ORDER BY p.grp, CASE WHEN m.canonical_id IS NULL THEN 0 ELSE 1 END, p.smith_refs DESC, p.id;

\echo ''
\echo '=== UNSUPPORTED BRIDGES: the owning department has devices, none on this group ==='
\echo '(none of these can be fixed here; the repair has to leave the content group,'
\echo ' which no Smith-side write can express. blocks_merge marks the ones that also'
\echo ' stop a phase 3 group from collapsing)'
SELECT * FROM (
    SELECT c.grp, c.network_wifi_id, c.network_smith_id AS points_at, wd.department_id,
           (SELECT n.ssid FROM network n WHERE n.id = c.network_smith_id) AS smith_ssid,
           (c.grp IN (SELECT grp FROM _skip_grps)) AS blocks_merge
    FROM _class c
    LEFT JOIN _wifi_dept wd ON wd.network_wifi_id = c.network_wifi_id
    WHERE c.class = 'UNSUPPORTED'
) u ORDER BY u.blocks_merge DESC, u.grp, u.network_wifi_id;

\echo ''
\echo '=== FK rows this plan will repoint ==='
SELECT
    (SELECT COUNT(*) FROM device d JOIN _to_remap r ON d.network_id = r.old_id) AS assigned,
    (SELECT COUNT(*) FROM device d JOIN _to_remap r ON d.current_network_id = r.old_id) AS current,
    (SELECT COUNT(*) FROM device_configured_network x JOIN _to_remap r ON x.network_id = r.old_id) AS profiles,
    (SELECT COUNT(*) FROM device_network_intent x JOIN _to_remap r ON x.network_id = r.old_id) AS intents,
    (SELECT COUNT(*) FROM network_reference x JOIN _to_remap r ON x.network_id = r.old_id) AS refs;

ROLLBACK;
SQL
} | smith_psql -v ON_ERROR_STOP=1

# ── App API update. Goes FIRST on purpose; see README.md. ───────────────────

echo "==> Running phase 3 on App API DB..."
{
    cat <<'SQL'
BEGIN;

CREATE TEMP TABLE _to_remap (old_id int, canonical_id int);
SQL
    copy_block "_to_remap (old_id, canonical_id)" "$REMAP_CSV"
    cat <<'SQL'

\echo ''
\echo '=== App API network_smith rows to update ==='
SELECT
    ns.id AS network_smith_row_id,
    ns.network_wifi_id,
    ns.network_smith_id AS old_smith_id,
    m.canonical_id AS new_smith_id
FROM network_smith ns
JOIN _to_remap m ON ns.network_smith_id = m.old_id::text
ORDER BY ns.network_wifi_id;

UPDATE network_smith
SET network_smith_id = m.canonical_id::text
FROM _to_remap m
WHERE network_smith.network_smith_id = m.old_id::text;

-- Abort rather than report: the Smith step deletes these rows next.
DO $$
DECLARE stale int;
BEGIN
    SELECT COUNT(*) INTO stale FROM network_smith ns
    JOIN _to_remap m ON ns.network_smith_id = m.old_id::text;
    IF stale > 0 THEN
        RAISE EXCEPTION 'aborting: % bridges still point at rows the Smith step deletes', stale;
    END IF;
END $$;
SQL
    printf '%s\n' "$FINAL_STATEMENT"
} | app_api_psql -v ON_ERROR_STOP=1

# ── Smith mutations ─────────────────────────────────────────────────────────
# Refs re-fetched after the App API commit so the guard below sees anything the
# sync job bridged since the run started, as phase 4 does before its delete.

ORIG_REF_CSV="$APP_API_REF_CSV"
echo "==> Re-fetching refs before the Smith step..."
refresh_app_api_refs "$APP_API_DB_URL"

echo "==> Running phase 3 on Smith DB..."
{
    cat <<'SQL'
BEGIN;

CREATE TEMP TABLE _to_remap (old_id int, canonical_id int);
SQL
    copy_block "_to_remap (old_id, canonical_id)" "$REMAP_CSV"
    cat <<'SQL'
CREATE TEMP TABLE _refs (network_wifi_id int, network_smith_id int);
SQL
    copy_block "_refs (network_wifi_id, network_smith_id)" "$APP_API_REF_CSV"
    cat <<'SQL'
CREATE TEMP TABLE _orig_refs (network_wifi_id int, network_smith_id int);
SQL
    copy_block "_orig_refs (network_wifi_id, network_smith_id)" "$ORIG_REF_CSV"
    cat <<'SQL'

-- The App API UPDATE moved every bridge this run knew about off its old row, so
-- a ref still on an old row that the run never saw is one the sync job created
-- in between. Deleting that row would strand it, so drop the whole remap for it.
CREATE TEMP TABLE _blocked AS
SELECT DISTINCT r.old_id
FROM _to_remap r
WHERE EXISTS (
    SELECT 1 FROM _refs a
    WHERE a.network_smith_id = r.old_id
      AND a.network_wifi_id NOT IN (SELECT o.network_wifi_id FROM _orig_refs o
                                    WHERE o.network_smith_id = r.old_id));

\echo ''
\echo '=== HELD BACK: a new bridge landed on these rows after the App API commit ==='
SELECT b.old_id, n.ssid FROM _blocked b JOIN network n ON n.id = b.old_id ORDER BY b.old_id;

DELETE FROM _to_remap WHERE old_id IN (SELECT old_id FROM _blocked);

-- device_network_intent is UNIQUE (device_id, network_id), so remapping would
-- abort where a device holds intents on several rows of one group. Only rows
-- being remapped are dropped, so an intent already on the canonical row keeps
-- its priority; among remapped ones the oldest survives.
DELETE FROM device_network_intent i
USING _to_remap r
WHERE i.network_id = r.old_id
  AND EXISTS (
      SELECT 1
      FROM device_network_intent j
      LEFT JOIN _to_remap r2 ON j.network_id = r2.old_id
      WHERE j.device_id = i.device_id
        AND coalesce(r2.canonical_id, j.network_id) = r.canonical_id
        AND (j.network_id = r.canonical_id OR j.id < i.id));

UPDATE device SET network_id = r.canonical_id
FROM _to_remap r WHERE device.network_id = r.old_id;

UPDATE device SET current_network_id = r.canonical_id
FROM _to_remap r WHERE device.current_network_id = r.old_id;

UPDATE device_configured_network SET network_id = r.canonical_id
FROM _to_remap r WHERE device_configured_network.network_id = r.old_id;

UPDATE device_network_intent SET network_id = r.canonical_id
FROM _to_remap r WHERE device_network_intent.network_id = r.old_id;

-- ON DELETE RESTRICT, and the PK can collide. See README.md.
INSERT INTO network_reference (holder, external_key, network_id)
SELECT nr.holder, nr.external_key, r.canonical_id
FROM network_reference nr JOIN _to_remap r ON nr.network_id = r.old_id
ON CONFLICT DO NOTHING;

DELETE FROM network_reference nr USING _to_remap r WHERE nr.network_id = r.old_id;

-- Keys outside psk would be dropped with the row. See README.md.
CREATE TEMP TABLE _creds_fold AS
SELECT r.canonical_id, jsonb_object_agg(e.key, e.value ORDER BY o.id) AS merged
FROM _to_remap r
JOIN network o ON o.id = r.old_id
CROSS JOIN LATERAL jsonb_each(o.credentials) e
GROUP BY r.canonical_id;

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

DELETE FROM network WHERE id IN (SELECT old_id FROM _to_remap);

\echo ''
SELECT COUNT(*) AS remaining_rows FROM network;
SQL
    printf '%s\n' "$FINAL_STATEMENT"
} | smith_psql -v ON_ERROR_STOP=1

echo "==> Done."
