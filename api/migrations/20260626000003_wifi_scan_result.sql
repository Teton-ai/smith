CREATE TABLE wifi_scan_result (
    id         SERIAL PRIMARY KEY,
    device_id  INTEGER NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    ssid       TEXT NOT NULL,
    signal     INTEGER,
    rate       INTEGER,
    security   TEXT,
    scanned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + INTERVAL '30 minutes'
);

CREATE INDEX ON wifi_scan_result (device_id, expires_at);
