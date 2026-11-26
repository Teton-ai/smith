-- no-transaction
-- GET /smith/releases/{release_id}/packages and release management queries filter
-- by release_id (and package_id), which had no index and ran sequential scans.
CREATE INDEX CONCURRENTLY idx_release_packages_release_id
    ON public.release_packages (release_id, package_id);
