UPDATE device SET modem_id = NULL WHERE modem_id IS NOT NULL;
ALTER TABLE device ADD CONSTRAINT device_modem_id_unique UNIQUE (modem_id);
