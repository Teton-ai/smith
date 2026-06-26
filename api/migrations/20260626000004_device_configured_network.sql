CREATE TABLE device_configured_network (
    device_id  INTEGER NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    network_id INTEGER NOT NULL REFERENCES network(id) ON DELETE RESTRICT,
    is_active  BOOLEAN NOT NULL DEFAULT false,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (device_id, network_id)
);
