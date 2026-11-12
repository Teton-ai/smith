-- A deployment is one run with its own history: redeploying a release adds a
-- row instead of restarting the previous one.
ALTER TABLE deployment DROP CONSTRAINT deployment_release_id_key;
CREATE UNIQUE INDEX deployment_one_canary_per_release
    ON deployment (release_id) WHERE status = 'in_progress';
CREATE INDEX deployment_release_id_idx ON deployment (release_id);

ALTER TABLE deployment
    ADD COLUMN kind text NOT NULL DEFAULT 'staged' CHECK (kind IN ('staged', 'direct', 'rollback')),
    -- The distribution's latest release when the deployment started.
    ADD COLUMN base_release_id integer REFERENCES release (id),
    ADD COLUMN created_by integer REFERENCES auth.users (id) ON DELETE SET NULL,
    ADD COLUMN promoted_at timestamptz,
    ADD COLUMN promoted_by integer REFERENCES auth.users (id) ON DELETE SET NULL,
    ADD COLUMN paused_at timestamptz,
    -- Completed rollouts are historical results, independent of current device health.
    ADD COLUMN completed_at timestamptz,
    -- A release candidate's pilot group, by labels or by devices.
    ADD COLUMN pilot_device_labels text[],
    ADD COLUMN pilot_device_ids integer[],
    -- A rollback's yanked release, which its devices were moved off.
    ADD COLUMN rolled_back_from integer REFERENCES release (id),
    -- When a rollout stops waiting for pending devices. Set when its staggered
    -- rollout starts; each pause pushes it back.
    ADD COLUMN times_out_at timestamptz;

UPDATE deployment SET promoted_at = updated_at WHERE status = 'done';
UPDATE deployment SET times_out_at = promoted_at + INTERVAL '1 hour' WHERE promoted_at IS NOT NULL;

ALTER TABLE deployment_devices
    -- 0 is the canary; the staggered rollout's waves count up from 1.
    ADD COLUMN wave integer NOT NULL DEFAULT 0,
    ADD COLUMN from_release_id integer REFERENCES release (id),
    ADD COLUMN opens_at timestamptz NOT NULL DEFAULT now(),
    -- Kept once set, so history survives the device moving on to a later release.
    ADD COLUMN reached_at timestamptz,
    -- Frozen when the deployment completes.
    ADD COLUMN outcome text CHECK (outcome IN ('updated', 'timed_out', 'superseded', 'canceled'));

UPDATE deployment_devices SET opens_at = created_at;

CREATE UNIQUE INDEX deployment_devices_deployment_device_key ON deployment_devices (deployment_id, device_id);
CREATE INDEX deployment_devices_device_id_idx ON deployment_devices (device_id);

CREATE TABLE deployment_event (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    deployment_id integer NOT NULL REFERENCES deployment (id) ON DELETE CASCADE,
    kind text NOT NULL,
    user_id integer REFERENCES auth.users (id) ON DELETE SET NULL,
    detail text,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX deployment_event_deployment_id_idx ON deployment_event (deployment_id, id);

-- Recover confirmations predating reached_at from the upgrade history.
UPDATE deployment_devices dd SET reached_at = observed.at
FROM (
    SELECT dd.deployment_id, dd.device_id, MIN(u.created_at) AS at
    FROM deployment_devices dd
    JOIN deployment dep ON dep.id = dd.deployment_id
    JOIN device_release_upgrades u ON u.device_id = dd.device_id
        AND u.upgraded_release_id = dep.release_id AND u.created_at >= dd.created_at
    GROUP BY dd.deployment_id, dd.device_id
) observed
WHERE dd.deployment_id = observed.deployment_id AND dd.device_id = observed.device_id
    AND dd.reached_at IS NULL;

-- The one definition of what a device is doing in a deployment, so the list,
-- the detail page and the conflict check can never disagree.
CREATE VIEW deployment_device_state AS
SELECT
    dd.deployment_id,
    dd.device_id,
    dd.wave,
    dd.from_release_id,
    dd.opens_at,
    dd.reached_at,
    COALESCE(dd.outcome, CASE
        WHEN dd.reached_at IS NOT NULL OR dev.release_id = dep.release_id THEN 'updated'
        WHEN dev.target_release_id IS DISTINCT FROM dep.release_id OR dev.archived THEN
            CASE WHEN dep.status = 'canceled' THEN 'canceled' ELSE 'superseded' END
        WHEN dep.status = 'canceled' THEN 'canceled'
        WHEN dep.paused_at IS NOT NULL THEN 'held'
        WHEN dev.target_release_eligible_at > now() THEN 'scheduled'
        WHEN dev.last_ping IS NULL OR dev.last_ping < now() - INTERVAL '3 minutes' THEN 'offline'
        -- However long it takes: field connections vary, and it gets there eventually.
        ELSE 'updating'
    END) AS state,
    dep.completed_at IS NULL AND dev.release_id = dep.release_id AND EXISTS (
        SELECT 1 FROM release_services rs
        WHERE rs.release_id = dep.release_id
          AND rs.watchdog_sec IS NOT NULL
          AND NOT EXISTS (
              SELECT 1 FROM device_service_status dss
              WHERE dss.device_id = dev.id
                AND dss.release_service_id = rs.id
                AND dss.active_state = 'active'
          )
    ) AS unhealthy
FROM deployment_devices dd
JOIN deployment dep ON dep.id = dd.deployment_id
JOIN device dev ON dev.id = dd.device_id;
