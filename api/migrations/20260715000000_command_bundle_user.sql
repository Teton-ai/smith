ALTER TABLE public.command_bundles
    ADD COLUMN user_id INTEGER;

ALTER TABLE public.command_bundles
    ADD CONSTRAINT command_bundles_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES auth.users(id) ON DELETE SET NULL NOT VALID;
