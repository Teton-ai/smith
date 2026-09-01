-- The base OS image a release ships. One per release, always pushed
-- explicitly: an image is never inherited, copied by `promote_release`, or
-- shared between releases, so it has no identity of its own -- hence UNIQUE
-- release_id and ON DELETE CASCADE rather than a catalog table releases point
-- at.
--
-- Rows exist before the bytes do. `POST /releases/{id}/os` inserts the row,
-- initiates an S3 multipart upload and hands back presigned part URLs; the row
-- carries that upload's identity until `/complete` flips it to 'ready'. These
-- are ~20 GB objects, so an interrupted push has to be resumable and an
-- abandoned one reclaimable -- neither is possible without persisting
-- upload_id and part_size.
CREATE TABLE public.os (
    id          serial PRIMARY KEY,
    release_id  integer NOT NULL UNIQUE REFERENCES public.release(id) ON DELETE CASCADE,
    file_name   text NOT NULL,
    -- os/<release_id>/<file_name>, in the packages bucket so the existing
    -- CloudFront distribution and smithd's resumable downloader serve it
    -- unchanged.
    object_key  text NOT NULL,
    -- sha256 of the image, computed by the client before the push. S3 multipart
    -- ETags are the MD5 of concatenated part MD5s, not a content hash, so this
    -- is the only value that can verify 20 GB on the device before it is
    -- unpacked and written to disk.
    checksum    text NOT NULL,
    -- Declared at create, re-verified against head_object at complete.
    size_bytes  bigint NOT NULL,
    status      text NOT NULL DEFAULT 'pending',
    -- S3 multipart upload id. NULL once completed or aborted.
    upload_id   text,
    -- Chunk size this push started with. A resumed push must split the file
    -- into identical byte ranges or the part numbers do not line up, so the
    -- server owns this rather than the client re-deriving it.
    part_size   integer NOT NULL,
    uploaded_at timestamp with time zone,
    user_id     integer REFERENCES auth.users(id),
    created_at  timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT os_status_check CHECK (status IN ('pending', 'ready', 'failed'))
);

-- The sweeper's working set: pushes abandoned mid-upload. An orphaned 20 GB
-- part set bills until AbortMultipartUpload runs.
CREATE INDEX os_pending_idx ON public.os (created_at) WHERE status = 'pending';

-- One row per part S3 has acknowledged. CompleteMultipartUpload needs every
-- (part_number, ETag) pair and rust-s3 exposes no ListParts, so the pairs are
-- recorded here as they land rather than re-read from S3 at completion. This is
-- also what makes a push resumable across CLI invocations: the client asks
-- which parts are already present and uploads only the rest.
CREATE TABLE public.os_part (
    os_id       integer NOT NULL REFERENCES public.os(id) ON DELETE CASCADE,
    part_number integer NOT NULL,
    etag        text NOT NULL,
    size_bytes  bigint NOT NULL,
    uploaded_at timestamp with time zone DEFAULT now() NOT NULL,
    PRIMARY KEY (os_id, part_number)
);
