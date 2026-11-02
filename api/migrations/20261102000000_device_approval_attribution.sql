-- Reference the account rather than copying its email, so identity stays behind
-- the users:read permission and can be erased by deleting the user.
-- Previous approvals have no reliable approver or timestamp to backfill.
ALTER TABLE device
    ADD COLUMN approved_by_user_id INT REFERENCES auth.users (id) ON DELETE SET NULL,
    ADD COLUMN approved_at TIMESTAMPTZ;

ALTER TABLE ledger
    ADD COLUMN user_id INT REFERENCES auth.users (id) ON DELETE SET NULL;
