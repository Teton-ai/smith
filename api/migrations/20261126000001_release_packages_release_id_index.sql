-- no-transaction
-- GET /smith/releases/{release_id}/packages filters by release_id, which had no
-- index and fell back to full sequential scans.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_release_packages_release_id
    ON public.release_packages (release_id);
