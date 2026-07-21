-- Backfill the security axis / credentials envelope for existing rows.
-- security_type here is a provisional guess derived from password: it reproduces
-- exactly what the ApplyNetworks builder infers today, so downstream behaviour is
-- unchanged. It is corrected authoritatively later from device-reported key-mgmt.
-- security_type is intentionally left nullable; NOT NULL is added once every
-- writer populates it. All statements are guarded so re-running is a no-op.

-- Normalize empty-string passwords to NULL first, so '' does not become {"psk":""}.
UPDATE network SET password = NULL WHERE password = '';

UPDATE network
   SET security_type = CASE WHEN password IS NULL THEN 'open' ELSE 'wpa-psk' END
 WHERE security_type IS NULL;

-- Open rows are already '{}' from the Stage 0 default, so only wpa-psk rows need
-- a psk envelope; excluding open rows keeps this a true no-op on re-run.
UPDATE network
   SET credentials = jsonb_build_object('psk', password)
 WHERE credentials = '{}'::jsonb AND password IS NOT NULL;
