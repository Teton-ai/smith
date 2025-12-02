-- Add migration script here
-- Setup the tables

CREATE TABLE label (
    id SERIAL PRIMARY KEY,
    -- e.g. 'department'
    name VARCHAR(255) UNIQUE NOT NULL
);

CREATE TABLE device_label (
    device_id INT NOT NULL,
    label_id INT NOT NULL,
    value VARCHAR(255) NOT NULL,
    -- ensure that a label is only applied to a device once, e.g. one device only has one department
    PRIMARY KEY (device_id, label_id),
    FOREIGN KEY (device_id) REFERENCES device(id),
    FOREIGN KEY (label_id) REFERENCES label(id)
);


-- Migrate the labels json to the normalized tables
-- Create a new label for each of the labels that exists in the device.labels
INSERT INTO label (name)
SELECT
    DISTINCT JSONB_OBJECT_KEYS(labels)
FROM device;

WITH device_label_keys AS (
    SELECT
        d.id,
        -- This creates a new row for each label in device, i.e. it explodes the labels
        JSONB_OBJECT_KEYS(d.labels) key,
        labels
    FROM device d
)
INSERT INTO device_label (device_id, label_id, value)
SELECT
    d.id,
    l.id,
    d.labels->>d.key
FROM device_label_keys d
INNER JOIN label l
    ON l.name = d.key;

-- Cleanup
ALTER TABLE device DROP COLUMN labels;
