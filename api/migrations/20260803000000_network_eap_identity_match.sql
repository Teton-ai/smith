-- wpa-eap has no shared psk, so every field the Stage 3 match compared was equal
-- for any two EAP rows on one SSID: the second device landed on the first
-- device's row and its own credential was dropped. Add identity to the match.
--
-- The lock key stays as it is. Coarsening a lock only merges buckets, so two EAP
-- rows sharing an SSID serializing against each other is correct.

-- DEFAULT NULL rather than an overload: a 5-argument call cannot be resolved
-- between a 5-argument and a 6-argument-with-default candidate, so the old
-- signature has to go. Existing 5-argument callers land here with p_identity
-- NULL, the relaxed case, so their behaviour is unchanged.
DROP FUNCTION IF EXISTS public.network_find_by_content(text, boolean, jsonb, text, text);

-- The identity clause is relaxed in both directions like the security_type one
-- above it, so a writer that knows no identity converges instead of forking, and
-- an identified writer still heals a row that has none. ORDER BY keeps
-- security_type primary to leave the existing ordering guarantees untouched.
CREATE OR REPLACE FUNCTION public.network_find_by_content(
    p_ssid text, p_is_network_hidden boolean, p_credentials jsonb,
    p_security_type text, p_network_type text, p_identity jsonb DEFAULT NULL
) RETURNS integer LANGUAGE sql STABLE
SET search_path = pg_catalog, public
AS $$
    SELECT id FROM public.network
    WHERE ssid IS NOT DISTINCT FROM p_ssid
      AND is_network_hidden = p_is_network_hidden
      AND network_type::text = p_network_type
      AND (credentials->>'psk') IS NOT DISTINCT FROM (p_credentials->>'psk')
      AND (p_security_type IS NULL OR security_type = p_security_type OR security_type IS NULL)
      AND (p_identity IS NULL OR identity = p_identity OR identity IS NULL)
    ORDER BY (security_type IS NOT DISTINCT FROM p_security_type) DESC,
             (identity IS NOT DISTINCT FROM p_identity) DESC,
             id DESC
    LIMIT 1;
$$;
