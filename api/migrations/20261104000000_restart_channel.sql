-- smith-rescue takes Restart commands through /smith/restart, apart from
-- smithd. The api binds each one to the boot of the poll that takes it, and
-- marks it done when a poll comes from another boot.
ALTER TABLE command_queue ADD COLUMN target_boot uuid;

-- Devices without smith-rescue keep getting Restart commands through /home
-- until their smith-rescue polls /smith/restart at least once.
ALTER TABLE device ADD COLUMN has_restart_channel boolean NOT NULL DEFAULT false;
