-- The `pinned` tag mirrors NOT follow_latest, so every label selector can find
-- pinned devices. A tag is a label with only a key, stored with an empty value. A
-- trigger keeps it in sync because new devices are pinned by the column default,
-- without any code path writing to follow_latest. The API refuses to change it by
-- hand.
INSERT INTO label (name) VALUES ('pinned') ON CONFLICT (name) DO NOTHING;

-- A label set by hand before this migration can disagree with the pin state.
DELETE FROM device_label
WHERE label_id = (SELECT id FROM label WHERE name = 'pinned');

INSERT INTO device_label (device_id, label_id, value)
SELECT d.id, l.id, ''
FROM device d
JOIN label l ON l.name = 'pinned'
WHERE NOT d.follow_latest;

CREATE FUNCTION sync_device_pinned_tag() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.follow_latest THEN
        DELETE FROM device_label
        WHERE device_id = NEW.id
          AND label_id = (SELECT id FROM label WHERE name = 'pinned');
    ELSE
        INSERT INTO device_label (device_id, label_id, value)
        SELECT NEW.id, id, '' FROM label WHERE name = 'pinned'
        ON CONFLICT (device_id, label_id) DO NOTHING;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER device_sync_pinned_tag
AFTER INSERT OR UPDATE OF follow_latest ON device
FOR EACH ROW EXECUTE FUNCTION sync_device_pinned_tag();
