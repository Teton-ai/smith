ALTER TABLE public.command_bundles
    ADD COLUMN user_id INTEGER REFERENCES auth.users(id) ON DELETE SET NULL;
