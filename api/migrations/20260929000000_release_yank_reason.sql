-- Why and when a release was withdrawn, so whoever finds a yanked release later
-- does not have to go digging for the incident that caused it.
ALTER TABLE release
    ADD COLUMN yanked_reason TEXT,
    ADD COLUMN yanked_at TIMESTAMPTZ;
