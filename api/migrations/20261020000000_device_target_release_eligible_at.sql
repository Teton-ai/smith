-- When a staggered rollout lets the device move to its target; NULL means as
-- soon as the target was set. The poll keeps the device on its current release
-- until then.
ALTER TABLE device
    ADD COLUMN target_release_eligible_at TIMESTAMPTZ;
