CREATE TABLE device_audit (
    device_id INTEGER NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    disk_encrypted BOOLEAN,
    password_access_disabled BOOLEAN,
    checked_at TIMESTAMPTZ DEFAULT NOW() NOT NULL,
    PRIMARY KEY (device_id)
);
