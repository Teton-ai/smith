CREATE TABLE wifi_scan_result (
    id         SERIAL PRIMARY KEY,
    device_id  INTEGER NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    ssid       TEXT,
    bssid      TEXT NOT NULL,
    signal     INTEGER,
    rate       INTEGER,
    security   TEXT,
    channel    INTEGER,
    scanned_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ON wifi_scan_result (device_id, scanned_at DESC);
