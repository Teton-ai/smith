INSERT INTO variable_preset (title, description, variables)
VALUES ('DEFAULT', 'DEFAULT', '[]');

INSERT INTO distribution (name, description, architecture)
VALUES ('test', 'test', 'arm64');

INSERT INTO release (distribution_id, version, draft)
VALUES ((SELECT id FROM distribution WHERE name = 'test'), '1.0.0', false);
