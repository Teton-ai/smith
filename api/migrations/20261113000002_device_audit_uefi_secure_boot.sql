CREATE TYPE uefi_secure_boot AS ENUM ('off', 'setup', 'on');

ALTER TABLE device_audit
    ADD COLUMN uefi_secure_boot uefi_secure_boot;
