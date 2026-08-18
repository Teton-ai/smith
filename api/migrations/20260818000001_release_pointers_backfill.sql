-- Give every device an explicit release intent.
--
-- Devices already sitting on their distribution's latest become followers.
-- Everything else is pinned exactly where it already is -- which changes no
-- device's behaviour, but converts today's invisible drift into a queryable
-- list (SELECT ... WHERE pinned_release_id IS NOT NULL) that can be worked
-- through one case at a time.
--
-- Order matters: followers are marked first so the second statement can use
-- `follows_latest = false` to mean "not already handled" and stay inside the
-- device_pin_xor_follow constraint.
UPDATE device d
SET follows_latest = true
FROM release r
JOIN distribution dist ON dist.id = r.distribution_id
WHERE r.id = d.target_release_id
  AND dist.latest_release_id = d.target_release_id
  AND d.archived = false;

UPDATE device d
SET pinned_release_id = d.target_release_id
WHERE d.target_release_id IS NOT NULL
  AND d.follows_latest = false;

-- Devices with no target at all are left in neither state on purpose: they
-- resolve to NULL exactly as they do today, and are the population the base
-- pointer is meant to fix at approval time.
