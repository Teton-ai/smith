CREATE TABLE device_upload_session (
    id uuid PRIMARY KEY,
    device_id integer NOT NULL REFERENCES device(id) ON DELETE CASCADE,
    bucket text NOT NULL,
    object_key text NOT NULL,
    size_bytes bigint NOT NULL CHECK (size_bytes > 0 AND size_bytes <= 512000000),
    checksum text NOT NULL CHECK (checksum ~ '^[0-9a-f]{64}$'),
    upload_id text,
    completed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    touched_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (device_id, bucket, object_key, size_bytes, checksum)
);
CREATE UNIQUE INDEX device_upload_active_key ON device_upload_session (bucket, object_key)
    WHERE completed_at IS NULL;
CREATE INDEX device_upload_expiry ON device_upload_session (touched_at);
CREATE TABLE device_upload_part (
    session_id uuid NOT NULL REFERENCES device_upload_session(id) ON DELETE CASCADE,
    part_number integer NOT NULL CHECK (part_number BETWEEN 1 AND 98),
    etag text NOT NULL,
    checksum text NOT NULL CHECK (checksum ~ '^[0-9a-f]{64}$'),
    PRIMARY KEY (session_id, part_number)
);
