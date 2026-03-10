ALTER TABLE distribution ADD COLUMN latest_release_id INTEGER REFERENCES release(id);
