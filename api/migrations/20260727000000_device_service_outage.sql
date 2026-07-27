-- History of things being down on a device: one row per outage, not one row
-- per ping, so a six-month outage costs the same single row as a six-minute one.
--
-- `service_name` is the systemd unit that was down. Device reachability is
-- recorded under the reserved name 'smithd', which is honest rather than a
-- special case: smithd is a real unit (smithd/debian/smithd.service), and if it
-- is not running, nothing checks in. It is the one unit whose failure must be
-- inferred from silence instead of reported, since a dead agent cannot report
-- its own death -- so those rows are written by the API sweeper and every other
-- name is written from the device's own service reports.
--
-- The name is denormalised rather than referencing release_services(id), whose
-- id differs per release: an outage spanning an OTA upgrade would otherwise
-- fracture across two ids for the same logical service.
CREATE TABLE IF NOT EXISTS public.device_service_outage (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    device_id    integer NOT NULL REFERENCES public.device(id) ON DELETE CASCADE,
    service_name text NOT NULL,
    started_at   timestamptz NOT NULL,
    ended_at     timestamptz,
    CONSTRAINT device_service_outage_order CHECK (ended_at IS NULL OR ended_at >= started_at)
);

-- At most one open outage per service per device. This is also what makes the
-- sweeper's `INSERT ... ON CONFLICT DO NOTHING` safe when several API replicas
-- sweep concurrently, so no leader election is needed.
CREATE UNIQUE INDEX IF NOT EXISTS device_service_outage_open
    ON public.device_service_outage (device_id, service_name)
    WHERE ended_at IS NULL;

CREATE INDEX IF NOT EXISTS device_service_outage_window
    ON public.device_service_outage (device_id, service_name, started_at DESC);
