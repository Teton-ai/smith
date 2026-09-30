-- Sign-offs on a deployment. A full rollout needs at least one approval from an
-- admin other than the user confirming it (enforced in `confirm_full_rollout`).
CREATE TABLE deployment_approval (
    deployment_id int4 NOT NULL REFERENCES deployment(id) ON DELETE CASCADE,
    user_id int4 NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT NOW() NOT NULL,
    PRIMARY KEY (deployment_id, user_id)
);
