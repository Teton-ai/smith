-- Stage 3: content-addressed lock + identity match, plus a one-time psk backfill.
-- No schema change; see .claude/feat/network-stage3-idempotent-post/arch.md.

-- Single definition of the lock key. Deliberately excludes security_type (and
-- network_type): a hash cannot represent "NULL means unknown, matches anything",
-- but dropping a field from the hash can only merge lock buckets, never split an
-- identity group, so coarsening the lock this way is safe.
-- jsonb_build_array(...)::text encodes NULL as JSON null and quotes strings, so
-- there is no separator-collision risk. Callees are schema-qualified rather than
-- pinned with SET search_path: a function carrying a SET clause cannot be
-- inlined, and this one is destined for the Stage 4 expression index.
CREATE OR REPLACE FUNCTION public.network_content_lock_key(
    p_ssid text, p_is_network_hidden boolean, p_credentials jsonb
) RETURNS bigint LANGUAGE sql IMMUTABLE AS $$
    SELECT pg_catalog.hashtextextended(
        pg_catalog.jsonb_build_array(
            p_ssid, p_is_network_hidden, p_credentials ->> 'psk')::text,
        0);
$$;

-- Single definition of the identity match, consumed by every writer so the match
-- cannot drift between them. The security_type clause is bidirectionally relaxed:
-- a NULL-security report matches a typed row and vice versa, so either write
-- direction converges instead of forking a duplicate.
--
-- ORDER BY prefers an exact security_type match over a relaxed one: without it a
-- typed writer can land on a NULL row while its exact twin exists and "heal" the
-- NULL row into a second identical typed row. id DESC then breaks ties toward the
-- newest row, which carries real reported data rather than the Stage 1
-- provisional backfill.
--
-- p_network_type is required (a NULL matches nothing): both writers always know
-- it, and leaving it out would let a POST for an ethernet network match a wifi
-- row that happens to share ssid/hidden/psk.
CREATE OR REPLACE FUNCTION public.network_find_by_content(
    p_ssid text, p_is_network_hidden boolean, p_credentials jsonb,
    p_security_type text, p_network_type text
) RETURNS integer LANGUAGE sql STABLE
SET search_path = pg_catalog, public
AS $$
    SELECT id FROM public.network
    WHERE ssid IS NOT DISTINCT FROM p_ssid
      AND is_network_hidden = p_is_network_hidden
      AND network_type::text = p_network_type
      AND (credentials->>'psk') IS NOT DISTINCT FROM (p_credentials->>'psk')
      AND (p_security_type IS NULL OR security_type = p_security_type OR security_type IS NULL)
    ORDER BY (security_type IS NOT DISTINCT FROM p_security_type) DESC, id DESC
    LIMIT 1;
$$;

-- F9: re-run only the psk-into-credentials backfill from the Stage 1 migration.
-- Unlike that migration, this does NOT resolve security_type: baking a guessed
-- security_type here would prevent later correct SAE/OWE/EAP reports from ever
-- matching under the relaxed clause above.
--
-- Keyed on the psk actually being absent rather than on credentials being empty,
-- so rows that picked up Stage 2 metadata (pmf/eap/...) but never a psk are
-- backfilled too; `||` preserves those keys. All statements are guarded so
-- re-running is a no-op.
UPDATE network SET password = NULL WHERE password = '';

UPDATE network
   SET credentials = credentials || jsonb_build_object('psk', password)
 WHERE credentials ->> 'psk' IS NULL AND password IS NOT NULL;
