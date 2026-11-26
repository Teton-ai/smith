-- no-transaction
-- /smith/home polls pending commands on every device tick (~250/s). Without a
-- partial index, Postgres scans every historical fetched command for the device
-- through idx_command2_queue_device_id.
CREATE INDEX CONCURRENTLY idx_command_queue_pending
    ON public.command_queue (device_id, id)
    WHERE fetched = false AND canceled = false;
