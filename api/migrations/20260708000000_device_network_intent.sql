CREATE TABLE device_network_intent (
    id         SERIAL PRIMARY KEY,
    device_id  INTEGER NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    network_id INTEGER NOT NULL REFERENCES network(id) ON DELETE RESTRICT,
    priority   INTEGER NOT NULL,
    managed_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT device_network_intent_device_network_unique UNIQUE (device_id, network_id)
);

CREATE INDEX idx_device_network_intent_device_id ON device_network_intent(device_id);
