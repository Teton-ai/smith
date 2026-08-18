-- Release intent, split out from the value served to the daemon.
--
-- `device.target_release_id` used to be both: the operator's intent ("this
-- device should run 4187") and the integer handed back on /smith/home. With
-- one column for both there was nowhere to record *why* a device sat where it
-- did, so a deliberate pin and an accidental drift were indistinguishable --
-- which is what the "Risk of Drift" Slack alert on every direct set is about.
--
-- After this migration the intent lives in the two columns below and
-- `target_release_id` becomes a materialised cache of the resolved answer, so
-- every existing query over it (outdated counts, deployment views) keeps
-- working untouched.
ALTER TABLE device
    ADD COLUMN pinned_release_id integer REFERENCES release(id),
    ADD COLUMN follows_latest boolean NOT NULL DEFAULT false;

-- The two real states are exclusive: a device is held at one release, or it
-- follows the distribution's stream. Nothing is both.
ALTER TABLE device
    ADD CONSTRAINT device_pin_xor_follow
    CHECK (NOT (follows_latest AND pinned_release_id IS NOT NULL));

CREATE INDEX device_pinned_release_idx ON device (pinned_release_id);
CREATE INDEX device_follows_latest_idx ON device (follows_latest) WHERE follows_latest;

-- `base` is what the production line flashes and what a device is pinned to at
-- approval. It deliberately trails `latest_release_id`: a release becomes base
-- only after the following fleet has already soaked it.
--
-- The two pointers have opposite blast radius. Moving `latest_release_id`
-- moves every following device on its next ping; moving `base_release_id`
-- moves nobody, and only changes what devices approved afterwards are born on.
-- That is what makes base cheap to update.
ALTER TABLE distribution
    ADD COLUMN base_release_id integer REFERENCES release(id);

-- Pointer moves are fleet-wide decisions and need attribution, but `ledger` is
-- device-scoped (device_id is NOT NULL) and cannot hold them. This also backs
-- the base gate: promoting to base requires the release to have been latest,
-- which is a question about history, not current state.
CREATE TABLE public.distribution_pointer_history (
    id bigserial PRIMARY KEY,
    distribution_id integer NOT NULL REFERENCES distribution(id),
    pointer text NOT NULL,
    previous_release_id integer REFERENCES release(id),
    new_release_id integer REFERENCES release(id),
    user_id integer,
    reason text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT distribution_pointer_history_pointer_check
        CHECK (pointer IN ('base', 'latest'))
);

CREATE INDEX distribution_pointer_history_distribution_idx
    ON distribution_pointer_history (distribution_id, created_at DESC);
CREATE INDEX distribution_pointer_history_release_idx
    ON distribution_pointer_history (pointer, new_release_id);
