CREATE INDEX IF NOT EXISTS device_release_upgrades_device_id_id_idx
    ON device_release_upgrades (device_id, id DESC);
