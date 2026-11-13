CREATE TYPE uda_encryption AS ENUM ('encrypted', 'unencrypted', 'absent');

ALTER TABLE device_audit
    ADD COLUMN root_encrypted BOOLEAN,
    ADD COLUMN uda_encryption uda_encryption;
