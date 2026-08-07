-- Stage 4: enforce the content-addressing guarantee at the DB level. Preceded by
-- a separate, prod-only dedupe run (scripts/dedupe-network/); see
-- .claude/feat/network-stage4-dedupe-constraint/arch.md. Not applicable to a
-- fresh dev/CI database, which starts with an empty (trivially deduped) table.

-- Plain CREATE UNIQUE INDEX, not CONCURRENTLY: CONCURRENTLY cannot run inside
-- sqlx's transactional migration wrapper, and at network's current/near-future
-- size (~166 rows) a blocking build is sub-second, so the tradeoff isn't worth
-- a manual deploy step. Revisit if network approaches ~100k rows: past that a
-- blocking build stops being negligible and this needs to become a manual
-- CONCURRENTLY step (which requires taking this out of one transaction).
--
-- NULLS NOT DISTINCT is required for this to dedupe anything: without it,
-- Postgres's default treats every NULL identity as distinct from every other,
-- so open/wpa-psk rows (the overwhelming majority, all NULL identity) would
-- never conflict with each other at all.
CREATE UNIQUE INDEX network_ident_uq_idx ON network
    (ssid, is_network_hidden, security_type, credentials, identity) NULLS NOT DISTINCT;

ALTER TABLE network ADD CONSTRAINT network_ident_uq UNIQUE USING INDEX network_ident_uq_idx;

-- security_type is WiFi-specific vocabulary (see security_type_for,
-- api/src/network/route.rs); Ethernet/Dongle rows have none, so the check is
-- scoped to wifi rows only, same as network_check's ssid requirement. Safe
-- for wifi rows now that Stages 2-3 made every wifi writer populate it, and
-- the dedupe run (precondition above) accounted for the historical rows that
-- weren't.
ALTER TABLE network ADD CONSTRAINT network_security_type_wifi_check
    CHECK ((network_type <> 'wifi'::network_type) OR (security_type IS NOT NULL));
