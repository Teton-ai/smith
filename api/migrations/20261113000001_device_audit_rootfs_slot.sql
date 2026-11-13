CREATE TYPE rootfs_slot AS ENUM ('a', 'b', 'single');

ALTER TABLE device_audit
    ADD COLUMN rootfs_slot rootfs_slot,
    ADD COLUMN rootfs_other_slot_bootable BOOLEAN;
