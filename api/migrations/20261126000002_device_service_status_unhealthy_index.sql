-- no-transaction
-- /dashboard/unhealthy-services and GET /devices?service_not_running=true look
-- only for rows where active_state <> 'active', which is a tiny fraction of
-- device_service_status.
CREATE INDEX CONCURRENTLY idx_device_service_status_unhealthy
    ON public.device_service_status (device_id, release_service_id)
    WHERE active_state <> 'active';
