-- Every /home poll queues behind the ACCESS EXCLUSIVE lock while it waits, so
-- fail the deploy instead of stalling the fleet behind a long transaction.
SET LOCAL lock_timeout = '3s';

-- A device holds a download slot for a release from the moment it is told to
-- move to it. Keying the slot on the release means a retarget elsewhere
-- invalidates it without touching the retarget queries.
ALTER TABLE device
    ADD COLUMN download_slot_release_id INT REFERENCES release(id) ON DELETE SET NULL,
    ADD COLUMN download_slot_at TIMESTAMPTZ;

-- NULL means the API default.
ALTER TABLE ip_address
    ADD COLUMN max_concurrent_downloads INT CHECK (max_concurrent_downloads > 0);

-- Every slot request counts all devices on its IP, holders and active ones.
CREATE INDEX device_ip_address_id_idx ON device (ip_address_id);
