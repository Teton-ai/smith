-- A device holds a download slot for a release from the moment it is told to
-- move to it. Keying the slot on the release means a retarget elsewhere
-- invalidates it without touching the retarget queries.
ALTER TABLE device
    ADD COLUMN download_slot_release_id INT REFERENCES release(id) ON DELETE SET NULL,
    ADD COLUMN download_slot_at TIMESTAMPTZ;

-- NULL means the API default.
ALTER TABLE ip_address
    ADD COLUMN max_concurrent_downloads INT CHECK (max_concurrent_downloads > 0);

CREATE INDEX device_download_slot_ip_idx
    ON device (ip_address_id)
    WHERE download_slot_release_id IS NOT NULL;

-- Devices already told to download keep their target through the deploy.
UPDATE device
SET download_slot_release_id = target_release_id, download_slot_at = NOW()
WHERE release_id IS DISTINCT FROM target_release_id
  AND target_release_id IS NOT NULL
  AND (target_release_eligible_at IS NULL OR target_release_eligible_at <= NOW());
