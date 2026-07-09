-- A user holds exactly one role. Previously auth.users_roles had
-- PRIMARY KEY (user_id, role), so a user could accumulate several rows — e.g. the
-- `default` baseline plus an elevated role granted from accounts.toml, or two
-- elevated roles. Move the primary key to user_id alone so the schema itself
-- guarantees a single role per user and no code path can ever assign a second.

-- Collapse users who currently hold more than one role down to a single row,
-- keeping their elevated role over the `default` baseline (and, if somehow two
-- elevated roles, keeping one deterministically).
DELETE FROM auth.users_roles t
WHERE t.ctid NOT IN (
    SELECT DISTINCT ON (user_id) ctid
    FROM auth.users_roles
    ORDER BY user_id, (role = 'default'), role
);

ALTER TABLE auth.users_roles DROP CONSTRAINT users_roles_pkey;
ALTER TABLE auth.users_roles ADD CONSTRAINT users_roles_pkey PRIMARY KEY (user_id);

-- The default-role trigger must conflict on the new key.
CREATE OR REPLACE FUNCTION auth.assign_default_role()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO auth.users_roles (user_id, role)
    VALUES (NEW.id, 'default')
    ON CONFLICT (user_id) DO NOTHING;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
