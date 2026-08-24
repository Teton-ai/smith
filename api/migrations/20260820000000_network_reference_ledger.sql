-- network_reference (20260720000000_network_schema_expand.sql) already tracks
-- external holds; this is the other half of collection: single definition of
-- "does anything internal to Smith still reference this network", so
-- `collect_network` (api/src/network/ledger.rs) and any later caller share one
-- answer instead of drifting into two different opinions of what counts as
-- referenced.
CREATE OR REPLACE FUNCTION public.network_has_internal_reference(p_network_id integer)
RETURNS boolean LANGUAGE sql STABLE
SET search_path = pg_catalog, public
AS $$
    SELECT EXISTS (
        SELECT 1 FROM device
         WHERE network_id = p_network_id OR current_network_id = p_network_id
        UNION ALL
        SELECT 1 FROM device_configured_network WHERE network_id = p_network_id
        UNION ALL
        SELECT 1 FROM device_network_intent WHERE network_id = p_network_id
    );
$$;
