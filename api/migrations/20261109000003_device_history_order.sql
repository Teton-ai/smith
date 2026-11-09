CREATE INDEX device_release_upgrades_device_time_id_idx
    ON device_release_upgrades (device_id, created_at DESC, id DESC);
CREATE INDEX device_pin_history_device_time_id_idx
    ON device_pin_history (device_id, changed_at DESC, id DESC);
