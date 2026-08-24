-- Lets bundle dispatch (queue_commands_bundle) stagger when a device becomes
-- eligible to fetch a command, instead of every row being fetchable the
-- instant it's inserted. Defaults to now() so every existing insert path
-- (single-device commands, non-staggered bulk commands) is unaffected.
ALTER TABLE command_queue ADD COLUMN available_at timestamptz NOT NULL DEFAULT now();
