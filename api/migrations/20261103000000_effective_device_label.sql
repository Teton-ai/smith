-- Labels as clients see them: the stored labels plus the `pinned` tag, computed
-- from NOT follow_latest so it can never disagree with the pin state. A tag is a
-- label with only a key, stored with an empty value. A stored `pinned` row,
-- set by hand before the key was reserved, is hidden rather than deleted.
CREATE VIEW effective_device_label AS
SELECT dl.device_id, l.name, dl.value
FROM device_label dl
JOIN label l ON l.id = dl.label_id
WHERE l.name <> 'pinned'
UNION ALL
SELECT d.id, 'pinned', ''
FROM device d
WHERE NOT d.follow_latest;
