#!/usr/bin/env bash
# Phase 4: Repair the App API bridges phase 3 held back as UNSUPPORTED, then
# delete the Smith rows that repair leaves unreferenced.
#
# See README.md for the resolution test, why unresolvable bridges are left
# alone, and the ordering of the two non-atomic commits.
#
# Run phases 1, 2 and 3 before this script.
#
# Usage: ./phase4-repair-stale-bridges.sh [<app-api-db-url>] [<smith-db-url>] [--commit]
#
#   <app-api-db-url>  Connection string for the App API postgres DB (read + write).
#                     Defaults to local dev: postgres://postgres:postgres@localhost:5433/postgres
#
#   <smith-db-url>    Smith postgres DB. Connection string or docker://<container>.
#                     Defaults to docker://smith-postgres.
#
#   --commit          Apply. Without it the script dry-runs (ROLLBACK on both).

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

DEV_APP_API_DB_URL="postgres://postgres:postgres@localhost:5433/postgres"
DEV_SMITH_DB_URL="docker://smith-postgres"

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

smith_psql() { db_psql "$SMITH_DB_URL" "$@"; }
app_api_psql() { db_psql "$APP_API_DB_URL" "$@"; }

echo "==> Smith DB:   $SMITH_DB_URL"
echo "==> App API DB: $APP_API_DB_URL"

# Before the ref fetch, deliberately.
WATERMARK=$(smith_watermark "$SMITH_DB_URL")
echo "==> Watermark: network.id <= $WATERMARK"

echo "==> Fetching App API bridge, ownership and content..."
fetch_app_api_refs "$APP_API_DB_URL"
fetch_app_api_wifi_depts "$APP_API_DB_URL"
fetch_app_api_device_depts "$APP_API_DB_URL"
fetch_app_api_wifi_content "$APP_API_DB_URL"
echo "==> $APP_API_REF_COUNT bridges, $APP_API_DEV_DEPT_COUNT devices."

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
    cat <<'SQL'
CREATE TEMP TABLE _wifi (network_wifi_id int, ssid text, hidden boolean, psk text);
SQL
    copy_block "_wifi (network_wifi_id, ssid, hidden, psk)" "$APP_API_WIFI_CONTENT_CSV"
}

# Phase-4-specific views. Requires _referenced from _common.sh.
emit_model() {
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

CREATE TEMP VIEW _unsupported AS
SELECT b.network_wifi_id AS wid, b.network_smith_id AS sid, wd.department_id AS dept
FROM _refs b
JOIN _referenced g ON g.id = b.network_smith_id
JOIN _wifi_dept wd ON wd.network_wifi_id = b.network_wifi_id
WHERE EXISTS (SELECT 1 FROM _dept_grp x WHERE x.department_id = wd.department_id)
  AND NOT EXISTS (SELECT 1 FROM _dept_grp x
                  WHERE x.department_id = wd.department_id AND x.grp = g.grp);

-- A destination needs two independent things to agree: the App API record's own
-- content matches the group, AND the department's devices are on it.
CREATE TEMP VIEW _candidate AS
SELECT DISTINCT u.wid, u.sid, x.grp
FROM _unsupported u
JOIN _wifi w ON w.network_wifi_id = u.wid
JOIN _dept_grp x ON x.department_id = u.dept
WHERE EXISTS (SELECT 1 FROM _referenced g
              WHERE g.grp = x.grp AND g.ssid = w.ssid AND g.hidden = w.hidden
                AND g.psk IS NOT DISTINCT FROM w.psk);

-- Not phase 3's canonical rule: that one picks only among bridged rows because
-- every candidate there holds a bridge. A phase 4 destination group may hold no
-- bridge at all, so the whole group is in play.
CREATE TEMP VIEW _repair AS
SELECT c.wid, c.sid AS old_id,
       (SELECT id FROM _referenced x WHERE x.grp = c.grp
        ORDER BY x.smith_refs DESC, x.id LIMIT 1) AS new_id
FROM _candidate c
WHERE c.wid IN (SELECT wid FROM _candidate GROUP BY wid HAVING count(*) = 1);
SQL
}

