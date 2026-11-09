ALTER TABLE device_pin_history
    ADD COLUMN release_id INTEGER REFERENCES release(id),
    ADD COLUMN initial_approval BOOLEAN NOT NULL DEFAULT FALSE;

-- Snapshot the pinned release so later assignments cannot rewrite the event.
-- Approval records its initial pin separately, after its target has been assigned.
CREATE OR REPLACE FUNCTION record_device_pin_change() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $$
BEGIN
    INSERT INTO public.device_pin_history (device_id, follow_latest, release_id)
    VALUES (NEW.id, NEW.follow_latest,
            CASE WHEN NOT NEW.follow_latest THEN COALESCE(NEW.target_release_id, NEW.release_id) END);
    RETURN NEW;
END;
$$;

-- Approval may precede the device's first version report. Fill that missing
-- version only while this is still the same uninterrupted initial pin.
CREATE FUNCTION complete_initial_pin_version() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $$
BEGIN
    UPDATE public.device_pin_history h
    SET release_id = COALESCE(NEW.target_release_id, NEW.release_id)
    WHERE h.device_id = NEW.id AND h.initial_approval
      AND h.changed_at = NEW.approved_at AND h.release_id IS NULL
      AND NOT EXISTS (
          SELECT 1 FROM public.device_pin_history later
          WHERE later.device_id = h.device_id AND later.id > h.id
      );
    RETURN NEW;
END;
$$;

CREATE TRIGGER device_initial_pin_version
AFTER UPDATE OF release_id ON device
FOR EACH ROW WHEN (OLD.release_id IS NULL AND NEW.release_id IS NOT NULL
                   AND NEW.approved AND NOT NEW.follow_latest)
EXECUTE FUNCTION complete_initial_pin_version();
