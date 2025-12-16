-- Create release_services table to store systemd service information per release
-- Services can be auto-extracted from deb packages or manually registered
CREATE TABLE release_services (
    id SERIAL PRIMARY KEY,
    release_id INTEGER NOT NULL REFERENCES release(id) ON DELETE CASCADE,
    package_id INTEGER REFERENCES package(id) ON DELETE SET NULL,  -- NULL for manual services
    service_name TEXT NOT NULL,
    watchdog_sec INTEGER,  -- NULL if WatchdogSec not defined in service file
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    UNIQUE (release_id, service_name)
);

CREATE INDEX idx_release_services_release_id ON release_services(release_id);
