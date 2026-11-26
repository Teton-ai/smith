-- no-transaction
-- GET /commands/bundles and GET /commands/bundles/{uuid} join command_queue
-- by bundle uuid. Without an index on bundle, every lookup seq-scans the
-- entire command_queue table.
CREATE INDEX CONCURRENTLY idx_command_queue_bundle
    ON public.command_queue (bundle);
