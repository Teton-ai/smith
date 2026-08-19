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
--
-- network_type is part of the key because network_find_by_content's match
-- already treats it that way (api/migrations/20260730000000_..., "a NULL
-- matches nothing... leaving it out would let a POST for an ethernet network
-- match a wifi row"). Without it here, two distinct Ethernet/Dongle rows -
-- both NULL ssid/security_type/identity, default '{}' credentials - would
-- collide under NULLS NOT DISTINCT even though they aren't the same network.
CREATE UNIQUE INDEX network_ident_uq_idx ON network
    (network_type, ssid, is_network_hidden, security_type, credentials, identity) NULLS NOT DISTINCT;

ALTER TABLE network ADD CONSTRAINT network_ident_uq UNIQUE USING INDEX network_ident_uq_idx;

-- Same provisional heuristic as the Stage 1 backfill (20260720000001), scoped
-- to wifi only: ReportNMProfiles can still land a wifi row with no
-- security_type when a device reports a key_mgmt Smith doesn't recognize
-- (api/src/home.rs, map_key_mgmt's `other => None` branch is a permanent,
-- anticipated case, not a transient one), so unlike Stage 1 this cannot be
-- treated as a closed backlog. Two such rows exist in prod as of this
-- migration; this heals them the same way a later, recognized report would
-- (network_find_by_content's relaxed match + COALESCE), so it is a no-op for
-- any row a real report reaches first.
UPDATE network
   SET security_type = CASE WHEN credentials ->> 'psk' IS NULL THEN 'open' ELSE 'wpa-psk' END
 WHERE network_type = 'wifi' AND security_type IS NULL;

-- security_type is WiFi-specific vocabulary (see security_type_for,
-- api/src/network/route.rs); Ethernet/Dongle rows have none, so the check is
-- scoped to wifi rows only, same as network_check's ssid requirement.
ALTER TABLE network ADD CONSTRAINT network_security_type_wifi_check
    CHECK ((network_type <> 'wifi'::network_type) OR (security_type IS NOT NULL));
