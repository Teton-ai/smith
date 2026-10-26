-- Power save as the Wi-Fi interface runs it (`iw`), next to the NetworkManager
-- default, which only reaches a connection when it is activated again.
ALTER TYPE device_setting ADD VALUE 'wifi_power_save_on';

ALTER TABLE device_settings_reported ADD COLUMN wifi_power_save_on boolean;
