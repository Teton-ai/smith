-- The Deployment page polls staggered rollout progress by target release.
CREATE INDEX IF NOT EXISTS idx_device_target_release_id ON device (target_release_id);
