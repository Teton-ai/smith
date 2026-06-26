ALTER TABLE network
    ADD CONSTRAINT network_ssid_password_unique UNIQUE NULLS NOT DISTINCT (ssid, password);
