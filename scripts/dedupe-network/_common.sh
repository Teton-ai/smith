#!/usr/bin/env bash
# Shared helpers for the dedupe-network scripts. Sourced, not executed.
# shellcheck shell=bash

# psql against a connection string or docker://<container>[/<database>].
# See README.md for why the docker form exists.
db_psql() {
    local url="$1"
    shift
    if [[ "$url" == docker://* ]]; then
        local rest="${url#docker://}"
        local container="${rest%%/*}"
        local dbname="postgres"
        [[ "$rest" == */* ]] && dbname="${rest#*/}"
        [[ -z "$dbname" ]] && dbname="postgres"
        docker exec -i "$container" psql -U postgres -d "$dbname" "$@"
    else
        psql "$url" "$@"
    fi
}

# Sets APP_API_REF_CSV and APP_API_REF_COUNT. network_smith_id is TEXT, hence
# the integer validation.
fetch_app_api_refs() {
    local url="$1"
    local rows
    rows=$(db_psql "$url" -t -A -F $'\t' -c \
        "SELECT network_wifi_id, network_smith_id FROM network_smith WHERE network_smith_id IS NOT NULL ORDER BY network_wifi_id;")

    if [[ -z "$rows" ]]; then
        echo "ERROR: no rows returned from App API network_smith table" >&2
        return 1
    fi

    if printf '%s\n' "$rows" | grep -qvE '^[0-9]+'$'\t''[0-9]+$'; then
        echo "ERROR: network_smith returned a non-integer id pair; refusing to continue." >&2
        echo "       network_smith_id is TEXT, so inspect it before re-running." >&2
        return 1
    fi

    APP_API_REF_CSV=$(printf '%s\n' "$rows" | tr '\t' ',')
    APP_API_REF_COUNT=$(printf '%s\n' "$rows" | wc -l | tr -d ' ')
}

# Which department owns each bridge. Sets APP_API_WIFI_DEPT_CSV.
fetch_app_api_wifi_depts() {
    local url="$1"
    local rows
    rows=$(db_psql "$url" -t -A -F $'\t' -c \
        "SELECT ns.network_wifi_id, s.department_id
         FROM network_smith ns
         JOIN network_wifis w ON w.id = ns.network_wifi_id
         JOIN network_surveys s ON s.id = w.survey_id
         WHERE ns.network_smith_id IS NOT NULL AND s.department_id IS NOT NULL
         ORDER BY ns.network_wifi_id;")

    if [[ -z "$(printf '%s' "$rows" | tr -d '[:space:]')" ]]; then
        echo "ERROR: no wifi/department rows returned from the App API" >&2
        return 1
    fi

    if printf '%s\n' "$rows" | sed '/^$/d' | grep -qvE '^[0-9]+'$'\t''[0-9]+$'; then
        echo "ERROR: wifi/department fetch returned a non-integer pair; refusing to continue." >&2
        return 1
    fi

    APP_API_WIFI_DEPT_CSV=$(printf '%s\n' "$rows" | sed '/^$/d' | tr '\t' ',')
}

# Sets APP_API_DEV_DEPT_CSV and APP_API_DEV_DEPT_COUNT. serial_number is free
# text, so this is RFC-4180 CSV and MUST be streamed into a quoted heredoc.
fetch_app_api_device_depts() {
    local url="$1"
    APP_API_DEV_DEPT_CSV=$(db_psql "$url" -q -t -A -c \
        "\copy (SELECT serial_number, department FROM public.device
                WHERE serial_number IS NOT NULL ORDER BY 1) TO STDOUT WITH (FORMAT csv)")

    if [[ -z "$APP_API_DEV_DEPT_CSV" ]]; then
        echo "ERROR: no rows returned from App API device table" >&2
        return 1
    fi

    APP_API_DEV_DEPT_COUNT=$(printf '%s\n' "$APP_API_DEV_DEPT_CSV" | sed '/^$/d' | wc -l | tr -d ' ')
}

# Sets APP_API_WIFI_CONTENT_CSV. Carries live WiFi passwords, so this is
# RFC-4180 CSV and MUST be streamed into a quoted heredoc. Never echo it.
fetch_app_api_wifi_content() {
    local url="$1"
    APP_API_WIFI_CONTENT_CSV=$(db_psql "$url" -q -t -A -c \
        "\copy (SELECT ns.network_wifi_id, w.ssid, w.hidden, NULLIF(w.password, '')
                FROM network_smith ns JOIN network_wifis w ON w.id = ns.network_wifi_id
                WHERE ns.network_smith_id IS NOT NULL
                ORDER BY ns.network_wifi_id) TO STDOUT WITH (FORMAT csv)")

    if [[ -z "$APP_API_WIFI_CONTENT_CSV" ]]; then
        echo "ERROR: no rows returned from App API network_wifis" >&2
        return 1
    fi
}

# Emitted between QUOTED heredocs so bash never expands a payload.
#
# psql ends COPY data at a line holding only \., before the server sees it, and
# resumes reading the rest as psql input. CSV quoting does not protect against
# this: ssids, passwords and serials are free text and a value containing a
# newline can put such a line into the stream, after which the remainder runs as
# meta-commands (\! included) with the operator's permissions. Refuse instead.
copy_block() {
    # grep -c, not -q: -q exits on the first match, which SIGPIPEs the writer and
    # fails the pipeline under pipefail, silently skipping the check.
    local terminators
    terminators=$(printf '%s\n' "$2" | grep -cE '^\\\.[[:space:]]*$' || true)
    if [[ "$terminators" != 0 ]]; then
        echo "ERROR: payload for $1 contains a COPY terminator line; refusing to continue." >&2
        echo "       Inspect the source rows before re-running; this is not valid data." >&2
        return 1
    fi
    printf '\\copy %s FROM STDIN WITH (FORMAT csv)\n' "$1"
    printf '%s\n' "$2"
    printf '\\.\n'
}

# The one content key. Every phase groups on this; see README.md.
NETWORK_CONTENT_KEY="n.ssid, n.is_network_hidden, n.credentials ->> 'psk', n.network_type::text, n.security_type, n.identity"

# Emits _referenced: every row anything points at, with its content group and
# reference counts. Requires a _refs table to already exist. Materialized rather
# than a view because callers scan it repeatedly and the counts are subqueries.
emit_referenced() {
    cat <<SQL
CREATE TEMP TABLE _referenced AS
SELECT n.id,
       n.ssid,
       n.is_network_hidden AS hidden,
       n.credentials ->> 'psk' AS psk,
       n.network_type::text AS type,
       n.security_type AS sec,
       n.identity ->> 'username' AS ident,
       dense_rank() OVER (ORDER BY $NETWORK_CONTENT_KEY) AS grp,
       count(*)     OVER (PARTITION BY $NETWORK_CONTENT_KEY) AS grp_size,
       (SELECT COUNT(*) FROM _refs a WHERE a.network_smith_id = n.id) AS app_refs,
       (SELECT COUNT(*) FROM device WHERE network_id = n.id) +
       (SELECT COUNT(*) FROM device WHERE current_network_id = n.id) +
       (SELECT COUNT(*) FROM device_configured_network WHERE network_id = n.id) +
       (SELECT COUNT(*) FROM device_network_intent WHERE network_id = n.id) +
       (SELECT COUNT(*) FROM network_reference WHERE network_id = n.id) AS smith_refs
FROM network n
WHERE EXISTS (SELECT 1 FROM _refs a WHERE a.network_smith_id = n.id)
   OR EXISTS (SELECT 1 FROM device WHERE network_id = n.id)
   OR EXISTS (SELECT 1 FROM device WHERE current_network_id = n.id)
   OR EXISTS (SELECT 1 FROM device_configured_network WHERE network_id = n.id)
   OR EXISTS (SELECT 1 FROM device_network_intent WHERE network_id = n.id)
   OR EXISTS (SELECT 1 FROM network_reference WHERE network_id = n.id);
SQL
}

# Same validation for the remap pairs phase 3 writes back to both databases.
validate_int_pairs() {
    if printf '%s\n' "$1" | grep -qvE '^[0-9]+'$'\t''[0-9]+$'; then
        echo "ERROR: computed remap contains a non-integer pair; refusing to continue." >&2
        return 1
    fi
}

# ── Race mitigation against the App API sync job. See README.md. ─────────────

# network has no created_at; id is `generated always as identity`, so monotonic.
smith_watermark() {
    db_psql "$1" -tAc "SELECT coalesce(max(id), 0) FROM network;" | tr -d '[:space:]'
}

# ── Post-run verification ────────────────────────────────────────────────────

# Bridges pointing at a Smith row that no longer exists, one "wifi_id,smith_id"
# per line. Read-only against both databases.
dangling_bridges() {
    local smith_url="$1" refs_csv="$2"
    {
        printf 'CREATE TEMP TABLE _dang (network_wifi_id int, network_smith_id int);\n'
        copy_block "_dang (network_wifi_id, network_smith_id)" "$refs_csv"
        cat <<'SQL'
SELECT a.network_wifi_id || ',' || a.network_smith_id
FROM _dang a
WHERE NOT EXISTS (SELECT 1 FROM network n WHERE n.id = a.network_smith_id)
ORDER BY a.network_wifi_id;
SQL
    } | db_psql "$smith_url" -q -t -A -v ON_ERROR_STOP=1 | sed '/^$/d'
}

# Re-reads the bridges and fails if this run stranded any that were not already
# stranded. Repair is left to the operator: nulling a bridge makes the sync job
# recreate the network, which is a write to a fleet-facing database.
verify_no_new_dangling() {
    local app_url="$1" smith_url="$2" before="$3" after new
    fetch_app_api_refs "$app_url" || return 1
    after=$(dangling_bridges "$smith_url" "$APP_API_REF_CSV") || return 1

    # LC_ALL=C throughout: comm requires the exact collation sort produced.
    new=$(LC_ALL=C comm -13 \
        <(printf '%s\n' "$before" | sed '/^$/d' | LC_ALL=C sort) \
        <(printf '%s\n' "$after"  | sed '/^$/d' | LC_ALL=C sort))

    if [[ -z "$new" ]]; then
        echo "==> Verified: no bridge was stranded by this run."
        [[ -n "$(printf '%s' "$before" | tr -d '[:space:]')" ]] &&
            echo "    ($(printf '%s\n' "$before" | sed '/^$/d' | wc -l | tr -d ' ') were already stranded beforehand, unchanged.)"
        return 0
    fi

    echo "" >&2
    echo "!!! $(printf '%s\n' "$new" | wc -l | tr -d ' ') bridge(s) were stranded by this run." >&2
    echo "    The Smith rows below were deleted while the App API still pointed at them." >&2
    printf '%s\n' "$new" | sed 's/^/      network_wifi_id,network_smith_id = /' >&2
    echo "" >&2
    echo "    This is recoverable. Nulling the bridge puts the record back in the" >&2
    echo "    sync job's retry set, and the next pass recreates the network:" >&2
    echo "" >&2
    echo "      UPDATE network_smith SET network_smith_id = NULL" >&2
    echo "      WHERE network_wifi_id IN ($(printf '%s\n' "$new" | cut -d, -f1 | paste -sd, -));" >&2
    echo "" >&2
    echo "    Run it against the App API only after checking the rows are what you expect." >&2
    return 1
}

# Call immediately before the mutating transaction.
refresh_app_api_refs() {
    local url="$1"
    local first="$APP_API_REF_CSV"
    fetch_app_api_refs "$url" || return 1
    APP_API_REF_CSV=$(printf '%s\n%s\n' "$first" "$APP_API_REF_CSV" | sort -u | sed '/^$/d')
    APP_API_REF_COUNT=$(printf '%s\n' "$APP_API_REF_CSV" | sed '/^$/d' | wc -l | tr -d ' ')
}
