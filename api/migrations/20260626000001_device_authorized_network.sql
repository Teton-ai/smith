CREATE TABLE device_authorized_network (
    device_id  INTEGER NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    network_id INTEGER NOT NULL REFERENCES network(id) ON DELETE RESTRICT,
    added_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (device_id, network_id)
);

INSERT INTO device_authorized_network (device_id, network_id)
SELECT id, network_id
FROM device
WHERE network_id IS NOT NULL;

ALTER TABLE device DROP COLUMN network_id;
