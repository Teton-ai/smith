ALTER TABLE public.release ADD COLUMN release_candidate boolean DEFAULT false NOT NULL;

UPDATE public.release SET release_candidate = true WHERE version LIKE '%-rc%';
