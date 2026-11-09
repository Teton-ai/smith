CREATE TABLE device_pin_history (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    device_id INTEGER NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    follow_latest BOOLEAN NOT NULL,
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX device_pin_history_device_id_id_idx ON device_pin_history (device_id, id DESC);

-- Only actual state transitions count; a device's initial default is not a pin action.
-- Sharing the update transaction also rolls back history if retargeting fails.
CREATE FUNCTION record_device_pin_change() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $$
BEGIN
    INSERT INTO public.device_pin_history (device_id, follow_latest)
    VALUES (NEW.id, NEW.follow_latest);
    RETURN NEW;
END;
$$;

CREATE TRIGGER device_pin_changed
AFTER UPDATE OF follow_latest ON device
FOR EACH ROW WHEN (OLD.follow_latest IS DISTINCT FROM NEW.follow_latest)
EXECUTE FUNCTION record_device_pin_change();
