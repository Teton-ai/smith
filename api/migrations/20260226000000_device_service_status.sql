CREATE TABLE device_service_status (
    device_id INTEGER NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    release_service_id INTEGER NOT NULL REFERENCES release_services(id) ON DELETE CASCADE,
    active_state TEXT NOT NULL,
    n_restarts INTEGER NOT NULL DEFAULT 0,
    checked_at TIMESTAMPTZ DEFAULT NOW() NOT NULL,
    PRIMARY KEY (device_id, release_service_id)
);
