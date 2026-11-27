-- Track staggered rollout waves on deployment_devices alongside canaries (wave 0).
ALTER TABLE deployment_devices
    ADD COLUMN wave integer NOT NULL DEFAULT 0,
    ADD COLUMN opens_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN reached_at timestamptz;

UPDATE deployment_devices SET opens_at = created_at;

CREATE UNIQUE INDEX deployment_devices_deployment_device_key
    ON deployment_devices (deployment_id, device_id);
CREATE INDEX deployment_devices_device_id_idx
    ON deployment_devices (device_id);
