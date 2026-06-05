CREATE TABLE command_recipes (
    id          SERIAL PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE CHECK (btrim(name) <> ''),
    description TEXT,
    commands    JSONB NOT NULL CHECK (jsonb_typeof(commands) = 'array'),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
