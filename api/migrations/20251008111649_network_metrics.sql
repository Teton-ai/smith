CREATE TABLE device_network (
    device_id INTEGER PRIMARY KEY REFERENCES device(id) ON DELETE CASCADE,
    network_score INTEGER CHECK(network_score >= 1 AND network_score <= 5),
    rx_mbps REAL NOT NULL,
    tx_mbps REAL NOT NULL,
    rx_bytes_delta BIGINT NOT NULL,
    tx_bytes_delta BIGINT NOT NULL,
    interval_seconds BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
