-- Keep the account identity as a snapshot, even if its email changes later.
-- Previous approvals have no reliable approver or timestamp to backfill.
ALTER TABLE device
    ADD COLUMN approved_by TEXT,
    ADD COLUMN approved_at TIMESTAMPTZ;
