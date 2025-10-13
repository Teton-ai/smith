ALTER TABLE device ADD COLUMN labels jsonb NOT NULL DEFAULT '{}'::jsonb;
