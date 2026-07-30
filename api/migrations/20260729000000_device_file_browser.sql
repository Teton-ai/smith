-- Cross-replica device session plumbing, plus the remote filesystem browser
-- built on top of it.
--
-- The api runs multiple replicas with no session stickiness, so the sockets
-- involved in one device session routinely land on different processes: a file
-- browse has four (dashboard control, device control, device upload, browser
-- download) and a log stream has two. `session_message` is the relay a replica
-- writes a frame and NOTIFYs, whichever replica owns the other end is LISTENing
-- and picks it up. Both features share it; neither keeps sessions in memory.

CREATE TABLE public.stream_session (
    id uuid PRIMARY KEY,
    -- Which feature owns the session. Kept on the row so one relay, one
    -- sweeper and one lifecycle serve every device session kind.
    kind text NOT NULL,
    device_id integer NOT NULL REFERENCES public.device(id) ON DELETE CASCADE,
    user_id integer,
    device_connected boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    closed_at timestamp with time zone,
    CONSTRAINT stream_session_kind_check CHECK (kind IN ('files', 'logs'))
);

CREATE INDEX stream_session_device_idx ON public.stream_session (device_id);
CREATE INDEX stream_session_created_idx ON public.stream_session (created_at);

-- NOTIFY payloads are capped at 8000 bytes and a directory listing exceeds
-- that, so the frame lives here and the notification carries only this row id.
-- Consumers DELETE ... RETURNING, which makes delivery atomic and self-cleaning.
CREATE TABLE public.session_message (
    id bigserial PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES public.stream_session(id) ON DELETE CASCADE,
    direction text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT session_message_direction_check
        CHECK (direction IN ('to_device', 'to_dashboard'))
);

CREATE INDEX session_message_session_idx ON public.session_message (session_id);
CREATE INDEX session_message_created_idx ON public.session_message (created_at);

-- One row per download handed out. Single-use: `claim_upload` matches on
-- `uploaded_at IS NULL` and sets it in the same statement, so a replayed token
-- matches zero rows. `swept_at` claims the row for the sweeper, which deletes
-- the S3 object before dropping the row so a failed delete stays retryable.
CREATE TABLE public.file_download (
    upload_token text PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES public.stream_session(id) ON DELETE CASCADE,
    op_id bigint NOT NULL,
    object_key text NOT NULL,
    file_name text NOT NULL,
    size bigint NOT NULL,
    uploaded_at timestamp with time zone,
    swept_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE INDEX file_download_session_idx ON public.file_download (session_id);
CREATE INDEX file_download_created_idx ON public.file_download (created_at);

-- Audit trail. Session-level attribution is already free via command_bundles,
-- but per-operation access -- especially downloads -- is not.
CREATE TABLE public.device_file_access (
    id bigserial PRIMARY KEY,
    device_id integer NOT NULL REFERENCES public.device(id) ON DELETE CASCADE,
    user_id integer,
    session_id uuid NOT NULL,
    op text NOT NULL,
    path text NOT NULL,
    bytes bigint,
    outcome text NOT NULL,
    detail text,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE INDEX device_file_access_device_created_idx
    ON public.device_file_access (device_id, created_at DESC);
CREATE INDEX device_file_access_user_created_idx
    ON public.device_file_access (user_id, created_at DESC);
