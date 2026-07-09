ALTER TABLE device
    ADD COLUMN intent_version          INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN observed_intent_version INTEGER,
    ADD COLUMN network_conditions      JSONB;
