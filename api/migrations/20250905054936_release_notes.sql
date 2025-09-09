ALTER TABLE release ADD COLUMN description TEXT;
COMMENT ON COLUMN release.description IS 'Optional release notes; including features, bug fixes, etc';

ALTER TABLE device ADD COLUMN production BOOL DEFAULT TRUE;
COMMENT ON COLUMN device.production IS 'Flag indicating if device is in production environment (TRUE) or development/testing (FALSE)';
