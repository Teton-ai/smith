-- Wanted settings: per distribution defaults and per device pins, resolved by
-- the api into one full set per device when someone presses Apply.

-- Bumped on every edit of the device's pins; 0 means nothing to apply.
-- settings_needs_newer_smithd: the device answered its latest ApplySettings as
-- a command it does not know; cleared by its next ApplySettingsResult.
ALTER TABLE device
    ADD COLUMN settings_version int4 NOT NULL DEFAULT 0,
    ADD COLUMN settings_needs_newer_smithd boolean NOT NULL DEFAULT false;

-- A device with this set gets its resolved settings on a home poll, without an
-- Apply press, once it has defaults or pins. Devices that exist now start false
-- and wait for the explicit, staggered launch Apply; devices registered later
-- start true, so they pick up their distribution's defaults by themselves.
ALTER TABLE device ADD COLUMN settings_auto_apply boolean NOT NULL DEFAULT false;
ALTER TABLE device ALTER COLUMN settings_auto_apply SET DEFAULT true;

-- NULL means "inherit the distribution default". A row exists only while the
-- device has at least one pin.
CREATE TABLE device_settings (
    device_id int4 PRIMARY KEY REFERENCES device(id) ON DELETE CASCADE,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by int4,
    wireless_mode int2,
    power_mgnt int2,
    roam_flags int2,
    idle_diff int4,
    busy_diff int4,
    scanr_exp_ms int4,
    scan_int int4,
    idle_th int4,
    busy_th int4,
    nm_wifi_powersave int2,
    nm_autoconnect_retries int4,
    nm_cloned_mac text,
    nm_scan_rand_mac boolean,
    hostname text,
    ntp_servers text[],
    bgscan text,
    -- roam_param is written as one group; the busy fields stay NULL on drivers
    -- without them.
    CONSTRAINT device_settings_roam_group CHECK (
        num_nulls(idle_diff, scanr_exp_ms, scan_int, idle_th) IN (0, 4)
        AND (idle_diff IS NOT NULL OR num_nulls(busy_diff, busy_th) = 2)
    ),
    CONSTRAINT device_settings_mac_group CHECK (
        num_nulls(nm_cloned_mac, nm_scan_rand_mac) IN (0, 2)
    ),
    CONSTRAINT device_settings_has_a_pin CHECK (
        num_nonnulls(
            wireless_mode, power_mgnt, roam_flags, idle_diff, nm_wifi_powersave,
            nm_autoconnect_retries, nm_cloned_mac, hostname, ntp_servers, bgscan
        ) > 0
    )
);

-- NULL means "not managed": smithd leaves the setting as the OS has it.
CREATE TABLE distribution_setting_default (
    distribution_id int4 PRIMARY KEY REFERENCES distribution(id) ON DELETE CASCADE,
    wireless_mode int2,
    power_mgnt int2,
    roam_flags int2,
    idle_diff int4,
    busy_diff int4,
    scanr_exp_ms int4,
    scan_int int4,
    idle_th int4,
    busy_th int4,
    nm_wifi_powersave int2,
    nm_autoconnect_retries int4,
    nm_cloned_mac text,
    nm_scan_rand_mac boolean,
    hostname text,
    ntp_servers text[],
    bgscan text,
    CONSTRAINT distribution_setting_default_roam_group CHECK (
        num_nulls(idle_diff, scanr_exp_ms, scan_int, idle_th) IN (0, 4)
        AND (idle_diff IS NOT NULL OR num_nulls(busy_diff, busy_th) = 2)
    ),
    CONSTRAINT distribution_setting_default_mac_group CHECK (
        num_nulls(nm_cloned_mac, nm_scan_rand_mac) IN (0, 2)
    )
);

-- One row per change to device_settings: the row after the change, or the
-- removed row for a DELETE, so the previous row of a device holds the old
-- values. No foreign key: the history outlives the device.
CREATE TABLE device_settings_audit (
    audit_id int8 GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    changed_at timestamptz NOT NULL DEFAULT now(),
    -- NULL for changes made outside the api (migrations, psql).
    changed_by int4,
    operation text NOT NULL CHECK (operation IN ('INSERT', 'UPDATE', 'DELETE')),
    device_id int4 NOT NULL,
    wireless_mode int2,
    power_mgnt int2,
    roam_flags int2,
    idle_diff int4,
    busy_diff int4,
    scanr_exp_ms int4,
    scan_int int4,
    idle_th int4,
    busy_th int4,
    nm_wifi_powersave int2,
    nm_autoconnect_retries int4,
    nm_cloned_mac text,
    nm_scan_rand_mac boolean,
    hostname text,
    ntp_servers text[],
    bgscan text
);

CREATE INDEX device_settings_audit_device_id_idx
    ON device_settings_audit (device_id, changed_at);

-- The api sets smith.user_id with set_config(..., true) in the transaction of
-- the change.
CREATE FUNCTION device_settings_audit_row() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    r device_settings;
BEGIN
    IF TG_OP = 'DELETE' THEN
        r := OLD;
    ELSE
        r := NEW;
    END IF;
    INSERT INTO device_settings_audit (
        changed_by, operation, device_id,
        wireless_mode, power_mgnt, roam_flags,
        idle_diff, busy_diff, scanr_exp_ms, scan_int, idle_th, busy_th,
        nm_wifi_powersave, nm_autoconnect_retries, nm_cloned_mac, nm_scan_rand_mac,
        hostname, ntp_servers, bgscan
    ) VALUES (
        NULLIF(current_setting('smith.user_id', true), '')::int4, TG_OP, r.device_id,
        r.wireless_mode, r.power_mgnt, r.roam_flags,
        r.idle_diff, r.busy_diff, r.scanr_exp_ms, r.scan_int, r.idle_th, r.busy_th,
        r.nm_wifi_powersave, r.nm_autoconnect_retries, r.nm_cloned_mac, r.nm_scan_rand_mac,
        r.hostname, r.ntp_servers, r.bgscan
    );
    RETURN NULL;
END
$$;

CREATE TRIGGER device_settings_audit
    AFTER INSERT OR UPDATE OR DELETE ON device_settings
    FOR EACH ROW EXECUTE FUNCTION device_settings_audit_row();

-- Grafana reads with its own role, granted per table; absent outside prod.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'grafana_reader') THEN
        GRANT SELECT ON device_settings, distribution_setting_default, device_settings_audit
            TO grafana_reader;
    END IF;
END
$$;
