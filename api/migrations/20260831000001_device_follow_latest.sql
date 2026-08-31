-- Devices that do not follow latest are skipped by fleet-wide rollouts and by
-- automatic canary selection; they only move when an operator targets them
-- explicitly. New rows default to pinned so freshly flashed devices stay on the
-- release they shipped with, but the existing fleet keeps its current behaviour.
ALTER TABLE device
    ADD COLUMN follow_latest BOOLEAN NOT NULL DEFAULT FALSE;

UPDATE device SET follow_latest = TRUE;
