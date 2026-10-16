-- Settings each device reports it runs (phase 1 is view only, nothing here is
-- pushed to devices). One row per device, replaced by every report.
CREATE TYPE device_setting AS ENUM (
    'wireless_mode', 'power_mgnt', 'roam_flags',
    'idle_diff', 'busy_diff', 'scanr_exp_ms', 'scan_int', 'idle_th', 'busy_th',
    'nm_wifi_powersave', 'nm_autoconnect_retries', 'nm_cloned_mac', 'nm_scan_rand_mac',
    'bgscan', 'country_code', 'hostname',
    'ntp_servers', 'ntp_link_servers', 'ntp_fallback_servers', 'ntp_server_in_use', 'ntp_synchronized',
    'check_interval_s'
);

CREATE TABLE device_settings_reported (
    device_id int4 PRIMARY KEY REFERENCES device(id) ON DELETE CASCADE,
    wireless_mode int2,
    power_mgnt int2,
    -- Raw driver flags; active roaming is bit 0x04 (0x07 on, 0x03 disabled).
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
    bgscan text,
    country_code text,
    hostname text,
    ntp_servers text[],
    ntp_link_servers text[],
    ntp_fallback_servers text[],
    ntp_server_in_use text,
    ntp_synchronized boolean,
    check_interval_s int4,
    -- Settings the device should have but failed to read this time.
    read_errors device_setting[] NOT NULL DEFAULT '{}',
    -- Version of the wanted settings this report reflects; NULL until devices
    -- receive wanted settings (phase 2).
    reported_version int4,
    reported_at TIMESTAMPTZ DEFAULT NOW() NOT NULL
);

-- Settings a distribution's devices cannot have (e.g. no idle/busy split in
-- the L4T 35 driver), stored once per distribution instead of per device.
CREATE TABLE distribution_unsupported_setting (
    distribution_id int4 NOT NULL REFERENCES distribution(id) ON DELETE CASCADE,
    setting device_setting NOT NULL,
    PRIMARY KEY (distribution_id, setting)
);

-- L4T 35 vendor driver v5.9: one diff and one threshold, no scan MAC
-- randomization.
INSERT INTO distribution_unsupported_setting (distribution_id, setting)
SELECT d.id, s.setting::device_setting
FROM distribution d
CROSS JOIN (VALUES ('busy_diff'), ('busy_th'), ('nm_scan_rand_mac')) AS s(setting)
WHERE d.name LIKE 'orin-nx-l4t-35.3.1%';

-- Platform C runs the upstream rtw88 driver (none of the vendor driver's
-- parameters or roaming files) and chrony (no fallback servers).
INSERT INTO distribution_unsupported_setting (distribution_id, setting)
SELECT d.id, s.setting::device_setting
FROM distribution d
CROSS JOIN (VALUES
    ('wireless_mode'), ('power_mgnt'), ('roam_flags'),
    ('idle_diff'), ('busy_diff'), ('scanr_exp_ms'), ('scan_int'), ('idle_th'), ('busy_th'),
    ('ntp_fallback_servers')
) AS s(setting)
WHERE d.name = 'orin-nano-rcmptr-mini-revc-up-l4t-36.4.3';

-- x86 overview machines: Intel Wi-Fi, no vendor driver and no `iw`.
INSERT INTO distribution_unsupported_setting (distribution_id, setting)
SELECT d.id, s.setting::device_setting
FROM distribution d
CROSS JOIN (VALUES
    ('wireless_mode'), ('power_mgnt'), ('roam_flags'),
    ('idle_diff'), ('busy_diff'), ('scanr_exp_ms'), ('scan_int'), ('idle_th'), ('busy_th'),
    ('country_code')
) AS s(setting)
WHERE d.name LIKE 'overview%';
