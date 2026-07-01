CREATE TABLE device_configured_network (
    device_id    INTEGER NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    network_id   INTEGER NOT NULL REFERENCES network(id) ON DELETE CASCADE,
    profile_name TEXT    NOT NULL,
    is_active    BOOLEAN NOT NULL DEFAULT false,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (device_id, profile_name)
);
CREATE INDEX ON device_configured_network(network_id);
