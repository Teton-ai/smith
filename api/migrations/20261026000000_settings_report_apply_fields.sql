-- Fields the settings report gains for applying settings from the server, and
-- the AP's announced country next to the device's own.
ALTER TYPE device_setting ADD VALUE 'ap_country_code';

ALTER TABLE device_settings_reported
    ADD COLUMN ap_country_code text,
    ADD COLUMN applied_version int4,
    ADD COLUMN corrected device_setting[] NOT NULL DEFAULT '{}';

CREATE TYPE device_setting_apply_state AS ENUM ('failed', 'reverted');

-- Settings not running their wanted value, replaced by every report. Kept out
-- of device_settings_reported so the report row stays one typed column per
-- setting.
CREATE TABLE device_setting_apply_status (
    device_id int4 NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    setting device_setting NOT NULL,
    state device_setting_apply_state NOT NULL,
    reason text NOT NULL,
    PRIMARY KEY (device_id, setting)
);

-- Grafana reads with its own role, granted per table; absent outside prod.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'grafana_reader') THEN
        GRANT SELECT ON device_setting_apply_status TO grafana_reader;
    END IF;
END
$$;
