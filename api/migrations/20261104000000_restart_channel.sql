-- The restart channel: smith-rescue polls /smith/restart, separately from
-- smithd's /home, so a Restart reaches a device whose smithd is stuck.

-- One row per device whose smith-rescue has polled. A recent poll routes the
-- device's Restart commands to the restart channel instead of /home.
CREATE TABLE device_restart_channel (
    device_id integer PRIMARY KEY REFERENCES device(id) ON DELETE CASCADE,
    last_poll_at timestamptz NOT NULL DEFAULT now(),
    boot_id uuid NOT NULL
);

-- A Restart command handed to smith-rescue, bound to the boot it must end.
-- The command is done when the device polls again from another boot.
CREATE TABLE restart_delivery (
    command_id integer PRIMARY KEY REFERENCES command_queue(id) ON DELETE CASCADE,
    device_id integer NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    target_boot uuid NOT NULL,
    delivered_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX restart_delivery_device_id_idx ON restart_delivery (device_id);
