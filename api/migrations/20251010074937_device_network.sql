CREATE TABLE device_network (
    device_id INTEGER PRIMARY KEY REFERENCES device(id) ON DELETE CASCADE,
    network_score INTEGER CHECK(network_score >= 1 AND network_score <= 5),
    source TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