# Read-only, and emitted from _repair rather than _to_repair so it works before
# the repair plan exists. These bridges are what phase 4 exists to surface;
# going silent once everything resolvable is done would hide them entirely.
emit_unresolvable_report() {
    cat <<'SQL'

\echo ''
\echo '=== UNRESOLVABLE: left as-is, no computable destination ==='
SELECT u.wid AS network_wifi_id, w.ssid AS record_says, u.sid AS points_at,
       (SELECT string_agg(DISTINCT gg.ssid, ' | ')
        FROM _dept_grp x JOIN _referenced gg ON gg.grp = x.grp
        WHERE x.department_id = u.dept) AS devices_actually_on,
       (SELECT COUNT(*) FROM _candidate c WHERE c.wid = u.wid) AS candidate_groups
FROM _unsupported u
JOIN _wifi w ON w.network_wifi_id = u.wid
WHERE u.wid NOT IN (SELECT wid FROM _repair WHERE old_id <> new_id)
ORDER BY u.wid;
SQL
}

echo "==> Resolving unsupported bridges..."
REPAIR_TSV=$(
    {
        emit_inputs
        emit_referenced
        emit_model
        cat <<'SQL'
SELECT wid::text, old_id::text, new_id::text FROM _repair WHERE old_id <> new_id ORDER BY wid;
SQL
    } | smith_psql -q -t -A -F $'\t' -v ON_ERROR_STOP=1
)

if [[ -z "$REPAIR_TSV" ]]; then
    echo "==> Nothing resolvable. The unsupported bridges that remain:"
    {
        emit_inputs
        emit_referenced
        emit_model
        emit_unresolvable_report
    } | smith_psql -v ON_ERROR_STOP=1
    exit 0
fi

if printf '%s\n' "$REPAIR_TSV" | grep -qvE '^[0-9]+'$'\t''[0-9]+'$'\t''[0-9]+$'; then
    echo "ERROR: repair plan has an unexpected row shape; refusing to continue." >&2
    exit 1
fi

REPAIR_CSV=$(printf '%s\n' "$REPAIR_TSV" | tr '\t' ',')
REPAIR_BRIDGE_COUNT=$(printf '%s\n' "$REPAIR_TSV" | wc -l | tr -d ' ')
echo "==> $REPAIR_BRIDGE_COUNT bridges to repoint."

if [[ "$COMMIT" == "--commit" ]]; then
    FINAL_STATEMENT="COMMIT;"
    echo "==> Running in COMMIT mode."
else
    FINAL_STATEMENT="ROLLBACK;"
    echo "==> Running in DRY-RUN mode (ROLLBACK). Pass --commit to apply."
fi

# ── Report. Printed before either database is touched. ─────────────────────

echo "==> Phase 4 plan:"
{
    cat <<'SQL'
BEGIN;
SQL
    emit_inputs
    emit_referenced
    emit_model
    cat <<'SQL'
CREATE TEMP TABLE _to_repair (wid int, old_id int, new_id int);
SQL
    copy_block "_to_repair (wid, old_id, new_id)" "$REPAIR_CSV"
    cat <<'SQL'

\echo ''
\echo '=== BRIDGES TO REPOINT ==='
SELECT r.wid AS network_wifi_id, w.ssid AS record_says, r.old_id AS points_at,
       (SELECT ssid FROM network n WHERE n.id = r.old_id) AS old_ssid,
       r.new_id AS repoint_to
FROM _to_repair r JOIN _wifi w ON w.network_wifi_id = r.wid
ORDER BY r.wid;

\echo ''
\echo '=== SMITH ROWS THIS FREES (deleted below if nothing else points at them) ==='
SELECT g.id, g.ssid, g.smith_refs,
       (SELECT COUNT(*) FROM _refs a WHERE a.network_smith_id = g.id) -
       (SELECT COUNT(*) FROM _to_repair r WHERE r.old_id = g.id) AS bridges_after
FROM _referenced g
WHERE g.id IN (SELECT old_id FROM _to_repair)
ORDER BY g.id;
SQL
    emit_unresolvable_report
    cat <<'SQL'

ROLLBACK;
SQL
} | smith_psql -v ON_ERROR_STOP=1

