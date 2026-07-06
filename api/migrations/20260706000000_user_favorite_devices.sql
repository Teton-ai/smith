CREATE TABLE auth.user_favorite_devices (
    user_id INT NOT NULL REFERENCES auth.users (id) ON DELETE CASCADE,
    device_id INT NOT NULL REFERENCES device (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, device_id)
);
