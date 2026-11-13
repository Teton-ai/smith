ALTER TABLE device_audit
    ADD COLUMN console_logins_closed BOOLEAN,
    ADD COLUMN reboot_watchdog_set BOOLEAN;
