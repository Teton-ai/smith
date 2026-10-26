-- x86 overview machines have no `iw` (as for country_code). Separate from the
-- migration adding the enum value, which cannot be used in the same transaction.
INSERT INTO distribution_unsupported_setting (distribution_id, setting)
SELECT d.id, 'wifi_power_save_on'::device_setting
FROM distribution d
WHERE d.name LIKE 'overview%';