# ── App API update. Goes FIRST on purpose; see README.md. ───────────────────

echo "==> Repointing bridges on App API DB..."
{
    cat <<'SQL'
BEGIN;

CREATE TEMP TABLE _to_repair (wid int, old_id int, new_id int);
SQL
    copy_block "_to_repair (wid, old_id, new_id)" "$REPAIR_CSV"
    cat <<'SQL'

UPDATE network_smith ns
SET network_smith_id = r.new_id::text
FROM _to_repair r
WHERE ns.network_wifi_id = r.wid AND ns.network_smith_id = r.old_id::text;

-- Abort rather than report: the Smith step deletes these rows next, so a bridge
-- left on an old id would be stranded by that delete.
DO $$
DECLARE stale int;
BEGIN
    SELECT COUNT(*) INTO stale FROM network_smith ns
    JOIN _to_repair r ON r.wid = ns.network_wifi_id AND ns.network_smith_id = r.old_id::text;
    IF stale > 0 THEN
        RAISE EXCEPTION 'aborting: % bridges did not move off their old row', stale;
    END IF;
END $$;
SQL
    printf '%s\n' "$FINAL_STATEMENT"
} | app_api_psql -v ON_ERROR_STOP=1

# ── Smith delete ───────────────────────────────────────────────────────────
# Refs re-fetched after the App API commit so the guard sees the repointing and
# anything the sync job bridged since the run started.

echo "==> Re-fetching refs before the delete..."
refresh_app_api_refs "$APP_API_DB_URL"

echo "==> Deleting rows the repair left unreferenced..."
{
    cat <<'SQL'
BEGIN;

CREATE TEMP TABLE _refs (network_wifi_id int, network_smith_id int);
SQL
    copy_block "_refs (network_wifi_id, network_smith_id)" "$APP_API_REF_CSV"
    cat <<'SQL'
CREATE TEMP TABLE _to_repair (wid int, old_id int, new_id int);
SQL
    copy_block "_to_repair (wid, old_id, new_id)" "$REPAIR_CSV"
    cat <<SQL

-- Scoped to the rows this run freed; a general sweep is phase 1's job. Only the
-- bridge leaving this row is excluded from the guard, never every repaired
-- bridge: a row that simultaneously receives one must still count as referenced.
-- Excluding rather than reading back keeps the dry run identical to a commit.
CREATE TEMP TABLE _dead AS
SELECT DISTINCT r.old_id AS id
FROM _to_repair r
WHERE r.old_id <= $WATERMARK
  AND NOT EXISTS (SELECT 1 FROM _refs a WHERE a.network_smith_id = r.old_id
                    AND a.network_wifi_id <> r.wid)
  AND NOT EXISTS (SELECT 1 FROM device WHERE network_id = r.old_id)
  AND NOT EXISTS (SELECT 1 FROM device WHERE current_network_id = r.old_id)
  AND NOT EXISTS (SELECT 1 FROM device_configured_network WHERE network_id = r.old_id)
  AND NOT EXISTS (SELECT 1 FROM device_network_intent WHERE network_id = r.old_id)
  AND NOT EXISTS (SELECT 1 FROM network_reference WHERE network_id = r.old_id);
SQL
    cat <<'SQL'

\echo ''
\echo '=== ROWS DELETED ==='
SELECT d.id, n.ssid FROM _dead d JOIN network n ON n.id = d.id ORDER BY d.id;

DELETE FROM network WHERE id IN (SELECT id FROM _dead);

\echo ''
SELECT COUNT(*) AS remaining_rows FROM network;
SQL
    printf '%s\n' "$FINAL_STATEMENT"
} | smith_psql -v ON_ERROR_STOP=1

echo "==> Done."
