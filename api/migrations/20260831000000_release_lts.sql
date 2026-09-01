-- LTS marks a release as one we commit to keeping devices on long-term. It is
-- a flag rather than a pointer on distribution so the full history of which
-- versions were designated LTS is preserved.
ALTER TABLE release
    ADD COLUMN lts BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN lts_marked_at TIMESTAMPTZ;
