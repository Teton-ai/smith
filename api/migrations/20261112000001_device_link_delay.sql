-- Round trip to the API as the device measures it: the minimum over the last
-- hours, the recent minimum, and when the device last reported them.
ALTER TABLE device
    ADD COLUMN link_baseline_ms INT,
    ADD COLUMN link_current_ms INT,
    ADD COLUMN link_delay_at TIMESTAMPTZ;
