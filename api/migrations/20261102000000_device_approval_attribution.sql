-- Reference the account rather than copying its email, so identity stays behind
-- the users:read permission and can be erased by deleting the user.
ALTER TABLE device
    ADD COLUMN approved_by_user_id INT REFERENCES auth.users (id) ON DELETE SET NULL,
    ADD COLUMN approved_at TIMESTAMPTZ;

ALTER TABLE ledger
    ADD COLUMN user_id INT REFERENCES auth.users (id) ON DELETE SET NULL;

-- Past approvals never recorded the approver, but the ledger has their time: the
-- first approval since the last revocation, as repeat requests also wrote entries.
UPDATE device d
SET approved_at = a.approved_at
FROM (
    SELECT l.device_id, MIN(l.timestamp) AS approved_at
    FROM ledger l
    LEFT JOIN (
        SELECT device_id, MAX(timestamp) AS revoked_at
        FROM ledger
        WHERE class = 'revoked'
        GROUP BY device_id
    ) r ON r.device_id = l.device_id
    WHERE l.class = 'approved'
      AND (r.revoked_at IS NULL OR l.timestamp > r.revoked_at)
    GROUP BY l.device_id
) a
WHERE d.id = a.device_id AND d.approved;
